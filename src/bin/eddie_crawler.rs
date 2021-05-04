extern crate eddie_crawler;

use crate::eddie_crawler::crawler_modules::user_interface;

use std::env;
use tokio;

#[tokio::main]
async fn main() {
    let args = env::args();
    if let Err(error) = user_interface::run(args).await {
        eprintln!("Fatal error: {}", error);
        eprintln!("Exiting program...")
    } else {
        println!("Thank you for using EddieWikiCrawler.");
    }
}