mod approval_commands;
mod branch_commands;
mod builtin_targets;
mod changeset_input;
mod cli;
mod cli_helpers;
mod compile_commands;
mod context_commands;
mod diagnostic_commands;
mod error;
mod eval_commands;
mod graph_loading;
mod graph_query_commands;
mod inspect_commands;
mod link_commands;
mod output;
mod package_commands;
mod package_output;
mod package_registry_io;
mod policy_commands;
mod project;
mod project_commands;
mod remote_commands;
mod remote_config;
mod run_commands;
mod store;
mod store_artifacts;
mod workflow_commands;

use error::exit_code;

#[tokio::main]
async fn main() {
    if let Err(err) = cli::run().await {
        eprintln!("ail: {err}");
        std::process::exit(exit_code(&err));
    }
}
