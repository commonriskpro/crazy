mod changeset_input;
mod cli;
mod error;
mod output;
mod project;
mod remote_config;
mod store;

use error::exit_code;

#[tokio::main]
async fn main() {
    if let Err(err) = cli::run().await {
        eprintln!("ail: {err}");
        std::process::exit(exit_code(&err));
    }
}
