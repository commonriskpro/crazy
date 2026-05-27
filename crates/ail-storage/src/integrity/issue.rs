use serde::{Deserialize, Serialize};

use crate::object::ObjectId;

/// A single problem detected by the integrity verifier.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntegrityIssue {
    /// A `graph_root_hash` in a snapshot points to an object that does not
    /// exist in the object store.
    MissingObject {
        /// The `ObjectId` that was expected but not found.
        id: ObjectId,
    },
    /// A stored object's content does not match its declared `ObjectId`.
    ///
    /// The object store is content-addressed: every object's id is the BLAKE3
    /// hash of its bytes.  A hash mismatch indicates corruption.
    HashMismatch {
        /// The `ObjectId` whose content hash is inconsistent.
        id: ObjectId,
    },
    /// A snapshot's `parent_id` points to a snapshot that is not present in
    /// the store (orphaned chain link).
    OrphanedSnapshot {
        /// The `ObjectId` of the orphaned snapshot.
        id: ObjectId,
    },
    /// A ChangeSet declared by a snapshot is not linked to a verification
    /// report in the provided index.
    ChangeMissingReport {
        /// The ChangeSet `ObjectId` that has no linked report.
        id: ObjectId,
    },
    /// A verification report hash is not backed by a corresponding artifact
    /// hash in the provided index.
    ReportMissingArtifact {
        /// The verification report `ObjectId` that has no artifact hash entry.
        id: ObjectId,
    },
    /// An approval record references a `subject_change_id` that is not in the
    /// known ChangeSet id set.
    ApprovalOrphanedChange {
        /// The `ObjectId` of the approval record with the dangling reference.
        id: ObjectId,
    },
    /// An assumption record references a `boundary_id` that is not in the
    /// known boundary id set.
    AssumptionOrphanedBoundary {
        /// The `ObjectId` of the assumption record with the dangling reference.
        id: ObjectId,
    },
    /// An index entry does not match the current snapshot root and has not
    /// been marked as stale.
    StaleIndex {
        /// The `ObjectId` of the stale index entry.
        id: ObjectId,
    },
}

impl IntegrityIssue {
    /// Numeric sort key for ordering issues by kind.
    pub(super) fn kind_ord(&self) -> u8 {
        match self {
            IntegrityIssue::MissingObject { .. } => 0,
            IntegrityIssue::HashMismatch { .. } => 1,
            IntegrityIssue::OrphanedSnapshot { .. } => 2,
            IntegrityIssue::ChangeMissingReport { .. } => 3,
            IntegrityIssue::ReportMissingArtifact { .. } => 4,
            IntegrityIssue::ApprovalOrphanedChange { .. } => 5,
            IntegrityIssue::AssumptionOrphanedBoundary { .. } => 6,
            IntegrityIssue::StaleIndex { .. } => 7,
        }
    }

    /// The primary `ObjectId` associated with this issue.
    pub(super) fn id(&self) -> &ObjectId {
        match self {
            IntegrityIssue::MissingObject { id }
            | IntegrityIssue::HashMismatch { id }
            | IntegrityIssue::OrphanedSnapshot { id }
            | IntegrityIssue::ChangeMissingReport { id }
            | IntegrityIssue::ReportMissingArtifact { id }
            | IntegrityIssue::ApprovalOrphanedChange { id }
            | IntegrityIssue::AssumptionOrphanedBoundary { id }
            | IntegrityIssue::StaleIndex { id } => id,
        }
    }
}
