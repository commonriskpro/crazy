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
//    `ObjectId`.
// 3. **CorruptObject** — A raw object's stored bytes cannot be decoded as the
//    expected CBOR type during an opt-in typed object scan.
// 4. **DuplicateObjectEntry / DuplicateIndexEntry** — An enumerable object store
//    or supplied integrity input repeated an object or index id.
// 5. **OrphanedSnapshot** — `parent_id` points to a non-existent snapshot.
// 6. **ChangeMissingReport** — A ChangeSet id declared by a snapshot
//    (`applied_change_id`) is not linked to any verification report id in the
//    provided `change_report_index`.
// 7. **ReportMissingArtifact** — A verification report hash is not backed by a
//    corresponding artifact hash in the `report_artifact_index`.
// 8. **ApprovalOrphanedChange** — An approval record references a
//    `subject_change_id` that is not present in the ChangeSet id set.
// 9. **AssumptionOrphanedBoundary** — An assumption record references a
//    `boundary_id` that is not in the set of known boundary ids.
// 10. **StaleIndex** — An index record does not match the current snapshot root
//    and has not been explicitly marked as stale.
//
// # Report semantics
//
// `IntegrityReport.passed` is `true` iff `issues` is empty.  `issues` is
// sorted by kind first, then by the primary `ObjectId` within the same kind.
// `diagnostics` mirrors that order with stable, redacted descriptors for
// production logs and health endpoints.
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
pub use issue::{IntegrityIssue, IntegrityIssueDescriptor};
pub use object::{verify_decodable_object_store_integrity, verify_object_store_integrity};
pub use report::{IntegrityReport, ObjectIntegrityReport};
pub use verify::verify_integrity;

#[cfg(test)]
mod tests;
