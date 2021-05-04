# EddieWikiCrawler
The final assignment for the distributed systems course of LUT-University

The project is a wikipedia crawler that searches the shortest path between two articles utilizing worker threads analyzing data and a server thread
fetching data from wikipedia api and spawning new worker threads.

## Running the program

To run the program you need the rust environment (at least 1.51.0), you can get it from [the official Rust website](https://www.rust-lang.org/)

There are two ways to build and run a rust project, development build (quick build, unoptimized executable) or production build (slow build, 
optimized executable). Both are initiated with the rust package manager cargo. This is controlled by the --release -tag in run/build commands.
It is recommended to use the release build, unless you plan to make modifications to the program, in which case the faster build time of the dev
build comes in handy.

You can either run the project with a single command, or build the project first and then run the executable. These happen by running the following
commands in the project root directory, with run running the program straight away and build only building the corresponding executable:

> cargo run [--releae]
> cargo build [--release]

## Using the program

Dev build executable can be found in root/target/debug and production build executable in root/target/release. You can run them as normal executable
files. They take one optional argument: api_path. If you don't want to use the default (https://en.wikipedia.org/w/api.php) you can specify a new API
path for the program to use here. If you stay in the root folder you can run the program with one of the following commands

#### Release build

> ./target/release/eddie_crawler [api_path]

#### Dev build

> ./target/debug/eddie_crawler [api_path]

## Providing secrets

The bot requires a mediawiki api bot account. You can find exact instructions for creating a bot account (here)[https://www.mediawiki.org/wiki/Manual:Bot_passwords].

The bot password is tied to your account so make sure your account has standard login rights to the api path you are planning to use.

Once you have the account username (in the form of YourAccount@BotName) and the bot password, you should write them in lines 1 and 2 in a file called 'secrets.txt' **in
the project root directory**. The first line contains the bot username and the second contains the password. The bot doesn't care about the contents of the rest of the file.
