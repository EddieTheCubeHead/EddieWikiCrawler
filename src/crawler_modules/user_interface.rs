use super::{configs, crawler, wiki_api};
use std::fs;
use std::env;
use std::io;
use std::io::{stdout, Write};
use std::error::Error;
use std::path::Path;

use mediawiki;

pub const SECRETS: &str = "./secrets.txt";

/// A struct containing the username and password of the bot account to use with the crawler
#[derive(PartialEq, Debug)]
pub struct BotLoginData {
    pub username: String,
    pub password: String,
}

impl BotLoginData {
    /// A function for reading a file and returning a BotLoginData from the contents
    /// 
    /// # Arguments
    /// 
    /// * 'secret_file' - A string slice containing the file name
    /// 
    /// # Returns
    /// 
    ///  * Option<BotLoginData> - An option containing the received login data, if found
    fn get_login_from_file(secret_file: &Path) -> Option<BotLoginData> {
        let file_contents = fs::read_to_string(secret_file);

        let file_contents = match file_contents {
            Ok(file_contents) => file_contents,
            Err(error) => {
                eprintln!("Error while opening the file'{:?}':\n{:?}", secret_file, error);
                return None;
            },
        };

        // https://stackoverflow.com/questions/37547225/split-a-string-and-return-vecstring
        let file_rows: Vec<String> = file_contents.split("\n").map(|s| s.to_string()).collect();

        let username = match file_rows.get(0) {
            Some(string) => string.trim().to_string(),
            None => return None,
        };

        let password = match file_rows.get(1) {
            Some(string) => string.trim().to_string(),
            None => return None,
        };

        Some(BotLoginData { username, password })
    }
}

/// An async function for running the program, should be the only one called in main
/// 
/// # Arguments
/// 
/// * 'config' - A Config struct with the config data of the program
/// 
/// # Returns
/// 
/// * Result<(), Box<dyn Error>> - Result containing possible errors
pub async fn run(args: env::Args) -> Result<(), Box<dyn Error>> {
    let config = configs::Config::new(args);
    let login_data = match BotLoginData::get_login_from_file(Path::new(SECRETS)) {
        Some(result) => result,
        None => return Err(Box::new(io::Error::new(io::ErrorKind::Other, 
                                               "Fatal error: didn't find bot login credentials in secret file!"))),
    };

    start_cli(config, login_data).await
}

/// An async function for initializing the api and starting the command line interface loop
/// 
/// # Arguments
/// 
/// * 'config' - A Config struct with the config data of the progarm
/// * 'login_data' - A BotLoginData struct containing the login data of the bot account to be used
/// 
/// # Returns
/// 
/// * Result<(), Box<dyn Error>> - Result containing possible errors
async fn start_cli(config: configs::Config, login_data: BotLoginData) -> Result<(), Box<dyn Error>> {
    println!("Opening api connection and logging in...");
    let mut api = mediawiki::api::Api::new(&config.api_path).await?;
    api.login(&login_data.username, &login_data.password).await?;
    println!("Logged in as '{}'", &login_data.username);

    core_loop(api).await
}

/// An async function responsible for running the cli loop at the core of the program
/// Designed to be easily expandable if I continue development after the assignment
/// 
/// # Arguments
/// 
/// * 'api' - Mutable mediawiki::api::Api struct with a logged in bot account
/// 
/// # Returns
/// 
/// * Result<(), Box<dyn Error>> - Result containing possible errors
async fn core_loop(mut api: mediawiki::api::Api) -> Result<(), Box<dyn Error>> {
    let prompt = r#"
Welcome to EddieWikiCrawler, a tool for finding the shortest path between two wikipedia articles.
    
Choose your operation:
1: Start a new crawl
0: Exit
Your choice: "#;
    loop {
        let user_choice_string: String;
        match get_user_input(prompt).await {
            Some(string) => user_choice_string = string,
            None => {
                println!("Something went wrong while reading input! Please try again.");
                continue;
            }
        }

        match user_choice_string.parse::<u8>() {
            Err(_) => {
                println!("Please type a number between 0 and 2!");
                continue;
            },
            Ok(0) => {
                println!("Exiting program...");
                break
            },
            Ok(1) => api = crawl(api).await?,
            Ok(_) => {
                println!("Please type a number between 0 and 2!");
                continue;
            }
        }
    }
    
    Ok(())
}

/// An async func that starts the crawling process. Should be called from the core loop
/// 
/// # Arguments
/// 
/// * 'api' - A logged in mediawiki::api::Api instance
/// 
/// # Returns
/// 
/// * Resulut<mediawiki::api::Api, Box<dyn Error>> - Result returning the borrowed api or containing error data
async fn crawl(api: mediawiki::api::Api) 
    -> Result<mediawiki::api::Api, Box<dyn Error>> {

    let (origin, goal) = match query_names().await {
        Some(tuple) => tuple,

        // Raising an error manually takes some serious work in rust, huh?
        None => return Err(Box::new(io::Error::new(io::ErrorKind::Other,
            "Error while getting article names from user."))),
    };

    println!("\nValidating given articles' existence...\n");

    let origin = match wiki_api::validate_article(&origin, &api).await {
        Ok(result) => match result {
            Some(string) => string,
            None => return Ok(api),
        },
        Err(error) => return Err(Box::new(error)),
    };

    let goal = match wiki_api::validate_article(&goal, &api).await {
        Ok(result) => match result {
            Some(string) => string,
            None => return Ok(api),
        },
        Err(error) => return Err(Box::new(error)),
    };

    if origin == goal {
        println!("Please input two different articles.");
        return Ok(api);
    }

    let crawler_arc = crawler::Crawler::new_arc(&origin, &goal);
    let result_route = match crawler::start(crawler_arc, &api).await {
        Some(path) => path,
        None => {
            eprintln!("Error: something went wrong while traversing the path backwards to complete an answer.");
            return Ok(api);
        },
    };
    pretty_print_path(result_route);
    Ok(api)
}

/// A function for formatting the path while printing it to the user
/// 
/// # Arguments
/// 
/// * 'path' - A Vec of String instances containing the articles in the path from origin to goal
fn pretty_print_path(path: Vec<String>) -> () {
    if path.len() < 2 {
        println!("Error: path should contain at least two articles!");
    }

    print!("{}", path[0]);

    for article in &path[1..] {
        print!(" -> {}", article);
    }
    print!{"\n"};
}

/// A function for getting two article names from the user
/// 
/// # Returns
/// 
/// * Option<(String, String)> - An option tuple of the recieved strings, None in the case of error
async fn query_names() -> Option<(String, String)> {
    let start_article = match get_user_input("Give the name of the starting article: ").await {
        Some(string) => {
            string
        },
        None => {
            println!("Something went wrong while reading input!");
            return None;
        },
    };

    let goal_article = match get_user_input("Give the name of the finishing article: ").await {
        Some(string) => string,
        None => {
            println!("Something went wrong while reading input!");
            return None;
        },
    };

    Some((start_article, goal_article))
}

// https://users.rust-lang.org/t/how-to-get-user-input/5176/8

/// A function for simply recieving user input. Basically functions like python's input()
/// 
/// # Arguments
/// 
/// 'prompt' - A string slice to prompt the user with while querying input
/// 
/// # Returns
/// 
/// * Option<String> - An Option containing the recieved String or None in the case of error
pub async fn get_user_input(prompt: &str) -> Option<String> {
    print!("{}", prompt);
    let _ = stdout().flush();
    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        Ok(_) => {},
        Err(_) => return None,
    }
    Some(input.trim().to_string())
}
