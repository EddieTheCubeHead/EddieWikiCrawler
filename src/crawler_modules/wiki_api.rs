use std::collections::HashMap;
use std::error::Error;
use std::io;

use serde_json;
use mediawiki;

use super::user_interface;

// https://stackoverflow.com/questions/65976432/how-to-remove-first-and-last-character-of-a-string-in-rust
// This is required, because wikipedia API always surrounds the titles with quotes

/// A function for stripping the surrounding quotes from all the strings recieved from wikipedia
/// 
/// # Arguments
/// 
/// * 'quoted' - The quoted string slice recieved from wikipedia API
/// 
/// # Returns
/// 
/// * &str - An unquoted string slice of the original (=first and last character removed)
fn strip_quotes(quoted: &str) -> &str {
    let mut chars = quoted.chars();
    chars.next();
    chars.next_back();
    chars.as_str()
}

/// An async function that takes a string and validates it by searching wikipedia for it.
/// 
/// Returns the same string if it represents an article title verbatim, or queries user for replacement articles
/// with similiar names and returns the article gotten this way if one is found. Otherwise returns None
/// 
/// # Arguments
/// 
/// * 'article' - A string slice of the article name
/// * 'api' - A reference to a logged in mediawiki::api::Api instance
/// 
/// # Returns
/// 
/// * Result<Option<String>, mediawiki::media_wiki_error::MediaWikiError> - A result with a string option inside
///     containing a valid article or None if no article found
pub async fn validate_article(article: &str, api: &mediawiki::api::Api) 
    -> Result<Option<String>, mediawiki::media_wiki_error::MediaWikiError> {

    let query_map = api.params_into(&[
        ("action", "query"),
        ("format", "json"),
        ("list", "search"),
        ("srsearch", article),
        ("srnamespace", "0"),
        ("srlimit", "5"),
    ]);

    let result = api.get_query_api_json(&query_map).await?;

    // Super simple private function to remove doubled code below
    fn local_exit(article: &str) -> Result<Option<String>, mediawiki::media_wiki_error::MediaWikiError> {
        println!("Input: '{}' didn't match any articles. Cancelling operation...\n", article);
        return Ok(None)
    }

    // Parse result
    let found_articles = match result["query"].as_object() {
        Some(object) => match object.get("search") {
            Some(query) => query,
            None => return local_exit(article),
        },
        None => return local_exit(article),
    };

    let articles_array = match found_articles.as_array() {
        Some(array) => array,
        None => {
            eprintln!("Error while unwrapping query results during article name validation!");
            return Ok(None);
        },
    };
        
        
    let found_articles: Vec<String> = articles_array  
        .iter()
        .map(|article| {
            let quoted = article["title"].to_string();
            strip_quotes(&quoted).to_string()
        }).collect();

    match found_articles.get(0) {
        Some(best_result) => {
            if best_result == article {
                return Ok(Some(article.to_string()));
            }
        },
        None => {
            println!("Didn't find any articles with name '{}', terminating. Operation", article);
            return Ok(None);
        },
    }

    

    let mut prompt = String::new();
    prompt.push_str("\nDidn't find an article matching exact string '");
    prompt.push_str(article);
    prompt.push_str("', did you mean one of these articles:\n");
    
    let mut iterator: u8 = 0;
    for article_name in found_articles.iter() {
        iterator += 1;
        prompt.push_str(&iterator.to_string());
        prompt.push_str(": ");
        prompt.push_str(article_name);
        prompt.push_str("\n");
    }

    prompt.push_str("0: None of the above.\nPlease input a number representing your intent: ");

    loop {
        match user_interface::get_user_input(&prompt).await {
            Some(string) => match string.parse::<u8>() {
                Ok(0) => {
                    println!("Didn't find requested article.");
                    break;
                }
                Ok(num) => {
                    if num > iterator {
                        println!("Invalid input.");
                        continue
                    }
                    
                    match found_articles.get(usize::from(num-1)) {
                        Some(string) => return Ok(Some(string.to_string())),
                        None => {
                            println!("Something went wrong while fetching string.")
                        }
                    }
                },
                Err(_) => {
                    println!("Please give a whole number between 0 and {}", iterator);
                }
            }
            None => {
                println!("Something went wrong while reading input!");
            }
        };
        println!("Please try again.\n");
    }

    println!("Cancelling operation...");
    Ok(None)
}

/// An sync func that fetches all the links from a given Vec of strings
/// 
/// # Arguments
/// 
/// * 'articles' - A reference to a Vec of Strings containing the articles of which links' should be queried
/// * 'api' - A reference to a logged in mediawiki::api::Api instance
/// 
/// # Returns
/// 
/// * Result<HashMap<String, Vec<String>>, Box<dyn Error>> - A result containing a HashMap of String Vec<String> 
///     pairs with the articles paired up with their links
pub async fn get_links(articles: &Vec<String>, api: &mediawiki::api::Api) 
    -> Result<HashMap<String, Vec<String>>, Box<dyn Error>> {

    let articles_string = articles.join("|");
    let mut result_map: HashMap<String, Vec<String>> = HashMap::new();

    let result = fetch_links_from_api(&articles_string, api).await?;

    // Local error handling
    fn construct_error(articles: &str) -> Box<dyn Error> {
        let mut error_string = String::from("Error while fetching link data with the article collection '");
        error_string.push_str(articles);
        error_string.push_str("'\n");
        Box::new(io::Error::new(io::ErrorKind::Other, error_string))
    }

    // Parse result
    let found_pages_wrapped = match result["query"].as_object() {
        Some(object) => match object.get("pages") {
            Some(query) => query.as_object(),
            None => return Err(construct_error(&articles_string)),
        },
        None => return Err(construct_error(&articles_string)),
    };

    let found_pages = match found_pages_wrapped {
        Some(pages) => pages,
        None => return Err(construct_error(&articles_string)),
    };

    for (_, page) in found_pages.iter() {
        let links_array = match page["links"].as_array() {
            Some(array) => array,
            None => continue,
        };
        let page_links: Vec<String> = links_array
            .iter()
            .map(|article| {
                let quoted = article["title"].to_string();
            strip_quotes(&quoted).to_string()
            }).collect();

        let page_name = strip_quotes(&page["title"].to_string()).to_string();

        result_map.insert(page_name, page_links);
    }
    Ok(result_map)
}

/// An async func to be used with get_links to perform the actual wikipedia api query
/// 
/// # Arguments
/// 
/// * 'articles_string' - A string slice containing all the articles that should be queried separated by pipes
/// * 'api' - A reference to a logged in instance of mediawiki::api::Api
/// 
/// # Returns
/// 
/// * Result<serde_json::Value, Box<dyn Error>> - A result containing a serde_json::Value that has the query result
async fn fetch_links_from_api(articles_string: &str, api: &mediawiki::api::Api) 
    -> Result<serde_json::Value, Box<dyn Error>> {
    
    let query_map = api.params_into(&[
        ("action", "query"),
        ("format", "json"),
        ("titles", &articles_string),
        ("prop", "links"),
        ("pllimit", "max"),
        ("plnamespace", "0"),
        ]);

    let results = api.get_query_api_json_all(&query_map).await?;

    Ok(results)
}
