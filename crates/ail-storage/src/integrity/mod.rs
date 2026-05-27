// Storage integrity verification.
//
// # Design
//
// `verify_integrity` is a comprehensive read-only check that validates
// referential integrity, content-hash correctness, and cross-domain link
// consistency across the store.
//
// # Checks
//
// 1. **MissingObject**   — `graph_root_hash` in a snapshot does not exist in the
//    object store.
// 2. **HashMismatch**    — A raw object's stored bytes do not hash to its declared
//    `ObjectId`.  This is now actively checked via `check_object_hashes`.
// 3. **OrphanedSnapshot** — `parent_id` points to a non-existent snapshot.
// 4. **ChangeMissingReport** — A ChangeSet id declared by a snapshot
//    (`applied_change_id`) is not linked to any verification report id in the
//    provided `change_report_index`.
// 5. **ReportMissingArtifact** — A verification report hash is not backed by a
//    corresponding artifact hash in the `report_artifact_index`.
// 6. **ApprovalOrphanedChange** — An approval record references a
//    `subject_change_id` that is not present in the ChangeSet id set.
// 7. **AssumptionOrphanedBoundary** — An assumption record references a
//    `boundary_id` that is not in the set of known boundary ids.
// 8. **StaleIndex** — An index record does not match the current snapshot root
//    and has not been explicitly marked as stale.
//
// # Report semantics
//
// `IntegrityReport.passed` is `true` iff `issues` is empty.  `issues` is
// sorted by kind first, then by the primary `ObjectId` within the same kind.
//
// # Determinism
//
// All sorting is deterministic (BTreeSet/BTreeMap or explicit sort by bytes).
// No HashMap is used in the hot path.

mod input;
mod issue;
mod object;
mod report;
mod verify;

pub use input::IntegrityInput;
pub use issue::IntegrityIssue;
pub use object::verify_object_store_integrity;
pub use report::{IntegrityReport, ObjectIntegrityReport};
pub use verify::verify_integrity;

#[cfg(test)]
mod tests;
