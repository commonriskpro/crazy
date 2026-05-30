// ── ail-cli::lsp_commands ────────────────────────────────────────────────
//
// Minimal Language Server Protocol surface for ACL and `.ail` source documents.
//
// This is validation-stage editor support: enough for an editor/client to
// initialize the server and receive parser/op-schema diagnostics for ACL text
// plus parser diagnostics for the validation-stage `.ail` source surface.

mod commands;
mod definition;
mod diagnostics;
mod protocol;
mod references;
mod source_helpers;
mod symbols;
mod tokens;

pub(crate) use commands::cmd_lsp;
