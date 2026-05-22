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
// registries, consistent with the pattern used for branches and tags.
//
// # Determinism
//
// All serializable types follow the project's determinism contract: no
// HashMap fields, no floats, timestamps as u64 Unix milliseconds.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::error::StorageResult;
use crate::object::ObjectId;

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
    fn list_assumptions(
        &self,
    ) -> impl Future<Output = StorageResult<Vec<AssumptionRecord>>> + Send;
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
        reg.store_approval(record.clone()).await.expect("first store");
        reg.store_approval(record.clone()).await.expect("duplicate store must succeed");
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
        reg.store_assumption(record.clone()).await.expect("duplicate");
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
}
