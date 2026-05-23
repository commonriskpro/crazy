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

use ail_storage::object::ObjectId;
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
}
