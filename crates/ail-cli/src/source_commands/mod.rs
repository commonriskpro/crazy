// ── ail-cli::source_commands ──────────────────────────────────────────────
//
// Minimal AIL source-language frontend.
//
// This is intentionally small, but it is not ACL: users can write `.ail`
// source files and run/test them without authoring ChangeSet ops directly.
// The current frontend lowers that source into the existing semantic graph
// pipeline so the compiler/runtime path stays real end-to-end.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use ail_change::canonical::canonicalize_parsed;
use ail_change::model::{ChangeSetOutcome, SnapshotId};
use ail_change::parser::parse_changeset;
use ail_core::semantic_graph::SemanticGraph;
use serde_json::json;

use crate::cli_helpers::SimpleSnapshotBridge;
use crate::error::CliError;
use crate::output::{OutputMode, print_response};

mod api;
mod descriptors;
mod diagnostics;
mod format;
mod lower;
mod model;
mod parse;
mod syntax;
#[cfg(test)]
mod tests;
mod types;
mod validate;

pub(crate) use api::{
    cmd_check_source, format_ail_source, load_source_graph, load_source_graph_with_entry,
};
pub(crate) use descriptors::{source_export_type_descriptors, source_return_descriptor_for_module};
pub(crate) use diagnostics::{
    SourceIgnoredExpressionStatement, SourceUnusedBinding,
    source_ignored_expression_statement_diagnostics, source_unused_binding_diagnostics,
};
pub(crate) use model::{LoadedSourceGraph, SourceProgram};
pub(crate) use parse::{load_source_program, load_source_program_from_text, parse_ail_source};
