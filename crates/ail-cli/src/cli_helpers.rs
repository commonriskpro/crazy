// ── ail-cli::cli_helpers ─────────────────────────────────────────────────
//
// Pure utility helpers extracted from cli.rs to reduce hot-file conflicts.
// All items here are stateless utilities: no store access, no command dispatch.
//
// Re-exported by `crate::cli` for backward compatibility — existing
// `use crate::cli::xxx` imports in other modules continue to compile.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) use ail_change::apply::SnapshotBridge;
use ail_change::model::{
    ChangeSet, ChangeSetMeta, ChangeSetOp, ChangeSetOutcome, ConflictReason, SnapshotId, Timestamp,
};
use ail_core::semantic_graph::{GraphEdge, GraphNode, NodeRef, SemanticGraph};
use ail_storage::{SnapshotEnvelope, object::ObjectId};
use serde_json::{Value, json};

use crate::error::CliError;
use crate::store::StoreHandle;

// ── Hex / id helpers ─────────────────────────────────────────────────────

/// Encode a byte slice as a lowercase hex string.
pub(crate) fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Return `true` if `id` is a valid 64-character lowercase hex string.
pub(crate) fn is_valid_change_id(id: &str) -> bool {
    id.len() == 64 && id.chars().all(|c| c.is_ascii_hexdigit())
}

/// Convert a 64-char hex string into an `ObjectId`.
pub(crate) fn hex_to_object_id(hex: &str) -> Result<ObjectId, CliError> {
    if hex.len() != 64 {
        return Err(CliError::Domain(format!(
            "invalid id length: {}",
            hex.len()
        )));
    }
    let mut bytes = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let s =
            std::str::from_utf8(chunk).map_err(|_| CliError::Domain("non-UTF8 hex".to_string()))?;
        bytes[i] = u8::from_str_radix(s, 16)
            .map_err(|_| CliError::Domain(format!("invalid hex byte: {s}")))?;
    }
    Ok(ObjectId::from(bytes))
}

// ── Time helpers ─────────────────────────────────────────────────────────

/// Return the current time as Unix milliseconds.
pub(crate) fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Format a Unix-millisecond timestamp for human display.
pub(crate) fn format_unix_ms(ms: u64) -> String {
    if ms == 0 {
        "(unknown)".to_string()
    } else {
        format!("{ms} ms since Unix epoch")
    }
}

// ── Store helpers ─────────────────────────────────────────────────────────

/// Return the `.ail/` directory path for a file-backed store.
/// Returns an error for in-memory or Postgres stores.
pub(crate) fn ail_dir_for_store(store: &StoreHandle) -> Result<PathBuf, CliError> {
    match store {
        StoreHandle::File { ail_dir, .. } => Ok(ail_dir.clone()),
        _ => Err(CliError::Domain(
            "persistent .ail storage is not active".to_string(),
        )),
    }
}

// ── Snapshot helpers ─────────────────────────────────────────────────────

/// Return the snapshot with the latest `created_at` timestamp.
pub(crate) fn latest_snapshot(snapshots: &[SnapshotEnvelope]) -> Option<&SnapshotEnvelope> {
    snapshots.iter().max_by_key(|snapshot| snapshot.created_at)
}

// ── ChangeSet helpers ─────────────────────────────────────────────────────

/// Map a `ConflictReason` to a human-readable string.
pub(crate) fn conflict_reason_message(reason: &ConflictReason) -> &'static str {
    match reason {
        ConflictReason::SameNodeModifiedIncompatibly => "same node was modified incompatibly",
        ConflictReason::NodeDeletedWhileModified => {
            "node was deleted while another change modified it"
        }
        ConflictReason::PublicApiConflict => "public API changes conflict",
        ConflictReason::InvariantTouchedConcurrently => "invariant changes conflict",
        ConflictReason::IncompatibleNodeModification => {
            "semantic node content conflict (return type, body, or effects differ)"
        }
    }
}

/// Map a `ChangeSetOutcome` to a human-readable string.
pub(crate) fn changeset_outcome_message(outcome: &ChangeSetOutcome) -> &'static str {
    match outcome {
        ChangeSetOutcome::Applied => "applied",
        ChangeSetOutcome::RebaseRequired { .. } => "rebase required",
        ChangeSetOutcome::Failed { .. } => "change failed",
        ChangeSetOutcome::ConflictIrresolvable { reason } => conflict_reason_message(reason),
    }
}

/// Create a minimal ChangeSet from a free-text description string.
pub(crate) fn make_text_changeset(text: &str) -> ChangeSet {
    ChangeSet {
        meta: ChangeSetMeta {
            author: "cli".to_string(),
            description: text.to_string(),
            timestamp: Timestamp(unix_ms_now()),
        },
        base_snapshot_id: SnapshotId(0),
        ops: vec![],
    }
}

/// Determine the input source label for human output.
pub(crate) fn input_source_label(from_stdin: bool) -> &'static str {
    if from_stdin { "stdin" } else { "file" }
}

/// Build a structural diff preview from a slice of change ops.
/// At this stage the graph is empty so all ops are treated as additions.
pub(crate) fn build_structural_diff_preview(ops: &[ChangeSetOp]) -> Value {
    json!({
        "creates": ops.len(),
        "modifies": 0,
        "deletes": 0,
        "connects": 0,
        "disconnects": 0,
        "exposes": 0,
        "hides": 0,
        "effects_changed": 0,
        "contracts_changed": 0,
        "capabilities_changed": 0,
    })
}

/// Encode a value as CBOR bytes.
pub(crate) fn encode_cbor<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, CliError> {
    let mut buf = Vec::new();
    ciborium::into_writer(value, &mut buf)
        .map_err(|e| CliError::Domain(format!("CBOR encoding failed: {e}")))?;
    Ok(buf)
}

// ── SnapshotBridge ────────────────────────────────────────────────────────

/// A minimal `SnapshotBridge` that always returns a fixed id.
pub(crate) struct SimpleSnapshotBridge(pub(crate) SnapshotId);

impl SnapshotBridge for SimpleSnapshotBridge {
    fn current_snapshot_id(&self) -> SnapshotId {
        self.0
    }
}

// ── Graph JSON helpers ────────────────────────────────────────────────────

/// Serialize a `GraphNode` to JSON, falling back to a minimal object on error.
pub(crate) fn node_to_json(node: &GraphNode) -> Value {
    serde_json::to_value(node).unwrap_or_else(|_| json!({ "name": node.name }))
}

/// Serialize a `GraphEdge` to JSON, falling back to a minimal object on error.
pub(crate) fn edge_to_json(edge: &GraphEdge) -> Value {
    serde_json::to_value(edge).unwrap_or_else(|_| {
        json!({
            "source": edge.source.0,
            "target": edge.target.0,
            "kind": format!("{:?}", edge.kind),
        })
    })
}

// ── Graph lookup helpers ─────────────────────────────────────────────────

/// Resolve a target string (e.g. "fn.cart_total", "type.CartItem") to the node
/// name to search for. The convention is `<kind>.<name>` — we match by the
/// suffix after the last `.`, or the whole string when no `.` is present.
pub(crate) fn target_node_name(target: &str) -> &str {
    target.rsplit('.').next().unwrap_or(target)
}

/// Look up the `NodeRef`s of every node whose name matches `name`.
pub(crate) fn node_refs_for_name(graph: &SemanticGraph, name: &str) -> Vec<NodeRef> {
    graph
        .nodes
        .iter()
        .filter(|n| n.name == name)
        .map(|n| n.id)
        .collect()
}
