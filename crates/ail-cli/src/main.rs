mod changeset_input;
mod cli;
mod error;
mod output;
mod package_commands;
mod package_output;
mod package_registry_io;
mod project;
mod remote_commands;
mod remote_config;
mod store;
mod workflow_commands;

use error::exit_code;

#[tokio::main]
async fn main() {
    if let Err(err) = cli::run().await {
        eprintln!("ail: {err}");
        std::process::exit(exit_code(&err));
    }
}
