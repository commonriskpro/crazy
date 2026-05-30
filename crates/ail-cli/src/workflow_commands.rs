// ── ail-cli::workflow_commands ────────────────────────────────────────────
//
// Handlers for the verify/apply workflow: `ail verify` and `ail apply`.
//
// These two commands form the core change-application pipeline:
//   verify  → run the full VerificationPipeline (21 stages) + policy gate,
//             surface diagnostics, proof obligations, degradation events,
//             and repair options
//   apply   → run the pre-apply gate, atomically apply the ChangeSet, emit a snapshot
//
// Both commands share `rebase_required_repair_option`, which is defined here
// because it is used exclusively by this module.

use ail_change::model::ChangeSetOutcome;
use ail_core::semantic_graph::{NodeKind, SemanticGraph};
use ail_storage::{SnapshotEnvelope, object::ObjectId};
use ail_verify::pipeline::{PipelineContext, VerificationPipeline};
use ail_verify::policy::{PolicyDecision, PolicyEngine, PolicyInput, PolicyRule};
use ail_verify::proof::ProofObligation;
use ail_verify::report::VerificationReport;
use ail_verify::solver::{SimpleSolver, Solver, SolverOutcome};
use serde_json::{Value, json};

use crate::cli::{
    SimpleSnapshotBridge, conflict_reason_message, hex_to_object_id, is_valid_change_id,
    latest_snapshot, load_current_graph_with_snapshot_id_for_cli, unix_ms_now,
};
use crate::error::CliError;
use crate::output::{OutputMode, print_error_response, print_response};
use crate::store::StoreHandle;

mod apply;
mod helpers;
mod solver;
mod verify;

pub(crate) use apply::cmd_apply;
pub(crate) use verify::cmd_verify;

use helpers::{is_changeset_meta_stage_claim, rebase_required_repair_option};
use solver::build_solver;

#[cfg(test)]
mod tests;
