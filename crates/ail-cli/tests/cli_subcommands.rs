// ── ail-cli integration tests: subcommands (G8 + G31) ─────────────────────
//
// Mechanical split: test bodies live under tests/cli_subcommands/*.rs.
// Keep this file as the integration-test crate root so `--test cli_subcommands` still works.

mod common;

#[path = "cli_subcommands/fmt.rs"]
mod fmt;
#[path = "cli_subcommands/link.rs"]
mod link;
#[path = "cli_subcommands/lsp_diagnostics.rs"]
mod lsp_diagnostics;
#[path = "cli_subcommands/lsp_intelligence.rs"]
mod lsp_intelligence;
#[path = "cli_subcommands/project.rs"]
mod project;
#[path = "cli_subcommands/source_check.rs"]
mod source_check;
#[path = "cli_subcommands/source_compile.rs"]
mod source_compile;
#[path = "cli_subcommands/source_run.rs"]
mod source_run;
#[path = "cli_subcommands/source_test.rs"]
mod source_test;
