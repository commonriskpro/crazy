// Branch pointers to snapshots.
//
// # Design
//
// A `Branch` is a named, mutable pointer to a `SnapshotEnvelope`.  Branches
// are updated by appending ChangeSets; the pointer is moved to the new
// snapshot after each successful apply.
//
// `BranchRegistry` is the in-memory implementation.  It uses
// `Arc<Mutex<HashMap<String, Branch>>>` so the registry can be cloned and
// shared across `&self` async calls without ownership transfer.
//
// # Determinism
//
// `Branch` fields follow the project's determinism contract: no HashMap,
// no floats, timestamps as u64 Unix milliseconds.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::error::{StorageError, StorageResult};
use crate::object::ObjectId;

// ── Branch ────────────────────────────────────────────────────────────────

/// A named, mutable pointer to a snapshot.
///
/// The `name` is the human-readable branch identifier (e.g. `"main"`,
/// `"feature.checkout"`).  `target_snapshot_id` is updated each time the
/// branch advances to a new snapshot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Branch {
    /// Human-readable identifier for this branch.
    pub name: String,
    /// The snapshot this branch currently points to.
    pub target_snapshot_id: ObjectId,
    /// Unix timestamp in milliseconds when this branch was created.
    pub created_at: u64,
}

// ── BranchStore trait ─────────────────────────────────────────────────────

/// Async storage contract for branches.
pub trait BranchStore {
    /// Create a new branch pointing to `target_snapshot_id`.
    ///
    /// Returns `StorageError::AlreadyExists` if a branch with `name` already
    /// exists.  (Uses `StorageError::Codec` as the closest existing variant
    /// for "already exists" until a dedicated variant is added.)
    fn create_branch(
        &self,
        name: &str,
        target_snapshot_id: ObjectId,
        created_at: u64,
    ) -> impl Future<Output = StorageResult<Branch>> + Send;

    /// Return the branch named `name`, or `None` if it does not exist.
    fn get_branch(&self, name: &str) -> impl Future<Output = StorageResult<Option<Branch>>> + Send;

    /// Move branch `name` to point at `new_target_snapshot_id`.
    ///
    /// Returns `StorageError::NotFound` if `name` does not exist.
    fn update_branch(
        &self,
        name: &str,
        new_target_snapshot_id: ObjectId,
    ) -> impl Future<Output = StorageResult<Branch>> + Send;

    /// Delete branch `name`.  No-op if the branch does not exist.
    fn delete_branch(&self, name: &str) -> impl Future<Output = StorageResult<()>> + Send;

    /// List all branches in insertion order (by `created_at`, ties broken by name).
    fn list_branches(&self) -> impl Future<Output = StorageResult<Vec<Branch>>> + Send;
}

// ── BranchRegistry ────────────────────────────────────────────────────────

/// In-memory implementation of [`BranchStore`].
///
/// Backed by `Arc<Mutex<HashMap<String, Branch>>>` so the registry can be
/// cloned and shared across async tasks.
#[derive(Clone, Default)]
pub struct BranchRegistry {
    inner: Arc<Mutex<HashMap<String, Branch>>>,
}

impl BranchRegistry {
    /// Create an empty `BranchRegistry`.
    pub fn new() -> Self {
        Self::default()
    }
}

impl BranchStore for BranchRegistry {
    async fn create_branch(
        &self,
        name: &str,
        target_snapshot_id: ObjectId,
        created_at: u64,
    ) -> StorageResult<Branch> {
        let mut guard = self
            .inner
            .lock()
            .expect("branch_registry lock must not be poisoned");
        if guard.contains_key(name) {
            return Err(StorageError::Codec(format!(
                "branch '{name}' already exists"
            )));
        }
        let branch = Branch {
            name: name.to_owned(),
            target_snapshot_id,
            created_at,
        };
        guard.insert(name.to_owned(), branch.clone());
        Ok(branch)
    }

    async fn get_branch(&self, name: &str) -> StorageResult<Option<Branch>> {
        let guard = self
            .inner
            .lock()
            .expect("branch_registry lock must not be poisoned");
        Ok(guard.get(name).cloned())
    }

    async fn update_branch(
        &self,
        name: &str,
        new_target_snapshot_id: ObjectId,
    ) -> StorageResult<Branch> {
        let mut guard = self
            .inner
            .lock()
            .expect("branch_registry lock must not be poisoned");
        match guard.get_mut(name) {
            None => Err(StorageError::NotFound),
            Some(branch) => {
                branch.target_snapshot_id = new_target_snapshot_id;
                Ok(branch.clone())
            }
        }
    }

    async fn delete_branch(&self, name: &str) -> StorageResult<()> {
        let mut guard = self
            .inner
            .lock()
            .expect("branch_registry lock must not be poisoned");
        guard.remove(name);
        Ok(())
    }

    async fn list_branches(&self) -> StorageResult<Vec<Branch>> {
        let guard = self
            .inner
            .lock()
            .expect("branch_registry lock must not be poisoned");
        let mut branches: Vec<Branch> = guard.values().cloned().collect();
        // Sort by created_at, ties broken by name for full determinism.
        branches.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.name.cmp(&b.name)));
        Ok(branches)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn snap_id(seed: u8) -> ObjectId {
        ObjectId::from_bytes(&[seed; 32])
    }

    // Scenario: list_branches returns empty vec when no branches exist.
    //   GIVEN a fresh BranchRegistry
    //   WHEN list_branches called
    //   THEN empty vec returned
    #[tokio::test]
    async fn list_branches_empty_on_fresh_registry() {
        let reg = BranchRegistry::new();
        let list = reg.list_branches().await.expect("list must succeed");
        assert!(list.is_empty());
    }

    // Scenario: create then get returns branch.
    //   GIVEN create_branch("main", snap_id)
    //   WHEN get_branch("main")
    //   THEN returns Some(branch) with correct name and target
    #[tokio::test]
    async fn create_and_get_branch() {
        let reg = BranchRegistry::new();
        let id = snap_id(1);
        reg.create_branch("main", id, 1000)
            .await
            .expect("create must succeed");
        let branch = reg
            .get_branch("main")
            .await
            .expect("get must succeed")
            .expect("branch must exist");
        assert_eq!(branch.name, "main");
        assert_eq!(branch.target_snapshot_id, id);
        assert_eq!(branch.created_at, 1000);
    }

    // Scenario: creating duplicate branch returns error.
    #[tokio::test]
    async fn create_duplicate_branch_errors() {
        let reg = BranchRegistry::new();
        reg.create_branch("main", snap_id(1), 0)
            .await
            .expect("first create");
        let err = reg.create_branch("main", snap_id(2), 1).await;
        assert!(err.is_err(), "duplicate create must return error");
    }

    // Scenario: update_branch moves the pointer.
    //   GIVEN branch "main" pointing at snap 1
    //   WHEN update_branch("main", snap 2)
    //   THEN get_branch("main") returns snap 2
    #[tokio::test]
    async fn update_branch_moves_pointer() {
        let reg = BranchRegistry::new();
        reg.create_branch("main", snap_id(1), 0)
            .await
            .expect("create");
        reg.update_branch("main", snap_id(2))
            .await
            .expect("update must succeed");
        let branch = reg
            .get_branch("main")
            .await
            .expect("get")
            .expect("must exist");
        assert_eq!(branch.target_snapshot_id, snap_id(2));
    }

    // Scenario: update_branch on nonexistent branch returns NotFound.
    #[tokio::test]
    async fn update_nonexistent_branch_returns_not_found() {
        let reg = BranchRegistry::new();
        let err = reg.update_branch("ghost", snap_id(1)).await;
        assert!(matches!(err, Err(StorageError::NotFound)));
    }

    // Scenario: delete_branch removes the branch.
    //   GIVEN branch "main" exists
    //   WHEN delete_branch("main")
    //   THEN get_branch("main") returns None
    #[tokio::test]
    async fn delete_branch_removes_branch() {
        let reg = BranchRegistry::new();
        reg.create_branch("main", snap_id(1), 0)
            .await
            .expect("create");
        reg.delete_branch("main").await.expect("delete");
        let result = reg.get_branch("main").await.expect("get");
        assert!(result.is_none());
    }

    // Scenario: delete_branch on nonexistent branch is a no-op.
    #[tokio::test]
    async fn delete_nonexistent_branch_is_noop() {
        let reg = BranchRegistry::new();
        reg.delete_branch("ghost")
            .await
            .expect("delete noop must succeed");
    }

    // Scenario: list_branches returns all created branches.
    #[tokio::test]
    async fn list_branches_returns_all() {
        let reg = BranchRegistry::new();
        reg.create_branch("main", snap_id(1), 100)
            .await
            .expect("create main");
        reg.create_branch("feature", snap_id(2), 200)
            .await
            .expect("create feature");
        let list = reg.list_branches().await.expect("list");
        assert_eq!(list.len(), 2);
        let names: Vec<&str> = list.iter().map(|b| b.name.as_str()).collect();
        assert!(names.contains(&"main"));
        assert!(names.contains(&"feature"));
    }

    // Scenario: list_branches is sorted by created_at.
    #[tokio::test]
    async fn list_branches_sorted_by_created_at() {
        let reg = BranchRegistry::new();
        reg.create_branch("z-late", snap_id(1), 300)
            .await
            .expect("create z");
        reg.create_branch("a-early", snap_id(2), 100)
            .await
            .expect("create a");
        let list = reg.list_branches().await.expect("list");
        assert_eq!(list[0].name, "a-early");
        assert_eq!(list[1].name, "z-late");
    }
}
