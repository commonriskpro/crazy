mod changeset_input;
mod cli;
mod error;
mod output;
mod project;

use error::exit_code;

fn main() {
    if let Err(err) = cli::run() {
        eprintln!("ail: {err}");
        std::process::exit(exit_code(&err));
    }
}
