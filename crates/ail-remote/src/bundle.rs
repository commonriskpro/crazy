// ── ail-remote::bundle ────────────────────────────────────────────────────
//
// Content-addressed object bundles for cross-process / network transfer.
//
// # Integrity model
//
// Every entry in an `ObjectBundle` is keyed by the BLAKE3 hash of its raw
// bytes (`ObjectId::from_bytes`).  `verify_integrity()` re-derives the
// expected `ObjectId` from each entry's bytes and checks it against the
// stored key, detecting any post-construction tampering.
//
// The `root` field declares which `ObjectId` is the bundle's root object;
// `verify_integrity()` also confirms that the root key exists in the map.
//
// # Determinism
//
// `ObjectBundle` uses `BTreeMap` (not `HashMap`) to guarantee deterministic
// CBOR serialization key order.  This satisfies the workspace codec contract.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use ail_storage::SnapshotEnvelope;
use ail_storage::codec::{CborCodec, ContentCodec};
use ail_storage::error::StorageError;
use ail_storage::object::{ObjectId, ObjectStore};
use serde::{Deserialize, Serialize};

// ── BundleError ───────────────────────────────────────────────────────────

/// Error returned by `ObjectBundle::verify_integrity()`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BundleError {
    /// The declared `root` `ObjectId` is absent from the objects map.
    RootNotFound,
    /// An entry's bytes do not hash to their stored key.
    HashMismatch {
        /// The `ObjectId` whose stored key does not match the hash of its bytes.
        object_id: ObjectId,
    },
}

impl fmt::Display for BundleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BundleError::RootNotFound => write!(f, "bundle root object is not present in the map"),
            BundleError::HashMismatch { object_id } => {
                write!(f, "hash mismatch for object {object_id}")
            }
        }
    }
}

impl std::error::Error for BundleError {}

// ── FileBundleStoreError ──────────────────────────────────────────────────

/// Error returned by fallible [`FileBundleStore`] operations.
#[derive(Debug)]
pub enum FileBundleStoreError {
    /// A filesystem operation failed.
    Io(io::Error),
    /// Bundle CBOR encoding or decoding failed.
    Codec(String),
    /// The decoded bundle failed integrity verification.
    Integrity(BundleError),
    /// The bundle file decoded successfully but belongs to another root.
    RootMismatch {
        /// The root requested by the caller.
        expected: ObjectId,
        /// The root encoded inside the bundle file.
        found: ObjectId,
    },
}

impl fmt::Display for FileBundleStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileBundleStoreError::Io(err) => write!(f, "bundle store io error: {err}"),
            FileBundleStoreError::Codec(msg) => write!(f, "bundle store codec error: {msg}"),
            FileBundleStoreError::Integrity(err) => {
                write!(f, "bundle store integrity error: {err}")
            }
            FileBundleStoreError::RootMismatch { expected, found } => write!(
                f,
                "bundle file root mismatch: expected {expected}, found {found}"
            ),
        }
    }
}

impl std::error::Error for FileBundleStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FileBundleStoreError::Io(err) => Some(err),
            FileBundleStoreError::Integrity(err) => Some(err),
            FileBundleStoreError::Codec(_) | FileBundleStoreError::RootMismatch { .. } => None,
        }
    }
}

impl From<io::Error> for FileBundleStoreError {
    fn from(err: io::Error) -> Self {
        FileBundleStoreError::Io(err)
    }
}

fn bundle_codec_error(err: StorageError) -> FileBundleStoreError {
    match err {
        StorageError::Codec(msg) => FileBundleStoreError::Codec(msg),
        StorageError::Io(err) => FileBundleStoreError::Io(err),
        other => FileBundleStoreError::Codec(other.to_string()),
    }
}

// ── ObjectBundle ──────────────────────────────────────────────────────────

/// A content-addressed bundle of raw objects for cross-boundary transfer.
///
/// The `objects` map keys are BLAKE3 hashes of the corresponding byte
/// values; `root` identifies the bundle's root entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectBundle {
    /// The `ObjectId` of the bundle's root object.
    pub root: ObjectId,
    /// All objects in the bundle, keyed by `ObjectId` (BLAKE3 hash of bytes).
    pub objects: BTreeMap<ObjectId, Vec<u8>>,
}

impl ObjectBundle {
    /// Construct a new `ObjectBundle` with the given root and objects map.
    ///
    /// The caller is responsible for ensuring each key equals
    /// `ObjectId::from_bytes(&value)`.  Call `verify_integrity()` to confirm.
    pub fn new(root: ObjectId, objects: BTreeMap<ObjectId, Vec<u8>>) -> Self {
        Self { root, objects }
    }

    /// Build a bundle from `root` plus any directly referenced objects declared
    /// by a snapshot envelope root.
    ///
    /// This is intentionally a conservative traversal foundation: raw graph
    /// objects remain opaque, but snapshot envelopes declare stable object ids
    /// for the graph root and associated metadata records.
    pub async fn from_store_with_snapshot_dependencies<S>(
        root: ObjectId,
        store: &S,
    ) -> Result<Self, StorageError>
    where
        S: ObjectStore + Sync,
    {
        let root_bytes = store.get(&root).await?.ok_or(StorageError::NotFound)?.0;
        let mut objects = BTreeMap::new();
        for dependency in snapshot_envelope_dependencies(&root_bytes) {
            if dependency == root || objects.contains_key(&dependency) {
                continue;
            }
            if let Some(bytes) = store.get(&dependency).await? {
                objects.insert(dependency, bytes.0);
            }
        }
        objects.insert(root, root_bytes);
        Ok(Self::new(root, objects))
    }

    /// Return true when this bundle contains at least one direct dependency
    /// declared by a `SnapshotEnvelope` root.
    #[must_use]
    pub fn includes_snapshot_envelope_dependencies(&self) -> bool {
        let Some(root_bytes) = self.objects.get(&self.root) else {
            return false;
        };
        snapshot_envelope_dependencies(root_bytes)
            .into_iter()
            .any(|dependency| dependency != self.root && self.objects.contains_key(&dependency))
    }

    /// Verify that every entry's stored key equals `blake3(bytes)` and that
    /// the declared root key exists in the map.
    ///
    /// # Errors
    ///
    /// - `BundleError::RootNotFound` if `root` is absent from `objects`.
    /// - `BundleError::HashMismatch { object_id }` for the first entry whose
    ///   key does not match the hash of its bytes.
    pub fn verify_integrity(&self) -> Result<(), BundleError> {
        // Check root presence first.
        if !self.objects.contains_key(&self.root) {
            return Err(BundleError::RootNotFound);
        }

        // Verify every entry's hash.
        for (stored_id, bytes) in &self.objects {
            let expected_id = ObjectId::from_bytes(bytes);
            if expected_id != *stored_id {
                return Err(BundleError::HashMismatch {
                    object_id: *stored_id,
                });
            }
        }

        Ok(())
    }
}

fn snapshot_envelope_dependencies(bytes: &[u8]) -> Vec<ObjectId> {
    let Ok(snapshot) = CborCodec.decode::<SnapshotEnvelope>(bytes) else {
        return vec![];
    };
    let mut dependencies = vec![snapshot.graph_root_hash];
    dependencies.extend(snapshot.parent_id);
    dependencies.extend(snapshot.applied_change_id);
    dependencies.extend(snapshot.audit_record_ids);
    dependencies.extend(snapshot.migration_metadata_ids);
    dependencies
}

// ── BundleStore ───────────────────────────────────────────────────────────

/// Storage boundary for accepted remote object bundles.
///
/// Implementations may keep bundles in memory, on disk, or in a database.  The
/// store assumes callers verify [`ObjectBundle::verify_integrity`] before write.
pub trait BundleStore {
    /// Store an accepted bundle, replacing any existing bundle with the same root.
    fn put_bundle(&mut self, bundle: ObjectBundle);

    /// Return the bundle for `root`, or `None` when the root is unknown.
    fn get_bundle(&self, root: &ObjectId) -> Option<ObjectBundle>;
}

/// In-memory bundle store for ephemeral coordinator instances and tests.
#[derive(Clone, Debug, Default)]
pub struct InMemoryBundleStore {
    bundles: BTreeMap<ObjectId, ObjectBundle>,
}

impl InMemoryBundleStore {
    /// Create an empty in-memory bundle store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl BundleStore for InMemoryBundleStore {
    fn put_bundle(&mut self, bundle: ObjectBundle) {
        self.bundles.insert(bundle.root, bundle);
    }

    fn get_bundle(&self, root: &ObjectId) -> Option<ObjectBundle> {
        self.bundles.get(root).cloned()
    }
}

/// Disk-backed bundle store rooted at a directory.
///
/// Bundles are encoded with [`CborCodec`] into one file per root object id.  The
/// store assumes callers verify [`ObjectBundle::verify_integrity`] before write;
/// reads verify decoded bundles before returning them.
#[derive(Clone)]
pub struct FileBundleStore {
    root_dir: PathBuf,
    codec: CborCodec,
}

impl FileBundleStore {
    /// Create or open a disk-backed bundle store at `root_dir`.
    ///
    /// # Errors
    ///
    /// Returns [`FileBundleStoreError::Io`] if the directory cannot be created.
    pub fn new(root_dir: impl Into<PathBuf>) -> Result<Self, FileBundleStoreError> {
        let root_dir = root_dir.into();
        fs::create_dir_all(&root_dir)?;
        Ok(Self {
            root_dir,
            codec: CborCodec,
        })
    }

    /// Return the directory used by this store.
    #[must_use]
    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    /// Store `bundle` using deterministic CBOR encoding.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding or filesystem writes fail.
    pub fn try_put_bundle(&self, bundle: &ObjectBundle) -> Result<(), FileBundleStoreError> {
        let bytes = self.codec.encode(bundle).map_err(bundle_codec_error)?;
        let path = self.bundle_path(&bundle.root);
        let tmp_path = self.tmp_bundle_path(&bundle.root);

        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);

        fs::rename(tmp_path, path)?;
        Ok(())
    }

    /// Return the stored bundle for `root`, or `Ok(None)` when absent.
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be read, decoded, or
    /// verified.
    pub fn try_get_bundle(
        &self,
        root: &ObjectId,
    ) -> Result<Option<ObjectBundle>, FileBundleStoreError> {
        let path = self.bundle_path(root);
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(FileBundleStoreError::Io(err)),
        };

        let bundle: ObjectBundle = self.codec.decode(&bytes).map_err(bundle_codec_error)?;
        if bundle.root != *root {
            return Err(FileBundleStoreError::RootMismatch {
                expected: *root,
                found: bundle.root,
            });
        }
        bundle
            .verify_integrity()
            .map_err(FileBundleStoreError::Integrity)?;
        Ok(Some(bundle))
    }

    fn bundle_path(&self, root: &ObjectId) -> PathBuf {
        self.root_dir.join(format!("{}.cbor", root.to_hex()))
    }

    fn tmp_bundle_path(&self, root: &ObjectId) -> PathBuf {
        self.root_dir.join(format!("{}.tmp", root.to_hex()))
    }
}

impl fmt::Debug for FileBundleStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileBundleStore")
            .field("root_dir", &self.root_dir)
            .finish_non_exhaustive()
    }
}

impl BundleStore for FileBundleStore {
    fn put_bundle(&mut self, bundle: ObjectBundle) {
        let _ = self.try_put_bundle(&bundle);
    }

    fn get_bundle(&self, root: &ObjectId) -> Option<ObjectBundle> {
        self.try_get_bundle(root).ok().flatten()
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_bundle() -> ObjectBundle {
        let bytes = b"hello world".to_vec();
        let id = ObjectId::from_bytes(&bytes);
        let mut objects = BTreeMap::new();
        objects.insert(id, bytes);
        ObjectBundle::new(id, objects)
    }

    // ── valid_bundle_passes_verify_integrity ──────────────────────────────
    // Task 3.2 / Spec: valid bundle verifies successfully.
    #[test]
    fn valid_bundle_passes_verify_integrity() {
        let bundle = valid_bundle();
        bundle
            .verify_integrity()
            .expect("valid bundle must pass integrity check");
    }

    // ── tampered_byte_fails_with_hash_mismatch ────────────────────────────
    // Task 3.2 / Spec: tampered bundle returns HashMismatch.
    #[test]
    fn tampered_byte_fails_with_hash_mismatch() {
        let mut bundle = valid_bundle();
        // Tamper by replacing the bytes in-place while keeping the original key.
        let key = bundle.root;
        bundle.objects.insert(key, b"tampered bytes!!!".to_vec());
        let result = bundle.verify_integrity();
        assert_eq!(
            result,
            Err(BundleError::HashMismatch { object_id: key }),
            "tampered bytes must return HashMismatch"
        );
    }

    // ── missing_root_fails_with_root_not_found ────────────────────────────
    // Task 3.2 / Spec: missing root fails with RootNotFound.
    #[test]
    fn missing_root_fails_with_root_not_found() {
        // Build a bundle with a root that does NOT appear in the objects map.
        let phantom_root = ObjectId::from_bytes(b"phantom root");
        let bundle = ObjectBundle::new(phantom_root, BTreeMap::new());
        let result = bundle.verify_integrity();
        assert_eq!(
            result,
            Err(BundleError::RootNotFound),
            "absent root must return RootNotFound"
        );
    }

    #[test]
    fn in_memory_store_returns_stored_bundle() {
        let bundle = valid_bundle();
        let root = bundle.root;
        let mut store = InMemoryBundleStore::new();

        store.put_bundle(bundle.clone());

        assert_eq!(store.get_bundle(&root), Some(bundle));
    }

    #[test]
    fn in_memory_store_returns_none_for_missing_root() {
        let store = InMemoryBundleStore::new();
        let missing_root = ObjectId::from_bytes(b"missing root");

        assert_eq!(store.get_bundle(&missing_root), None);
    }

    #[test]
    fn file_store_returns_bundle_after_reopen() {
        let dir = tempfile::tempdir().expect("tempdir must be created");
        let bundle = valid_bundle();
        let root = bundle.root;

        {
            let store = FileBundleStore::new(dir.path()).expect("file store must open");
            store
                .try_put_bundle(&bundle)
                .expect("bundle write must succeed");
        }

        let reopened = FileBundleStore::new(dir.path()).expect("file store must reopen");
        assert_eq!(reopened.try_get_bundle(&root).unwrap(), Some(bundle));
    }

    #[test]
    fn file_store_returns_none_for_missing_root() {
        let dir = tempfile::tempdir().expect("tempdir must be created");
        let store = FileBundleStore::new(dir.path()).expect("file store must open");
        let missing_root = ObjectId::from_bytes(b"missing root");

        assert_eq!(store.try_get_bundle(&missing_root).unwrap(), None);
    }

    #[test]
    fn file_store_reports_corrupt_bundle_file() {
        let dir = tempfile::tempdir().expect("tempdir must be created");
        let store = FileBundleStore::new(dir.path()).expect("file store must open");
        let root = ObjectId::from_bytes(b"corrupt root");

        fs::write(store.bundle_path(&root), b"not valid cbor").expect("corrupt file write");

        let err = store
            .try_get_bundle(&root)
            .expect_err("corrupt file must fail");
        assert!(matches!(err, FileBundleStoreError::Codec(_)));
        assert_eq!(store.get_bundle(&root), None);
    }
}
