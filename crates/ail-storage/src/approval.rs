// Approval and assumption records.
//
// # Design
//
// `ApprovalRecord` is an immutable audit record that captures who approved a
// change, which change was approved, and what scope the approval covers.
// `AssumptionRecord` tracks boundary assumptions that verification gates depend
// on; expired/revoked assumptions propagate to gate failures.
//
// Both record types are first-class stored objects with their own in-memory
// registries AND object-backed durable registries, consistent with the pattern
// used for branches and tags.
//
// # Approval expiry
//
// An approval is considered EXPIRED if the `canonical_change_hash` stored at
// approval time no longer matches the current canonical hash.  This crate
// provides `approval_is_valid` to check this.  Enforcing the gate is the
// caller's responsibility, but the function is provided here so callers do not
// have to re-implement the rule.
//
// # Assumption boundary validation
//
// `AssumptionRecord` carries a `boundary_id`.  `validate_assumption_boundary`
// checks that the assumption references a known boundary `ObjectId` from a
// provided set.
//
// # Verification gate
//
// `VerificationGateResult` is the outcome of evaluating all assumptions
// relevant to a verification run.  Expired or revoked assumptions cause the
// gate to fail.
//
// # Durable storage
//
// `ObjectBackedApprovalStore` and `ObjectBackedAssumptionStore` persist records
// as CBOR-encoded objects in any `ObjectStore`, making them content-addressed
// and durable across process restarts.
//
// # Determinism
//
// All serializable types follow the project's determinism contract: no
// HashMap fields, no floats, timestamps as u64 Unix milliseconds.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::codec::{CborCodec, ContentCodec};
use crate::error::StorageResult;
use crate::object::{ObjectId, ObjectStore, RawObject};

// ── ApprovalRecord ────────────────────────────────────────────────────────

/// An immutable record of an approval decision for a change.
///
/// Per the spec: approval expires if `canonical_change_hash` changes after
/// the record is created.  This crate stores and retrieves records; expiry
/// enforcement is the responsibility of the caller.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRecord {
    /// Opaque identity of this approval record.
    pub id: ObjectId,
    /// The change-set this approval covers.
    pub subject_change_id: ObjectId,
    /// Content hash of the canonical form of the change at approval time.
    /// The approval is invalidated if this hash no longer matches.
    pub canonical_change_hash: ObjectId,
    /// Role of the approver (e.g. `"role:maintainer"`).
    pub approver_role: String,
    /// Scope of the approval (e.g. `"public_api_changed"`).
    pub approves_scope: String,
    /// Unix timestamp in milliseconds when this approval was recorded.
    pub timestamp: u64,
}

// ── ApprovalStore trait ───────────────────────────────────────────────────

/// Async storage contract for approval records.
pub trait ApprovalStore {
    /// Persist `record`.  If a record with the same `id` already exists,
    /// this is a no-op (idempotent).
    fn store_approval(
        &self,
        record: ApprovalRecord,
    ) -> impl Future<Output = StorageResult<()>> + Send;

    /// Retrieve the record with `id`, or `None` if absent.
    fn get_approval(
        &self,
        id: &ObjectId,
    ) -> impl Future<Output = StorageResult<Option<ApprovalRecord>>> + Send;

    /// List all approval records sorted by `timestamp` (ascending).
    fn list_approvals(&self) -> impl Future<Output = StorageResult<Vec<ApprovalRecord>>> + Send;
}

// ── ApprovalRegistry ──────────────────────────────────────────────────────

/// In-memory implementation of [`ApprovalStore`].
#[derive(Clone, Default)]
pub struct ApprovalRegistry {
    inner: Arc<Mutex<HashMap<ObjectId, ApprovalRecord>>>,
}

impl ApprovalRegistry {
    /// Create an empty `ApprovalRegistry`.
    pub fn new() -> Self {
        Self::default()
    }
}

impl ApprovalStore for ApprovalRegistry {
    async fn store_approval(&self, record: ApprovalRecord) -> StorageResult<()> {
        let mut guard = self
            .inner
            .lock()
            .expect("approval_registry lock must not be poisoned");
        guard.entry(record.id).or_insert(record);
        Ok(())
    }

    async fn get_approval(&self, id: &ObjectId) -> StorageResult<Option<ApprovalRecord>> {
        let guard = self
            .inner
            .lock()
            .expect("approval_registry lock must not be poisoned");
        Ok(guard.get(id).cloned())
    }

    async fn list_approvals(&self) -> StorageResult<Vec<ApprovalRecord>> {
        let guard = self
            .inner
            .lock()
            .expect("approval_registry lock must not be poisoned");
        let mut records: Vec<ApprovalRecord> = guard.values().cloned().collect();
        records.sort_by(|a, b| {
            a.timestamp
                .cmp(&b.timestamp)
                .then(a.id.as_bytes().cmp(b.id.as_bytes()))
        });
        Ok(records)
    }
}

// ── AssumptionStatus ──────────────────────────────────────────────────────

/// Lifecycle state of an assumption.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssumptionStatus {
    /// The assumption is currently valid.
    Active,
    /// The assumption has passed its expiry date.
    Expired,
    /// The assumption was explicitly revoked before its expiry.
    Revoked,
}

// ── AssumptionRecord ──────────────────────────────────────────────────────

/// A tracked boundary assumption that verification gates depend on.
///
/// Expired or revoked assumptions should cause downstream verification gates
/// to fail.  This crate stores and retrieves records; gate enforcement is the
/// responsibility of the caller.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AssumptionRecord {
    /// Opaque identity of this assumption record.
    pub id: ObjectId,
    /// The boundary this assumption is attached to.
    pub boundary_id: ObjectId,
    /// Current lifecycle status.
    pub status: AssumptionStatus,
    /// Unix timestamp in milliseconds when this assumption expires, if bounded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    /// Team or role that owns this assumption (e.g. `"team.payments"`).
    pub owner: String,
}

// ── AssumptionStore trait ─────────────────────────────────────────────────

/// Async storage contract for assumption records.
pub trait AssumptionStore {
    /// Persist `record`.  Idempotent on duplicate `id`.
    fn store_assumption(
        &self,
        record: AssumptionRecord,
    ) -> impl Future<Output = StorageResult<()>> + Send;

    /// Retrieve the record with `id`, or `None` if absent.
    fn get_assumption(
        &self,
        id: &ObjectId,
    ) -> impl Future<Output = StorageResult<Option<AssumptionRecord>>> + Send;

    /// List all assumption records.
    fn list_assumptions(&self)
    -> impl Future<Output = StorageResult<Vec<AssumptionRecord>>> + Send;
}

// ── AssumptionRegistry ────────────────────────────────────────────────────

/// In-memory implementation of [`AssumptionStore`].
#[derive(Clone, Default)]
pub struct AssumptionRegistry {
    inner: Arc<Mutex<HashMap<ObjectId, AssumptionRecord>>>,
}

impl AssumptionRegistry {
    /// Create an empty `AssumptionRegistry`.
    pub fn new() -> Self {
        Self::default()
    }
}

impl AssumptionStore for AssumptionRegistry {
    async fn store_assumption(&self, record: AssumptionRecord) -> StorageResult<()> {
        let mut guard = self
            .inner
            .lock()
            .expect("assumption_registry lock must not be poisoned");
        guard.entry(record.id).or_insert(record);
        Ok(())
    }

    async fn get_assumption(&self, id: &ObjectId) -> StorageResult<Option<AssumptionRecord>> {
        let guard = self
            .inner
            .lock()
            .expect("assumption_registry lock must not be poisoned");
        Ok(guard.get(id).cloned())
    }

    async fn list_assumptions(&self) -> StorageResult<Vec<AssumptionRecord>> {
        let guard = self
            .inner
            .lock()
            .expect("assumption_registry lock must not be poisoned");
        let mut records: Vec<AssumptionRecord> = guard.values().cloned().collect();
        // Sort by id bytes for full determinism (no timestamp on assumptions).
        records.sort_by(|a, b| a.id.as_bytes().cmp(b.id.as_bytes()));
        Ok(records)
    }
}

// ── approval_is_valid ─────────────────────────────────────────────────────

/// Check whether an approval is still valid against a current canonical hash.
///
/// Returns `true` iff `current_canonical_hash == record.canonical_change_hash`.
///
/// Per the spec: *"Approval expires if `canonical_change_hash` changes."*
/// Callers must supply the current canonical hash; this crate does not fetch
/// it from any store.
pub fn approval_is_valid(record: &ApprovalRecord, current_canonical_hash: &ObjectId) -> bool {
    record.canonical_change_hash == *current_canonical_hash
}

// ── validate_assumption_boundary ─────────────────────────────────────────

/// Validate that an assumption's `boundary_id` is in the set of known boundaries.
///
/// Returns `Ok(())` if the assumption's `boundary_id` is a member of
/// `known_boundary_ids`.  Returns `Err(boundary_id)` (the invalid id) if the
/// assumption references an unknown boundary.
pub fn validate_assumption_boundary(
    record: &AssumptionRecord,
    known_boundary_ids: &[ObjectId],
) -> Result<(), ObjectId> {
    if known_boundary_ids.contains(&record.boundary_id) {
        Ok(())
    } else {
        Err(record.boundary_id)
    }
}

// ── VerificationGateResult ────────────────────────────────────────────────

/// The outcome of evaluating verification gate conditions.
///
/// The gate passes only when no expired or revoked assumptions are found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VerificationGateResult {
    /// All assumptions are active; the gate passes.
    Pass,
    /// At least one assumption is expired or revoked; the gate fails.
    ///
    /// Contains the ids of all offending assumptions, sorted by id bytes for
    /// determinism.
    Fail {
        /// Ids of the assumptions that caused the gate to fail, sorted.
        failed_assumption_ids: Vec<ObjectId>,
    },
}

/// Evaluate the verification gate against a slice of assumptions.
///
/// Any assumption with [`AssumptionStatus::Expired`] or
/// [`AssumptionStatus::Revoked`] causes the gate to fail.  If `now_ms` is
/// provided, assumptions whose `expires_at` has passed are also treated as
/// expired even if their stored status is `Active`.
///
/// Returns [`VerificationGateResult::Pass`] if all assumptions are active and
/// unexpired, or [`VerificationGateResult::Fail`] listing all offenders.
pub fn evaluate_verification_gate(
    assumptions: &[AssumptionRecord],
    now_ms: Option<u64>,
) -> VerificationGateResult {
    let mut failed: Vec<ObjectId> = assumptions
        .iter()
        .filter(|a| {
            // Explicit revoke/expire status.
            if matches!(
                a.status,
                AssumptionStatus::Expired | AssumptionStatus::Revoked
            ) {
                return true;
            }
            // Time-based expiry.
            if let (Some(expires_at), Some(now)) = (a.expires_at, now_ms) {
                if now >= expires_at {
                    return true;
                }
            }
            false
        })
        .map(|a| a.id)
        .collect();

    if failed.is_empty() {
        VerificationGateResult::Pass
    } else {
        failed.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
        VerificationGateResult::Fail {
            failed_assumption_ids: failed,
        }
    }
}

// ── ObjectBackedApprovalStore ─────────────────────────────────────────────

/// A durable, object-backed [`ApprovalStore`].
///
/// Records are CBOR-encoded and stored as content-addressed objects in the
/// wrapped [`ObjectStore`].  An in-memory index maps record id → CAS id so
/// that lookups remain O(n) in the index size, not the object store size.
///
/// This is the durable counterpart to [`ApprovalRegistry`], which is in-memory
/// only.
#[derive(Clone)]
pub struct ObjectBackedApprovalStore<S> {
    store: Arc<S>,
    codec: CborCodec,
    index: Arc<Mutex<HashMap<ObjectId, ObjectId>>>,
}

impl<S: ObjectStore + Send + Sync> ObjectBackedApprovalStore<S> {
    /// Wrap `store` in an `ObjectBackedApprovalStore`.
    pub fn new(store: S) -> Self {
        Self {
            store: Arc::new(store),
            codec: CborCodec,
            index: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl<S: ObjectStore + Send + Sync> ApprovalStore for ObjectBackedApprovalStore<S> {
    async fn store_approval(&self, record: ApprovalRecord) -> StorageResult<()> {
        // Encode → store → index.
        let bytes = self.codec.encode(&record)?;
        let cas_id = self.store.put(RawObject(bytes)).await?;
        let mut guard = self.index.lock().expect("approval_index lock not poisoned");
        guard.entry(record.id).or_insert(cas_id);
        Ok(())
    }

    async fn get_approval(&self, id: &ObjectId) -> StorageResult<Option<ApprovalRecord>> {
        let cas_id = {
            let guard = self.index.lock().expect("approval_index lock not poisoned");
            guard.get(id).copied()
        };
        match cas_id {
            None => Ok(None),
            Some(cas_id) => match self.store.get(&cas_id).await? {
                None => Ok(None),
                Some(raw) => {
                    let record: ApprovalRecord = self.codec.decode(&raw.0)?;
                    Ok(Some(record))
                }
            },
        }
    }

    async fn list_approvals(&self) -> StorageResult<Vec<ApprovalRecord>> {
        let pairs: Vec<(ObjectId, ObjectId)> = {
            let guard = self.index.lock().expect("approval_index lock not poisoned");
            guard.iter().map(|(k, v)| (*k, *v)).collect()
        };
        let mut records = Vec::with_capacity(pairs.len());
        for (_id, cas_id) in pairs {
            if let Some(raw) = self.store.get(&cas_id).await? {
                let record: ApprovalRecord = self.codec.decode(&raw.0)?;
                records.push(record);
            }
        }
        records.sort_by(|a, b| {
            a.timestamp
                .cmp(&b.timestamp)
                .then(a.id.as_bytes().cmp(b.id.as_bytes()))
        });
        Ok(records)
    }
}

// ── ObjectBackedAssumptionStore ───────────────────────────────────────────

/// A durable, object-backed [`AssumptionStore`].
///
/// Records are CBOR-encoded and stored as content-addressed objects in the
/// wrapped [`ObjectStore`].
#[derive(Clone)]
pub struct ObjectBackedAssumptionStore<S> {
    store: Arc<S>,
    codec: CborCodec,
    index: Arc<Mutex<HashMap<ObjectId, ObjectId>>>,
}

impl<S: ObjectStore + Send + Sync> ObjectBackedAssumptionStore<S> {
    /// Wrap `store` in an `ObjectBackedAssumptionStore`.
    pub fn new(store: S) -> Self {
        Self {
            store: Arc::new(store),
            codec: CborCodec,
            index: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl<S: ObjectStore + Send + Sync> AssumptionStore for ObjectBackedAssumptionStore<S> {
    async fn store_assumption(&self, record: AssumptionRecord) -> StorageResult<()> {
        let bytes = self.codec.encode(&record)?;
        let cas_id = self.store.put(RawObject(bytes)).await?;
        let mut guard = self
            .index
            .lock()
            .expect("assumption_index lock not poisoned");
        guard.entry(record.id).or_insert(cas_id);
        Ok(())
    }

    async fn get_assumption(&self, id: &ObjectId) -> StorageResult<Option<AssumptionRecord>> {
        let cas_id = {
            let guard = self
                .index
                .lock()
                .expect("assumption_index lock not poisoned");
            guard.get(id).copied()
        };
        match cas_id {
            None => Ok(None),
            Some(cas_id) => match self.store.get(&cas_id).await? {
                None => Ok(None),
                Some(raw) => {
                    let record: AssumptionRecord = self.codec.decode(&raw.0)?;
                    Ok(Some(record))
                }
            },
        }
    }

    async fn list_assumptions(&self) -> StorageResult<Vec<AssumptionRecord>> {
        let pairs: Vec<(ObjectId, ObjectId)> = {
            let guard = self
                .index
                .lock()
                .expect("assumption_index lock not poisoned");
            guard.iter().map(|(k, v)| (*k, *v)).collect()
        };
        let mut records = Vec::with_capacity(pairs.len());
        for (_id, cas_id) in pairs {
            if let Some(raw) = self.store.get(&cas_id).await? {
                let record: AssumptionRecord = self.codec.decode(&raw.0)?;
                records.push(record);
            }
        }
        records.sort_by(|a, b| a.id.as_bytes().cmp(b.id.as_bytes()));
        Ok(records)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_id(seed: u8) -> ObjectId {
        ObjectId::from_bytes(&[seed; 32])
    }

    fn make_approval(seed: u8, timestamp: u64) -> ApprovalRecord {
        ApprovalRecord {
            id: make_id(seed),
            subject_change_id: make_id(seed + 10),
            canonical_change_hash: make_id(seed + 20),
            approver_role: "role:maintainer".to_owned(),
            approves_scope: "public_api_changed".to_owned(),
            timestamp,
        }
    }

    // Scenario: store_approval then get_approval returns record.
    #[tokio::test]
    async fn store_and_get_approval() {
        let reg = ApprovalRegistry::new();
        let record = make_approval(1, 1000);
        reg.store_approval(record.clone())
            .await
            .expect("store must succeed");
        let got = reg
            .get_approval(&record.id)
            .await
            .expect("get must succeed")
            .expect("record must exist");
        assert_eq!(got, record);
    }

    // Scenario: store_approval is idempotent on duplicate id.
    #[tokio::test]
    async fn store_approval_idempotent() {
        let reg = ApprovalRegistry::new();
        let record = make_approval(1, 1000);
        reg.store_approval(record.clone())
            .await
            .expect("first store");
        reg.store_approval(record.clone())
            .await
            .expect("duplicate store must succeed");
        let list = reg.list_approvals().await.expect("list");
        assert_eq!(list.len(), 1, "duplicate must not create two entries");
    }

    // Scenario: get_approval on missing id returns None.
    #[tokio::test]
    async fn get_approval_missing_returns_none() {
        let reg = ApprovalRegistry::new();
        let result = reg.get_approval(&make_id(99)).await.expect("get");
        assert!(result.is_none());
    }

    // Scenario: list_approvals returns all records sorted by timestamp.
    #[tokio::test]
    async fn list_approvals_returns_all_sorted() {
        let reg = ApprovalRegistry::new();
        let r1 = make_approval(1, 200);
        let r2 = make_approval(2, 100);
        reg.store_approval(r1.clone()).await.expect("store r1");
        reg.store_approval(r2.clone()).await.expect("store r2");
        let list = reg.list_approvals().await.expect("list");
        assert_eq!(list.len(), 2);
        // r2 has earlier timestamp, must come first.
        assert_eq!(list[0].timestamp, 100);
        assert_eq!(list[1].timestamp, 200);
    }

    fn make_assumption(seed: u8, status: AssumptionStatus) -> AssumptionRecord {
        AssumptionRecord {
            id: make_id(seed),
            boundary_id: make_id(seed + 50),
            status,
            expires_at: None,
            owner: "team.payments".to_owned(),
        }
    }

    // Scenario: store_assumption then get_assumption returns record.
    #[tokio::test]
    async fn store_and_get_assumption() {
        let reg = AssumptionRegistry::new();
        let record = make_assumption(1, AssumptionStatus::Active);
        reg.store_assumption(record.clone())
            .await
            .expect("store must succeed");
        let got = reg
            .get_assumption(&record.id)
            .await
            .expect("get must succeed")
            .expect("record must exist");
        assert_eq!(got, record);
        assert_eq!(got.status, AssumptionStatus::Active);
    }

    // Scenario: store_assumption idempotent.
    #[tokio::test]
    async fn store_assumption_idempotent() {
        let reg = AssumptionRegistry::new();
        let record = make_assumption(2, AssumptionStatus::Active);
        reg.store_assumption(record.clone()).await.expect("first");
        reg.store_assumption(record.clone())
            .await
            .expect("duplicate");
        let list = reg.list_assumptions().await.expect("list");
        assert_eq!(list.len(), 1);
    }

    // Scenario: AssumptionStatus variants are stored correctly.
    #[tokio::test]
    async fn assumption_status_variants() {
        let reg = AssumptionRegistry::new();
        let a = make_assumption(10, AssumptionStatus::Active);
        let e = make_assumption(11, AssumptionStatus::Expired);
        let r = make_assumption(12, AssumptionStatus::Revoked);
        reg.store_assumption(a).await.expect("store a");
        reg.store_assumption(e).await.expect("store e");
        reg.store_assumption(r).await.expect("store r");
        let list = reg.list_assumptions().await.expect("list");
        assert_eq!(list.len(), 3);
        let statuses: Vec<AssumptionStatus> = list.iter().map(|r| r.status).collect();
        assert!(statuses.contains(&AssumptionStatus::Active));
        assert!(statuses.contains(&AssumptionStatus::Expired));
        assert!(statuses.contains(&AssumptionStatus::Revoked));
    }

    // Scenario: assumption with expires_at stores and retrieves correctly.
    #[tokio::test]
    async fn assumption_with_expires_at() {
        let reg = AssumptionRegistry::new();
        let record = AssumptionRecord {
            id: make_id(20),
            boundary_id: make_id(21),
            status: AssumptionStatus::Active,
            expires_at: Some(9_999_999_000),
            owner: "team.infra".to_owned(),
        };
        reg.store_assumption(record.clone()).await.expect("store");
        let got = reg
            .get_assumption(&record.id)
            .await
            .expect("get")
            .expect("must exist");
        assert_eq!(got.expires_at, Some(9_999_999_000));
    }

    // Scenario: list_assumptions returns all records.
    #[tokio::test]
    async fn list_assumptions_returns_all() {
        let reg = AssumptionRegistry::new();
        reg.store_assumption(make_assumption(1, AssumptionStatus::Active))
            .await
            .expect("a1");
        reg.store_assumption(make_assumption(2, AssumptionStatus::Revoked))
            .await
            .expect("a2");
        let list = reg.list_assumptions().await.expect("list");
        assert_eq!(list.len(), 2);
    }

    // ── approval_is_valid ─────────────────────────────────────────────────

    // Scenario: approval_is_valid returns true when hashes match.
    //   GIVEN approval with canonical_change_hash = X
    //   WHEN approval_is_valid(record, X)
    //   THEN true
    #[tokio::test]
    async fn approval_valid_when_hashes_match() {
        let record = make_approval(1, 1000);
        let current = record.canonical_change_hash;
        assert!(super::approval_is_valid(&record, &current));
    }

    // Scenario: approval_is_valid returns false when hashes differ.
    //   GIVEN approval with canonical_change_hash = X
    //   WHEN approval_is_valid(record, Y) where Y != X
    //   THEN false
    #[tokio::test]
    async fn approval_invalid_when_hash_changed() {
        let record = make_approval(1, 1000);
        let different_hash = make_id(99); // not the same as canonical_change_hash
        assert!(!super::approval_is_valid(&record, &different_hash));
    }

    // ── validate_assumption_boundary ──────────────────────────────────────

    // Scenario: validate_assumption_boundary passes when boundary_id is in set.
    #[tokio::test]
    async fn boundary_validation_passes_for_known_boundary() {
        let record = make_assumption(1, AssumptionStatus::Active);
        let known = vec![record.boundary_id];
        assert!(super::validate_assumption_boundary(&record, &known).is_ok());
    }

    // Scenario: validate_assumption_boundary fails when boundary_id is unknown.
    //   GIVEN assumption with boundary_id = B
    //   WHEN validate_assumption_boundary with empty known set
    //   THEN Err(B)
    #[tokio::test]
    async fn boundary_validation_fails_for_unknown_boundary() {
        let record = make_assumption(1, AssumptionStatus::Active);
        let result = super::validate_assumption_boundary(&record, &[]);
        assert_eq!(result, Err(record.boundary_id));
    }

    // ── evaluate_verification_gate ────────────────────────────────────────

    // Scenario: gate passes when all assumptions are active.
    #[tokio::test]
    async fn gate_passes_all_active() {
        let a1 = make_assumption(1, AssumptionStatus::Active);
        let a2 = make_assumption(2, AssumptionStatus::Active);
        let result = super::evaluate_verification_gate(&[a1, a2], None);
        assert_eq!(result, super::VerificationGateResult::Pass);
    }

    // Scenario: gate fails when any assumption is expired.
    //   GIVEN one Active and one Expired assumption
    //   WHEN evaluate_verification_gate
    //   THEN Fail containing id of expired assumption
    #[tokio::test]
    async fn gate_fails_with_expired_assumption() {
        let active = make_assumption(1, AssumptionStatus::Active);
        let expired = make_assumption(2, AssumptionStatus::Expired);
        let result = super::evaluate_verification_gate(&[active, expired.clone()], None);
        match result {
            super::VerificationGateResult::Fail {
                failed_assumption_ids,
            } => {
                assert!(failed_assumption_ids.contains(&expired.id));
            }
            super::VerificationGateResult::Pass => panic!("gate must fail"),
        }
    }

    // Scenario: gate fails when any assumption is revoked.
    #[tokio::test]
    async fn gate_fails_with_revoked_assumption() {
        let revoked = make_assumption(3, AssumptionStatus::Revoked);
        let result = super::evaluate_verification_gate(&[revoked.clone()], None);
        assert!(matches!(result, super::VerificationGateResult::Fail { .. }));
    }

    // Scenario: gate fails when assumption has time-based expiry and now_ms >= expires_at.
    #[tokio::test]
    async fn gate_fails_when_assumption_time_expired() {
        let mut record = make_assumption(4, AssumptionStatus::Active);
        record.expires_at = Some(1_000_000); // expires at timestamp 1_000_000
        // now_ms = 1_000_001 → assumption is expired
        let result = super::evaluate_verification_gate(&[record.clone()], Some(1_000_001));
        assert!(matches!(result, super::VerificationGateResult::Fail { .. }));
    }

    // Scenario: gate passes when now_ms < expires_at (not yet expired).
    #[tokio::test]
    async fn gate_passes_when_not_yet_expired() {
        let mut record = make_assumption(5, AssumptionStatus::Active);
        record.expires_at = Some(1_000_000);
        let result = super::evaluate_verification_gate(&[record], Some(999_999));
        assert_eq!(result, super::VerificationGateResult::Pass);
    }

    // Scenario: gate passes with no assumptions.
    #[tokio::test]
    async fn gate_passes_with_no_assumptions() {
        let result = super::evaluate_verification_gate(&[], None);
        assert_eq!(result, super::VerificationGateResult::Pass);
    }

    // ── ObjectBackedApprovalStore ─────────────────────────────────────────

    // Scenario: object-backed store persists approval record.
    //   GIVEN ObjectBackedApprovalStore wrapping MemoryObjectStore
    //   WHEN store_approval then get_approval
    //   THEN record is retrieved intact
    #[tokio::test]
    async fn object_backed_approval_store_roundtrip() {
        use crate::backends::memory::MemoryObjectStore;
        let store = super::ObjectBackedApprovalStore::new(MemoryObjectStore::new());
        let record = make_approval(1, 1000);
        store.store_approval(record.clone()).await.expect("store");
        let got = store
            .get_approval(&record.id)
            .await
            .expect("get")
            .expect("must exist");
        assert_eq!(got, record);
    }

    // Scenario: object-backed store is idempotent on duplicate id.
    #[tokio::test]
    async fn object_backed_approval_store_idempotent() {
        use crate::backends::memory::MemoryObjectStore;
        let store = super::ObjectBackedApprovalStore::new(MemoryObjectStore::new());
        let record = make_approval(2, 2000);
        store.store_approval(record.clone()).await.expect("first");
        store
            .store_approval(record.clone())
            .await
            .expect("duplicate");
        let list = store.list_approvals().await.expect("list");
        assert_eq!(list.len(), 1);
    }

    // Scenario: object-backed store list_approvals returns all records sorted.
    #[tokio::test]
    async fn object_backed_approval_store_list_sorted() {
        use crate::backends::memory::MemoryObjectStore;
        let store = super::ObjectBackedApprovalStore::new(MemoryObjectStore::new());
        store
            .store_approval(make_approval(1, 500))
            .await
            .expect("a");
        store
            .store_approval(make_approval(2, 100))
            .await
            .expect("b");
        let list = store.list_approvals().await.expect("list");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].timestamp, 100);
        assert_eq!(list[1].timestamp, 500);
    }

    // ── ObjectBackedAssumptionStore ───────────────────────────────────────

    // Scenario: object-backed assumption store persists record.
    #[tokio::test]
    async fn object_backed_assumption_store_roundtrip() {
        use crate::backends::memory::MemoryObjectStore;
        let store = super::ObjectBackedAssumptionStore::new(MemoryObjectStore::new());
        let record = make_assumption(1, AssumptionStatus::Active);
        store.store_assumption(record.clone()).await.expect("store");
        let got = store
            .get_assumption(&record.id)
            .await
            .expect("get")
            .expect("must exist");
        assert_eq!(got, record);
    }

    // Scenario: object-backed assumption store is idempotent.
    #[tokio::test]
    async fn object_backed_assumption_store_idempotent() {
        use crate::backends::memory::MemoryObjectStore;
        let store = super::ObjectBackedAssumptionStore::new(MemoryObjectStore::new());
        let record = make_assumption(1, AssumptionStatus::Active);
        store.store_assumption(record.clone()).await.expect("first");
        store
            .store_assumption(record.clone())
            .await
            .expect("duplicate");
        let list = store.list_assumptions().await.expect("list");
        assert_eq!(list.len(), 1);
    }

    // Scenario: object-backed assumption store returns all records.
    #[tokio::test]
    async fn object_backed_assumption_store_list_all() {
        use crate::backends::memory::MemoryObjectStore;
        let store = super::ObjectBackedAssumptionStore::new(MemoryObjectStore::new());
        store
            .store_assumption(make_assumption(1, AssumptionStatus::Active))
            .await
            .expect("a1");
        store
            .store_assumption(make_assumption(2, AssumptionStatus::Revoked))
            .await
            .expect("a2");
        let list = store.list_assumptions().await.expect("list");
        assert_eq!(list.len(), 2);
    }
}
