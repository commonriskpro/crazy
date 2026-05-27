use crate::approval::{ApprovalRecord, AssumptionRecord};
use crate::object::{ObjectId, RawObject};

/// Cross-domain reference data required for full integrity verification.
///
/// These are injected rather than fetched from the store because they reside
/// in domain-specific stores (`ApprovalStore`, `AssumptionStore`, etc.) that
/// are not part of the base `GraphStore`/`ObjectStore` interface.
#[derive(Default)]
pub struct IntegrityInput {
    /// Mapping from ChangeSet id → verification report id.
    ///
    /// Used for check 4: *changes link to reports*.
    pub change_report_index: Vec<(ObjectId, ObjectId)>,
    /// Mapping from verification report id → artifact hash.
    ///
    /// Used for check 5: *reports link to artifact hashes*.
    pub report_artifact_index: Vec<(ObjectId, ObjectId)>,
    /// All approval records to cross-check.
    ///
    /// Used for check 6: *approvals reference canonical changes*.
    pub approvals: Vec<ApprovalRecord>,
    /// All assumption records to cross-check.
    ///
    /// Used for check 7: *assumptions link to boundaries*.
    pub assumptions: Vec<AssumptionRecord>,
    /// All known boundary ids.
    ///
    /// Used for check 7: assumption `boundary_id` must be in this set.
    pub known_boundary_ids: Vec<ObjectId>,
    /// All raw objects to hash-verify (id, bytes pairs).
    ///
    /// Used for check 2: *object hashes match content*.
    ///
    /// Each entry is an `ObjectId` plus the raw bytes the store returned for
    /// it.  The verifier recomputes the BLAKE3 hash of the bytes and compares
    /// it to the id.
    pub objects_to_verify: Vec<(ObjectId, RawObject)>,
    /// Index entries as (index_id, expected_snapshot_root_hash) pairs.
    ///
    /// Used for check 8: *indexes match snapshot or are marked stale*.
    /// `stale_index_ids` lists any index ids explicitly marked as stale.
    pub index_entries: Vec<(ObjectId, ObjectId)>,
    /// Index ids that have been explicitly marked as stale (and thus are
    /// exempt from the staleness check).
    pub stale_index_ids: Vec<ObjectId>,
}
