mod cli;
mod config;
mod deps;
mod discover;
mod library;
mod media;
mod models;
mod output;

use clap::Parser;

fn main() {
    let cli = cli::Cli::parse();

    if let Err(error) = cli.run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
