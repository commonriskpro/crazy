// ── ail-dogfood::changeset_self ───────────────────────────────────────────
//
// Builds a self-referential `ChangeSet` that describes the operation of
// adding the `ChangeSet` type itself to a graph.
//
// This module is part of PR 2 (changeset + stdlib + context integration).
// The public API surface is declared here; full implementation and tests
// ship with PR 2.

use ail_change::model::{ChangeSet, ChangeSetMeta, ChangeSetOp, SnapshotId, Timestamp};

/// Build a self-describing `ChangeSet`.
///
/// # Postconditions
///
/// - `result.ops` is non-empty and contains `ChangeSetOp::Create`
/// - `result.meta.description` contains the string `"ChangeSet"`
/// - `result.base_snapshot_id` is a defined `SnapshotId`
pub fn build_changeset_self() -> ChangeSet {
    ChangeSet {
        meta: ChangeSetMeta {
            author: "ail-dogfood".to_string(),
            description: "Self-describing ChangeSet: models the ChangeSet type itself".to_string(),
            timestamp: Timestamp(0),
        },
        base_snapshot_id: SnapshotId(0),
        ops: vec![ChangeSetOp::Create],
    }
}
