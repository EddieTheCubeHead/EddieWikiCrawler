use std::sync::{Arc, RwLock, mpsc};
use std::collections::{HashSet, HashMap};
use std::thread;
use std::time::Duration;
use std::io::{stdout, Write};

use tokio;

use super::wiki_api;

/// A struct that should be used to build the tree of which the result of the crawl consists
pub struct ArticleNode {
    name: String,
    parent: Option<Arc<ArticleNode>>,
}

impl ArticleNode {
    /// A builder funtion for ArticleNode
    /// 
    /// # Arguments
    /// 
    /// * 'name' - A string slice that contains the name of the node
    /// * 'parent' - An option that has an arc containing the parent node of the new node, if it has one
    /// 
    /// # Returns
    /// 
    /// * ArticleNode - A new article node created from the given parameters
    fn new(name: &str, parent: Option<Arc<ArticleNode>>) -> ArticleNode {
        let name = name.to_string();
        ArticleNode { name, parent }
    }
}

/// A struct that should be used to transfer analysis results from worker threads back to the main thread
struct BatchData {
    parent: Option<Arc<ArticleNode>>,
    new_batch: Vec<String>,
}

impl BatchData {
    /// A builder function for BatchData
    /// 
    /// # Arguments
    /// 
    /// * 'parent' - An option that has the parent for the future ArticleNodes spawned from the result
    /// * 'new_batch' - A Vec that houses String representations of the new articles to be queried in main thread
    /// 
    /// # Returns
    /// 
    /// * BatchData - A new batch data struct created from the given parameters
    fn new(parent: Option<Arc<ArticleNode>>, new_batch: Vec<String>) -> BatchData {
        BatchData { parent, new_batch }
    }
}

/// A struct that houses the data of a crawl shared between main thread and worker threads
/// Should always be housed in an arc while crawling
pub struct Crawler {
    origin: ArticleNode,
    goal: String,
    visited: RwLock<HashSet<String>>,
    finished: RwLock<u8>,
    final_node: RwLock<Option<ArticleNode>>
}

impl Crawler {
    /// A constructor for Crawler that automatically wraps the created Crawler in an Arc
    /// Note that creating a crawler doesn't automatically start a crawl, instead call start for that
    /// 
    /// # Arguments
    /// 
    /// * 'origin' - A string slice with the name of the origin article of the crawl
    /// * 'goal' - A string slice with the name of the goal of the crawl
    /// 
    /// # Returns
    /// 
    /// * Arc<Crawler> - An Arc that has the created Crawler instance wrapped inside it
    pub fn new_arc(origin: &str, goal: &str) -> Arc<Crawler> {
        let mut visited_set: HashSet<String> = HashSet::new();
        visited_set.insert(origin.to_string());
        Arc::new( Crawler {
            origin: ArticleNode::new(origin, None),
            goal: goal.to_string(),
            visited: RwLock::new(visited_set),
            finished: RwLock::new(0),
            final_node: RwLock::new(None),
        })
    }
}

/// An async function that performs the actual crawl by spawning an UI thread and worker threads when necessary.
/// Wikipedia API calls are performed on the main thread to satisfy the rate limits of the API
/// 
/// # Arguments
/// 
/// * 'crawler_arc' - An arc that houses the Crawler struct used for data transfer between main thread and workers
/// * 'api' - A reference to a logged in mediawiki::api::Api instance
/// 
/// # Returns
/// 
/// * Option<Vec<String>> - An option that holds a Vec of Strings of the shortest path, or None if error occurred
pub async fn start(crawler_arc: Arc<Crawler>, api: &mediawiki::api::Api) -> Option<Vec<String>> {
    let crawler_display_clone = Arc::clone(&crawler_arc);

    // When this buffer fills child threads are forced to wait to dispatch their data. This means the program 
    // will be bottlenecked by the API rate limit after that, slowing it down significantly. Considering this
    // A buffer of 50000 seems more than justified
    let (sender, reciever) = mpsc::sync_channel::<BatchData>(50000);

    let display_processing_handle = thread::spawn(move || {
        display_process(&crawler_display_clone);
    });

    // Init the process by fetching the first bunch of links and initing the sender
    match sender.clone().send(BatchData::new(None, vec!(crawler_arc.origin.name.clone()))) {
        Ok(_) => (),
        Err(error) => {
            eprintln!("An error occurred while initing the first crawl link fetch batch:\n{:?}", error);
            return None;
        },
    };
    drop(api);

    let mut thread_handlers = vec!();

    // Ensure something wonky doesn't happen to the channel by forcing quit after 5 failed recieves
    let mut channel_failsafe: u8 = 0;

    loop {
        let loop_crawler = crawler_arc.clone();
        let finish_read = match loop_crawler.finished.read() {
            Ok(read_lock) => read_lock,
            Err(error) => {
                eprintln!("Error fetching read lock for finish shate check in main thread:\n{:?}", error);
                continue;
            },
        };
            if *finish_read != 0 {
                break;
            }
            drop(finish_read);

        let to_analyse = match reciever.recv() {
            Ok(batch) => {
                channel_failsafe = 0;
                batch
            },
            Err(error) => {
                eprintln!("Error recieving next batch from channel:");
                eprintln!("{:?}\nDropping batch and fetching next one...", error);
                channel_failsafe += 1;
                if channel_failsafe >= 5 {
                    return None;
                }
                continue;
            }
        };

        if to_analyse.new_batch.len() == 0 {
            continue;
        }

        let new_batches = match wiki_api::get_links(&to_analyse.new_batch, api).await {
            Ok(map) => map,
            Err(error) => {
                eprintln!("Error occurred while fetching links: {:?}", error);
                continue;
            }
        };
        let parent = to_analyse.parent.clone();
        let sender_clone = sender.clone();

        let new_handle = tokio::spawn(async move {
            threaded_processing(loop_crawler, new_batches, parent, sender_clone).await;
        });

        thread_handlers.push(new_handle);
    }

    match display_processing_handle.join() {
        Ok(_) => (),
        Err(error) => {
            eprintln!("Fatal error while closing display thread:\n{:?}", error);
            return None;
        },
    }

    drop(reciever);

    for handler in thread_handlers {
        match handler.await {
            Ok(_) => (),
            Err(error) => {
                eprintln!("Fatal error while waiting for all threads to close during crawl cleanup:{:?}", error);
                return None;
            },
        };
    }

    let crawler_raw = match Arc::try_unwrap(crawler_arc) {
        Ok(crawler) => crawler,
        Err(_) => {
            eprintln!("Fatal error while attempting to unwrap crawler during crawl cleanup.");
            return None
        },
    };
    detravel_path(crawler_raw).await
}

/// A function that handles the crawl UI component (keeping the user entertained with pretty blinking text)
/// 
/// # Arguments
/// 
/// * 'crawler_arc' - A Crawler struct wrapped in an arc for data transfer between threads
pub fn display_process(crawler_arc: &Arc<Crawler>) {
    print!("\n");
    loop {

        let total_analysed: usize;
        {         
            let read_set = match crawler_arc.visited.read() {
                Ok(read_lock) => read_lock,
                Err(error) => {
                    eprintln!("Error acquiring read lock for visited set size:\n{:?}", error);
                    thread::sleep(Duration::from_millis(1000));
                    continue;
                },
            };
            total_analysed = (*read_set).len();
            drop(read_set);
        }

        print!("\rCrawling, analyzed {} articles.  ", total_analysed);
        let _ = stdout().flush();

        thread::sleep(Duration::from_millis(600));

        print!("\rCrawling, analyzed {} articles.. ", total_analysed);
        let _ = stdout().flush();

        thread::sleep(Duration::from_millis(600));

        print!("\rCrawling, analyzed {} articles...", total_analysed);
        let _ = stdout().flush();

        thread::sleep(Duration::from_millis(800));

        let finish_read = match crawler_arc.finished.read() {
            Ok(read_lock) => read_lock,
            Err(error) => {
                eprintln!("Error acquiring read lock to check display thread health:\n{:?}", error);
                continue;
            },
        };
        if *finish_read != 0 {
            println!("\nArticle found! Tidying up some threads. This may take time...");
            break;
        }
    }
}

/// A function that takes a raw crawler (unwrapped from an arc at the end of a crawl) and travels backwards from
/// it's final node to construct a path from the origin to the goal
/// 
/// # Arguments
/// 
/// * 'crawler' - A Crawler struct representing a finished crawl
/// 
/// # Returns
/// 
/// * Option<Vec<String>> - An option that holds the final path as a Vec of Strings representing article names
pub async fn detravel_path(crawler: Crawler) -> Option<Vec<String>> {
    let mut _traverse_node = match crawler.final_node.into_inner() {
        Ok(option) => match option {
            Some(node) => node,
            None => {
                eprintln!("Error while fetching goal node: no node");
                return None
            },
        },
        Err(error) => {
            eprintln!("Error while fetching goal node: failure in getting lock inner object:\n{:?}", error);
            return None
        },
    };

    let mut constructed: Vec<String> = vec!();

    loop {
        constructed.push(_traverse_node.name.clone());
        _traverse_node = match _traverse_node.parent {
            Some(arc) => match Arc::try_unwrap(arc) {
                Ok(node) => node,
                Err(error_node) => {
                    eprintln!("Error while traveling path backwards: Unable to unwrap node {:?}:",
                                error_node.name);
                    return None
                },
            },
            None => break,
        };
    }

    constructed.reverse();
    Some(constructed)
}

/// A function that takes data from the main thread and analyses it in a separate one, returning the results to the
/// main thread for later use for fetching more articles. Represents the individual worker nodes of the program
/// 
/// # Arguments
/// 
/// * 'crawler_arc' - A Crawler struct wrapped in an Arc for inter-thread communication
/// * 'new_batches' - A HashMap of String - Vec<String> pairs that houses articles and their respective links
/// * 'parent' - The ArticleNode that should be the parent of the ArticleNodes spawned from the data in new_batch
/// * 'sender' - A SyncSender for sending BatchData instances back to main thread
async fn threaded_processing(crawler_arc: Arc<Crawler>, new_batches: HashMap<String, Vec<String>>,
                                parent: Option<Arc<ArticleNode>>, sender: mpsc::SyncSender<BatchData>) -> () { 

    for (article, links) in new_batches.iter() {
        
        for candidate in links.iter() {
            if candidate == &crawler_arc.goal {
                const MAX_TRIES: u8 = 10;
                let mut tries = 0;
                let mut finished = loop {
                    match crawler_arc.finished.write() {
                        Ok(write_lock) => break write_lock,
                        Err(error) => {
                            eprintln!("Error acquiring write lock for finish state (try {} out of {}):\n{:?}",
                                        tries, MAX_TRIES, error);
                        }
                    }
                    if tries >= MAX_TRIES {
                        panic!("Fatal error: failed to acquire write lock for finish state after {} tries.",
                                tries);
                    }
                    tries += 1;
                };
                *finished = 1;
                drop(finished);
                tries = 0;

                let mut node_lock = loop {
                    match crawler_arc.final_node.write() {
                        Ok(write_lock) => break write_lock,
                        Err(error) => {
                            eprintln!("Fatal error acquiring write lock for final node (try {} out of {}):\n{:?}",
                                        tries, MAX_TRIES, error);
                        }
                    }
                    if tries >= MAX_TRIES {
                        panic!("Fatal error: failed to acquire write lock for finish state after {} tries.",
                                tries);
                    }
                    tries += 1;
                };
                let temp_node = Arc::new(ArticleNode::new(article, parent.clone()));
                *node_lock = Some(ArticleNode::new(candidate, Some(temp_node.clone())));
                return;
            }

        }

        let article_node = ArticleNode::new(article, parent.clone());
        let article_node = Arc::new(article_node);

        for link_batch in paginate_links(links, &crawler_arc) {
            let article_node_clone = Arc::clone(&article_node);
            match sender.send(BatchData::new(Some(article_node_clone), link_batch)) {
                Ok(_) => (),

                // Note that finding the correct result will close the reciever. This WILL cause an error here
                Err(outer_error) => {
                    let finished = match crawler_arc.finished.read() {
                        Ok(read_lock) => read_lock,
                        Err(error) => {
                            eprintln!("Error acquiring read lock to check finished state:\n{:?}", error);
                            return;
                        },
                    };
                    if *finished == 1 {
                        return;
                    }
                    eprintln!("Error while sending data back to main thread:\n{:?}", outer_error);
                },
            }
        }
    };
}

/// A function that takes a list of all links in an article and divides them into pieces small enough for the
/// wikipedia API to handle
/// 
/// # Arguments
/// 
/// * 'links' - A reference to a Vec holding Strings representing all the links found from one article
/// * 'crawler_arc' - A reference to an arc housing a Crawler instance for inter-thread communication
/// 
/// # Returns
/// 
/// * Vec<Vec<String>> - A Vec holding Vecs of Strings representing the broken down link bunches
fn paginate_links(links: &Vec<String>, crawler_arc: &Arc<Crawler>) -> Vec<Vec<String>> {
    // The request data without the title string for the en.wikipedia api is 105 chars
    // I am leaving 20 chars extra space to ensure smooth operation in all conditions.
    // Most of the time the 50 article cap is met before the 2000 char cap, but one
    // cannot be too careful (2000 / 50 = 40, after all, a valid article name length)
    const MAX_URI: usize = 2000;
    const QUERY_LENGTH: usize = 105;
    const GRACE_SPACE: usize = 20;
    const MAX_LINKS: usize = 50;

    let max_chars: usize = MAX_URI - QUERY_LENGTH - GRACE_SPACE;
    let mut available_chars: usize = max_chars;
    let mut current_vector: usize = 0;
    let mut link_count: usize = 0;
    let mut link_batches: Vec<Vec<String>> = vec!();

    let new_vector: Vec<String> = vec!();
    link_batches.push(new_vector);

    let mut tries: u8 = 0;
    const MAX_TRIES: u8 = 10;
    let mut visited_lock = loop {
        match crawler_arc.visited.write() {
            Ok(write_lock) => break write_lock,
            Err(error) => {
                eprintln!("Error acquiring write lock for visite articles(try {} out of {}):\n{:?}",
                            tries, MAX_TRIES, error);
            }
        }

        if tries >= MAX_TRIES {
            panic!("Couldn't acquire write lock for visited articles after {} tries, terminating thread...",
                    tries)
        }

        tries += 1;
    };
    for link in links {

        if (*visited_lock).contains(link) {
            continue;
        }

        (*visited_lock).insert(link.to_string());

        link_count += 1;
        if (available_chars < link.len() + 1) | (link_count > MAX_LINKS) {
            available_chars = max_chars;
            link_count = 1;
            current_vector += 1;

            let new_vector: Vec<String> = vec!();
            link_batches.push(new_vector);
        } else {
            available_chars -= 1;
        }

        available_chars -= link.len();
        link_batches[current_vector].push(link.to_string())
    }
    drop(visited_lock);
    link_batches
}