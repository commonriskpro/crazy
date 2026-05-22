// Tags and releases.
//
// # Design
//
// A `Tag` is a named, immutable pointer to a snapshot — unlike branches, tags
// are not updated after creation.  A tag may carry optional `ReleaseMetadata`
// that anchors the snapshot to a specific graph root hash and verification
// report.
//
// `TagRegistry` is the in-memory implementation, following the same
// `Arc<Mutex<HashMap<String, Tag>>>` pattern as `BranchRegistry`.
//
// # Determinism
//
// All serializable types follow the project's determinism contract: no
// HashMap fields, no floats, timestamps as u64 Unix milliseconds.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::error::{StorageError, StorageResult};
use crate::object::ObjectId;

// ── ReleaseMetadata ───────────────────────────────────────────────────────

/// Metadata attached to a tag that marks a formal release.
///
/// All four spec-required fields are present:
/// `graph_root_hash`, `verification_report_hash`, `runtime_profile_hash`,
/// and `artifact_hashes`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReleaseMetadata {
    /// Content-addressed root of the graph at release time.
    pub graph_root_hash: ObjectId,
    /// BLAKE3 hash of the verification report for this release, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_report_hash: Option<[u8; 32]>,
    /// BLAKE3 hash of the runtime profile captured at release time, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_profile_hash: Option<[u8; 32]>,
    /// Content-addressed hashes of protected artifacts included in this
    /// release (e.g. WASM blobs, generated SDKs).  Sorted for determinism.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_hashes: Vec<ObjectId>,
}

// ── Tag ───────────────────────────────────────────────────────────────────

/// A named, immutable pointer to a snapshot.
///
/// Tags are created once and never updated (unlike branches).  A tag may
/// optionally carry [`ReleaseMetadata`] to anchor it to a specific release.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tag {
    /// Human-readable identifier for this tag (e.g. `"v1.0"`, `"prod.2026-05-21"`).
    pub name: String,
    /// The snapshot this tag points to.
    pub snapshot_id: ObjectId,
    /// Unix timestamp in milliseconds when this tag was created.
    pub created_at: u64,
    /// Optional release metadata attached to this tag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_metadata: Option<ReleaseMetadata>,
}

// ── TagStore trait ────────────────────────────────────────────────────────

/// Async storage contract for tags.
pub trait TagStore {
    /// Create a new tag pointing to `snapshot_id`.
    ///
    /// Returns an error if a tag with `name` already exists.
    fn create_tag(
        &self,
        name: &str,
        snapshot_id: ObjectId,
        created_at: u64,
        release_metadata: Option<ReleaseMetadata>,
    ) -> impl Future<Output = StorageResult<Tag>> + Send;

    /// Return the tag named `name`, or `None` if it does not exist.
    fn get_tag(&self, name: &str) -> impl Future<Output = StorageResult<Option<Tag>>> + Send;

    /// Delete tag `name`.  No-op if the tag does not exist.
    fn delete_tag(&self, name: &str) -> impl Future<Output = StorageResult<()>> + Send;

    /// List all tags sorted by `created_at` (ties broken by name).
    fn list_tags(&self) -> impl Future<Output = StorageResult<Vec<Tag>>> + Send;
}

// ── TagRegistry ───────────────────────────────────────────────────────────

/// In-memory implementation of [`TagStore`].
#[derive(Clone, Default)]
pub struct TagRegistry {
    inner: Arc<Mutex<HashMap<String, Tag>>>,
}

impl TagRegistry {
    /// Create an empty `TagRegistry`.
    pub fn new() -> Self {
        Self::default()
    }
}

impl TagStore for TagRegistry {
    async fn create_tag(
        &self,
        name: &str,
        snapshot_id: ObjectId,
        created_at: u64,
        release_metadata: Option<ReleaseMetadata>,
    ) -> StorageResult<Tag> {
        let mut guard = self
            .inner
            .lock()
            .expect("tag_registry lock must not be poisoned");
        if guard.contains_key(name) {
            return Err(StorageError::Codec(format!("tag '{name}' already exists")));
        }
        let tag = Tag {
            name: name.to_owned(),
            snapshot_id,
            created_at,
            release_metadata,
        };
        guard.insert(name.to_owned(), tag.clone());
        Ok(tag)
    }

    async fn get_tag(&self, name: &str) -> StorageResult<Option<Tag>> {
        let guard = self
            .inner
            .lock()
            .expect("tag_registry lock must not be poisoned");
        Ok(guard.get(name).cloned())
    }

    async fn delete_tag(&self, name: &str) -> StorageResult<()> {
        let mut guard = self
            .inner
            .lock()
            .expect("tag_registry lock must not be poisoned");
        guard.remove(name);
        Ok(())
    }

    async fn list_tags(&self) -> StorageResult<Vec<Tag>> {
        let guard = self
            .inner
            .lock()
            .expect("tag_registry lock must not be poisoned");
        let mut tags: Vec<Tag> = guard.values().cloned().collect();
        tags.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.name.cmp(&b.name)));
        Ok(tags)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn snap_id(seed: u8) -> ObjectId {
        ObjectId::from_bytes(&[seed; 32])
    }

    fn root_id(seed: u8) -> ObjectId {
        ObjectId::from_bytes(&[seed; 32])
    }

    // Scenario: list_tags returns empty on fresh registry.
    #[tokio::test]
    async fn list_tags_empty_on_fresh_registry() {
        let reg = TagRegistry::new();
        let list = reg.list_tags().await.expect("list must succeed");
        assert!(list.is_empty());
    }

    // Scenario: create then get returns tag.
    //   GIVEN create_tag("v1.0", snap_id, None)
    //   WHEN get_tag("v1.0")
    //   THEN returns Some(tag) with correct fields
    #[tokio::test]
    async fn create_and_get_tag() {
        let reg = TagRegistry::new();
        let id = snap_id(1);
        reg.create_tag("v1.0", id, 1000, None)
            .await
            .expect("create must succeed");
        let tag = reg
            .get_tag("v1.0")
            .await
            .expect("get must succeed")
            .expect("tag must exist");
        assert_eq!(tag.name, "v1.0");
        assert_eq!(tag.snapshot_id, id);
        assert_eq!(tag.created_at, 1000);
        assert!(tag.release_metadata.is_none());
    }

    // Scenario: create tag with release metadata (all four fields).
    //   GIVEN create_tag with ReleaseMetadata carrying all four spec fields
    //   WHEN get_tag
    //   THEN metadata is round-tripped intact
    #[tokio::test]
    async fn create_tag_with_release_metadata() {
        let reg = TagRegistry::new();
        let meta = ReleaseMetadata {
            graph_root_hash: root_id(42),
            verification_report_hash: Some([0xab; 32]),
            runtime_profile_hash: Some([0xcd; 32]),
            artifact_hashes: vec![
                ObjectId::from_bytes(&[0x01; 32]),
                ObjectId::from_bytes(&[0x02; 32]),
            ],
        };
        reg.create_tag("v2.0", snap_id(2), 2000, Some(meta.clone()))
            .await
            .expect("create");
        let tag = reg
            .get_tag("v2.0")
            .await
            .expect("get")
            .expect("must exist");
        assert_eq!(tag.release_metadata, Some(meta));
    }

    // Scenario: ReleaseMetadata runtime_profile_hash field is stored.
    //   GIVEN create_tag with runtime_profile_hash set
    //   WHEN get_tag
    //   THEN runtime_profile_hash is present
    #[tokio::test]
    async fn release_metadata_runtime_profile_hash() {
        let reg = TagRegistry::new();
        let meta = ReleaseMetadata {
            graph_root_hash: root_id(1),
            verification_report_hash: None,
            runtime_profile_hash: Some([0xee; 32]),
            artifact_hashes: Vec::new(),
        };
        reg.create_tag("v3.0", snap_id(3), 3000, Some(meta.clone()))
            .await
            .expect("create");
        let tag = reg.get_tag("v3.0").await.expect("get").expect("must exist");
        let rm = tag.release_metadata.expect("must have release metadata");
        assert_eq!(rm.runtime_profile_hash, Some([0xee; 32]));
    }

    // Scenario: ReleaseMetadata artifact_hashes field is stored.
    //   GIVEN create_tag with artifact_hashes list
    //   WHEN get_tag
    //   THEN artifact_hashes are preserved
    #[tokio::test]
    async fn release_metadata_artifact_hashes() {
        let reg = TagRegistry::new();
        let hashes = vec![
            ObjectId::from_bytes(&[0xaa; 32]),
            ObjectId::from_bytes(&[0xbb; 32]),
            ObjectId::from_bytes(&[0xcc; 32]),
        ];
        let meta = ReleaseMetadata {
            graph_root_hash: root_id(2),
            verification_report_hash: None,
            runtime_profile_hash: None,
            artifact_hashes: hashes.clone(),
        };
        reg.create_tag("v4.0", snap_id(4), 4000, Some(meta))
            .await
            .expect("create");
        let tag = reg.get_tag("v4.0").await.expect("get").expect("must exist");
        let rm = tag.release_metadata.expect("must have release metadata");
        assert_eq!(rm.artifact_hashes, hashes);
    }

    // Scenario: duplicate tag creation returns error.
    #[tokio::test]
    async fn create_duplicate_tag_errors() {
        let reg = TagRegistry::new();
        reg.create_tag("v1.0", snap_id(1), 0, None)
            .await
            .expect("first create");
        let err = reg.create_tag("v1.0", snap_id(2), 1, None).await;
        assert!(err.is_err(), "duplicate must fail");
    }

    // Scenario: delete_tag removes tag.
    #[tokio::test]
    async fn delete_tag_removes_tag() {
        let reg = TagRegistry::new();
        reg.create_tag("v1.0", snap_id(1), 0, None)
            .await
            .expect("create");
        reg.delete_tag("v1.0").await.expect("delete");
        let result = reg.get_tag("v1.0").await.expect("get");
        assert!(result.is_none());
    }

    // Scenario: delete nonexistent tag is no-op.
    #[tokio::test]
    async fn delete_nonexistent_tag_is_noop() {
        let reg = TagRegistry::new();
        reg.delete_tag("ghost").await.expect("noop must succeed");
    }

    // Scenario: list_tags returns all created tags.
    #[tokio::test]
    async fn list_tags_returns_all() {
        let reg = TagRegistry::new();
        reg.create_tag("v1.0", snap_id(1), 100, None)
            .await
            .expect("create v1.0");
        reg.create_tag("v2.0", snap_id(2), 200, None)
            .await
            .expect("create v2.0");
        let list = reg.list_tags().await.expect("list");
        assert_eq!(list.len(), 2);
        let names: Vec<&str> = list.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"v1.0"));
        assert!(names.contains(&"v2.0"));
    }

    // Scenario: list_tags sorted by created_at.
    #[tokio::test]
    async fn list_tags_sorted_by_created_at() {
        let reg = TagRegistry::new();
        reg.create_tag("z-late", snap_id(1), 500, None)
            .await
            .expect("create z");
        reg.create_tag("a-early", snap_id(2), 100, None)
            .await
            .expect("create a");
        let list = reg.list_tags().await.expect("list");
        assert_eq!(list[0].name, "a-early");
        assert_eq!(list[1].name, "z-late");
    }
}
