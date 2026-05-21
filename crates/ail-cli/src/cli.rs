use std::path::PathBuf;

use clap::error::ErrorKind;
use clap::{Parser, Subcommand};

/// ail — AI-native language toolchain.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Parse an AIL source file (stub — not yet implemented).
    Parse {
        /// Path to the AIL source file to parse.
        input: PathBuf,
    },
}

/// Entry point called from `main`. Parses CLI arguments and dispatches
/// to the appropriate command handler.
pub fn run() {
    let cli = Cli::try_parse().unwrap_or_else(|err| {
        let kind = err.kind();
        let code = err.exit_code();
        let _ = err.print();
        if kind == ErrorKind::InvalidSubcommand {
            eprintln!("Available subcommands: parse");
        }
        std::process::exit(code);
    });

    match cli.command {
        Commands::Parse { input: _ } => {
            println!("not yet implemented");
        }
    }
}
