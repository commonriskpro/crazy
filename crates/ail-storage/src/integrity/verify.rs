use crate::error::StorageResult;
use crate::graph::GraphStore;
use crate::object::{ObjectId, ObjectStore};

use super::{IntegrityInput, IntegrityIssue, IntegrityReport};

/// Run integrity checks on `graph_store` against `object_store`.
///
/// # Checks performed
///
/// 1. **MissingObject**           — `graph_root_hash` does not exist in object store.
/// 2. **HashMismatch**            — Stored bytes do not hash to declared id.
/// 3. **OrphanedSnapshot**        — `parent_id` points to non-existent snapshot.
/// 4. **ChangeMissingReport**     — ChangeSet has no linked verification report.
/// 5. **ReportMissingArtifact**   — Verification report has no artifact hash.
/// 6. **ApprovalOrphanedChange**  — Approval references unknown ChangeSet.
/// 7. **AssumptionOrphanedBoundary** — Assumption references unknown boundary.
/// 8. **StaleIndex**              — Index entry does not match snapshot root.
///
/// # Returns
///
/// An [`IntegrityReport`] describing all issues found.  Never mutates stores.
///
/// # Errors
///
/// Propagates any `StorageError` from `list_snapshots` or `exists`.
pub async fn verify_integrity<G, O>(
    graph_store: &G,
    object_store: &O,
    input: IntegrityInput,
) -> StorageResult<IntegrityReport>
where
    G: GraphStore + Send + Sync,
    O: ObjectStore + Send + Sync,
{
    let snapshots = graph_store.list_snapshots().await?;
    let snapshots_checked = snapshots.len() as u64;

    // Collect snapshot ids for parent-link and ChangeSet cross-checks.
    let all_snapshot_ids: std::collections::BTreeSet<ObjectId> =
        snapshots.iter().map(|s| s.id).collect();

    // Collect all ChangeSet ids declared by snapshots.
    let all_changeset_ids: std::collections::BTreeSet<ObjectId> = snapshots
        .iter()
        .filter_map(|s| s.applied_change_id)
        .collect();

    // Build change→report and report→artifact lookup sets.
    let change_report_map: std::collections::BTreeMap<ObjectId, ObjectId> =
        input.change_report_index.into_iter().collect();
    let report_artifact_map: std::collections::BTreeMap<ObjectId, ObjectId> =
        input.report_artifact_index.into_iter().collect();

    let known_boundary_set: std::collections::BTreeSet<ObjectId> =
        input.known_boundary_ids.into_iter().collect();

    let stale_set: std::collections::BTreeSet<ObjectId> =
        input.stale_index_ids.into_iter().collect();

    let mut issues = Vec::new();

    // ── Check 1 & 3: snapshot root exists, parent link valid ──────────────
    for snap in &snapshots {
        // Check 1: graph_root_hash must exist as a raw object.
        let root_exists = object_store.exists(&snap.graph_root_hash).await?;
        if !root_exists {
            issues.push(IntegrityIssue::MissingObject {
                id: snap.graph_root_hash,
            });
        }

        // Check 3: parent_id (when Some) must reference a known snapshot.
        if let Some(parent_id) = snap.parent_id
            && !all_snapshot_ids.contains(&parent_id)
        {
            issues.push(IntegrityIssue::OrphanedSnapshot { id: snap.id });
        }
    }

    // ── Check 2: object hashes match content ──────────────────────────────
    for (declared_id, raw) in &input.objects_to_verify {
        let actual_id = ObjectId::from_bytes(&raw.0);
        if actual_id != *declared_id {
            issues.push(IntegrityIssue::HashMismatch { id: *declared_id });
        }
    }

    // ── Check 4: changes link to reports ──────────────────────────────────
    for cs_id in &all_changeset_ids {
        if !change_report_map.contains_key(cs_id) {
            issues.push(IntegrityIssue::ChangeMissingReport { id: *cs_id });
        }
    }

    // ── Check 5: reports link to artifact hashes ──────────────────────────
    for report_id in change_report_map.values() {
        if !report_artifact_map.contains_key(report_id) {
            issues.push(IntegrityIssue::ReportMissingArtifact { id: *report_id });
        }
    }

    // ── Check 6: approvals reference canonical changes ────────────────────
    for approval in &input.approvals {
        if !all_changeset_ids.contains(&approval.subject_change_id) {
            issues.push(IntegrityIssue::ApprovalOrphanedChange { id: approval.id });
        }
    }

    // ── Check 7: assumptions link to boundaries ───────────────────────────
    for assumption in &input.assumptions {
        if !known_boundary_set.contains(&assumption.boundary_id) {
            issues.push(IntegrityIssue::AssumptionOrphanedBoundary { id: assumption.id });
        }
    }

    // ── Check 8: indexes match snapshot root or are marked stale ─────────
    // Get the current canonical snapshot root by taking the latest snapshot.
    let current_root: Option<ObjectId> = snapshots
        .iter()
        .max_by_key(|s| s.created_at)
        .map(|s| s.graph_root_hash);

    for (index_id, index_root) in &input.index_entries {
        // If index is in the stale set, it is exempt from this check.
        if stale_set.contains(index_id) {
            continue;
        }
        // If no snapshot exists, no index can be valid.
        match current_root {
            None => {
                issues.push(IntegrityIssue::StaleIndex { id: *index_id });
            }
            Some(root) => {
                if *index_root != root {
                    issues.push(IntegrityIssue::StaleIndex { id: *index_id });
                }
            }
        }
    }

    // Sort issues for determinism: by kind first, then by id bytes.
    issues.sort_by(|a, b| {
        a.kind_ord()
            .cmp(&b.kind_ord())
            .then(a.id().as_bytes().cmp(b.id().as_bytes()))
    });

    let passed = issues.is_empty();
    Ok(IntegrityReport {
        issues,
        snapshots_checked,
        passed,
    })
}
