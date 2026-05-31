use serde::{Deserialize, Serialize};

use crate::object::ObjectId;

/// Stable, deterministic, redacted diagnostic for an integrity issue.
///
/// This is intended for production logs and health endpoints: it exposes a
/// stable short fingerprint for correlation without emitting the full CAS hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrityIssueDescriptor {
    /// Stable machine-readable code.
    pub code: String,
    /// Coarse issue category.
    pub category: String,
    /// Severity suitable for health reporting.
    pub severity: String,
    /// Redacted subject kind.
    pub subject: String,
    /// Stable redacted object/index/snapshot fingerprint.
    pub fingerprint: String,
    /// Stable machine-readable reason.
    pub reason: String,
    /// Human-readable message with no full object id.
    pub message: String,
}

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
    /// A stored object could not be decoded as the expected CBOR type.
    CorruptObject {
        /// The `ObjectId` whose bytes could not be decoded.
        id: ObjectId,
    },
    /// The object enumeration or verifier input repeated the same object id.
    DuplicateObjectEntry {
        /// The duplicated `ObjectId`.
        id: ObjectId,
    },
    /// The supplied integrity index repeated the same index id.
    DuplicateIndexEntry {
        /// The duplicated index entry id.
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
            IntegrityIssue::CorruptObject { .. } => 2,
            IntegrityIssue::DuplicateObjectEntry { .. } => 3,
            IntegrityIssue::DuplicateIndexEntry { .. } => 4,
            IntegrityIssue::OrphanedSnapshot { .. } => 5,
            IntegrityIssue::ChangeMissingReport { .. } => 6,
            IntegrityIssue::ReportMissingArtifact { .. } => 7,
            IntegrityIssue::ApprovalOrphanedChange { .. } => 8,
            IntegrityIssue::AssumptionOrphanedBoundary { .. } => 9,
            IntegrityIssue::StaleIndex { .. } => 10,
        }
    }

    /// The primary `ObjectId` associated with this issue.
    pub(super) fn id(&self) -> &ObjectId {
        match self {
            IntegrityIssue::MissingObject { id }
            | IntegrityIssue::HashMismatch { id }
            | IntegrityIssue::CorruptObject { id }
            | IntegrityIssue::DuplicateObjectEntry { id }
            | IntegrityIssue::DuplicateIndexEntry { id }
            | IntegrityIssue::OrphanedSnapshot { id }
            | IntegrityIssue::ChangeMissingReport { id }
            | IntegrityIssue::ReportMissingArtifact { id }
            | IntegrityIssue::ApprovalOrphanedChange { id }
            | IntegrityIssue::AssumptionOrphanedBoundary { id }
            | IntegrityIssue::StaleIndex { id } => id,
        }
    }

    pub(super) fn descriptor(&self) -> IntegrityIssueDescriptor {
        let (code, category, subject, reason, message) = match self {
            IntegrityIssue::MissingObject { .. } => (
                "storage.cas.missing_object",
                "storage.cas.integrity",
                "cas_object",
                "missing_object",
                "CAS object referenced by storage metadata is missing",
            ),
            IntegrityIssue::HashMismatch { .. } => (
                "storage.cas.hash_mismatch",
                "storage.cas.integrity",
                "cas_object",
                "hash_mismatch",
                "CAS object bytes do not match their declared content hash",
            ),
            IntegrityIssue::CorruptObject { .. } => (
                "storage.cas.corrupt_object",
                "storage.cas.integrity",
                "cas_object",
                "decode_failed",
                "CAS object bytes could not be decoded as the expected type",
            ),
            IntegrityIssue::DuplicateObjectEntry { .. } => (
                "storage.cas.duplicate_object_entry",
                "storage.cas.index",
                "cas_object",
                "duplicate_object_entry",
                "CAS object enumeration contains a duplicate object entry",
            ),
            IntegrityIssue::DuplicateIndexEntry { .. } => (
                "storage.index.duplicate_entry",
                "storage.index.integrity",
                "index_entry",
                "duplicate_index_entry",
                "storage integrity input contains a duplicate index entry",
            ),
            IntegrityIssue::OrphanedSnapshot { .. } => (
                "storage.snapshot.orphaned_parent",
                "storage.snapshot.integrity",
                "snapshot",
                "orphaned_parent",
                "snapshot parent link points to a missing snapshot",
            ),
            IntegrityIssue::ChangeMissingReport { .. } => (
                "storage.change.missing_report",
                "storage.change.integrity",
                "change_set",
                "missing_report",
                "change set is missing a linked verification report",
            ),
            IntegrityIssue::ReportMissingArtifact { .. } => (
                "storage.report.missing_artifact",
                "storage.report.integrity",
                "verification_report",
                "missing_artifact",
                "verification report is missing a linked artifact hash",
            ),
            IntegrityIssue::ApprovalOrphanedChange { .. } => (
                "storage.approval.orphaned_change",
                "storage.approval.integrity",
                "approval",
                "orphaned_change",
                "approval references an unknown change set",
            ),
            IntegrityIssue::AssumptionOrphanedBoundary { .. } => (
                "storage.assumption.orphaned_boundary",
                "storage.assumption.integrity",
                "assumption",
                "orphaned_boundary",
                "assumption references an unknown boundary",
            ),
            IntegrityIssue::StaleIndex { .. } => (
                "storage.index.stale_entry",
                "storage.index.integrity",
                "index_entry",
                "stale_index",
                "index entry does not match the current snapshot root",
            ),
        };

        IntegrityIssueDescriptor {
            code: code.to_owned(),
            category: category.to_owned(),
            severity: "error".to_owned(),
            subject: subject.to_owned(),
            fingerprint: redacted_fingerprint(self.id()),
            reason: reason.to_owned(),
            message: message.to_owned(),
        }
    }
}

pub(super) fn sort_issues(issues: &mut [IntegrityIssue]) {
    issues.sort_by(|a, b| {
        a.kind_ord()
            .cmp(&b.kind_ord())
            .then(a.id().as_bytes().cmp(b.id().as_bytes()))
    });
}

pub(super) fn issue_descriptors(issues: &[IntegrityIssue]) -> Vec<IntegrityIssueDescriptor> {
    issues
        .iter()
        .map(IntegrityIssue::descriptor)
        .collect::<Vec<_>>()
}

fn redacted_fingerprint(id: &ObjectId) -> String {
    let hex = id.to_hex();
    format!("blake3:{}…", &hex[..12])
}
