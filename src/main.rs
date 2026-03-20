use clap::Parser;

mod crypto;
mod types;
mod consensus;
mod chain;
mod storage;
mod network;
mod wallet;
mod tui;
mod cli;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args = cli::commands::Cli::parse();

    if let Err(e) = cli::commands::run(args).await {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
