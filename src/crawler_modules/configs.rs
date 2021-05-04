use std::env;

pub const DEFAULT_API_PATH: &str = "https://en.wikipedia.org/w/api.php";

/// Struct representing the configs of the program
pub struct Config {
    pub api_path: String,
}

impl Config {

    /// Constructs a config struct out of the given arguments
    /// 
    /// # Arguments
    /// 
    /// * 'args' - An env::Args iterator
    /// 
    /// # Returns
    /// 
    /// * Config - A new Config instance
    pub fn new(mut args: env::Args) -> Config {

        // Consume program name
        args.next();

        let api_path = match args.next() {
            Some(string) => string.to_string(),
            None => {
                println!("Didn't find api path in args, using the default: '{}'", DEFAULT_API_PATH);
                DEFAULT_API_PATH.to_string()
            },
        };

        Config { api_path }
    }
}
