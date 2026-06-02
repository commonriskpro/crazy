// ── ail-cli::run_commands ─────────────────────────────────────────────────
//
// Handler for `ail run`.
//
// Rejects `--target native` with an explicit deterministic error because
// native linked execution is not yet supported.  For the WASM path, preflight
// check results are derived from actual `validate_and_instantiate` outcomes
// (audit log, manifest, module hash) rather than being hardcoded strings.
//
// Private helpers:
//   parse_runtime_args          — convert positional string args to RuntimeArg
//   derive_runtime_capability_ids — collect CapabilityIds from a SemanticGraph

use ail_compiler::{
    AbiDescriptor, AnfExpr, AnfIr, ArtifactManifest, WasmTypeDescriptor, emit_wasm_with_profile,
    lower_to_anf_with_graph, lower_to_core_ir,
};
use std::collections::BTreeMap;
use std::sync::Arc;

use ail_runtime::{
    AuditEvent, CapabilityGrant, CapabilityId, CapabilityManifest, ClockHandler, FileReadHandler,
    LogHandler, PreflightFailure, ResourceLimits, RuntimeArg, RuntimeError, RuntimeHost,
    RuntimeProfile, RuntimeReportStatus, RuntimeValue, SeededRandom, StructuredValue, ValueLayout,
    blake3_hex_of,
};
use serde_json::{Value, json};

use ail_core::semantic_graph::{NodeKind, SemanticGraph};

use crate::builtin_targets::runtime_anf_for_target;
use crate::cli::load_current_graph_for_cli;
use crate::compile_commands::accepted_compile_report;
use crate::error::CliError;
use crate::output::{OutputMode, print_response};
use crate::source_commands::load_source_graph_with_entry;
use crate::store::StoreHandle;
use std::path::Path;

mod args;
mod capabilities;
mod command;
mod errors;
mod invoke;

pub(crate) use args::parse_runtime_args;
pub(crate) use capabilities::derive_runtime_capability_ids;
pub(crate) use command::cmd_run;
pub(crate) use invoke::invoke_export_for_cli;

use errors::format_run_preflight_error;

struct RunWasmArtifact {
    wasm: Vec<u8>,
    export_types: BTreeMap<String, WasmTypeDescriptor>,
    source: &'static str,
}

impl RunWasmArtifact {
    fn emitted(artifact: ail_compiler::WasmArtifact, source: &'static str) -> Self {
        Self {
            wasm: artifact.wasm,
            export_types: artifact.export_types,
            source,
        }
    }
}

#[cfg(test)]
mod tests;
