// Content-addressed object types.
//
// # Content addressing
//
// Every `RawObject` is identified by the BLAKE3 hash of its bytes. Storing
// the same bytes twice always returns the same `ObjectId`; no separate UUID
// or sequence ID is needed.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::StorageResult;

/// A 32-byte BLAKE3 hash that uniquely identifies a `RawObject` by its content.
///
/// `ObjectId` is derived from the raw bytes of the object; storing identical
/// bytes in any backend always yields the same identifier.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct ObjectId([u8; 32]);

impl ObjectId {
    /// Compute an `ObjectId` from the BLAKE3 hash of `bytes`.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let hash = blake3::hash(bytes);
        ObjectId(*hash.as_bytes())
    }

    /// Return the raw 32-byte hash.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Return the lower-hex string representation of the hash.
    pub fn to_hex(&self) -> String {
        self.0.iter().fold(String::with_capacity(64), |mut acc, b| {
            use std::fmt::Write;
            let _ = write!(acc, "{b:02x}");
            acc
        })
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl From<[u8; 32]> for ObjectId {
    fn from(bytes: [u8; 32]) -> Self {
        ObjectId(bytes)
    }
}

impl Default for ObjectId {
    /// Returns the zero `ObjectId` (all 32 bytes are `0x00`).
    ///
    /// The zero id is a sentinel for "not yet assigned"; a real BLAKE3 hash
    /// will never be all-zeros in practice (cryptographically infeasible).
    fn default() -> Self {
        ObjectId([0u8; 32])
    }
}

/// Raw bytes of a content-addressed object.
///
/// No schema is assumed; the bytes are typically the CBOR encoding of a
/// domain value. The `ObjectStore` implementation is agnostic to their meaning.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RawObject(pub Vec<u8>);

/// Async trait for a content-addressed object store.
///
/// # Content addressing
///
/// `put` computes the `ObjectId` from the bytes and returns it. Callers
/// should not pre-compute the id; the store is the canonical source of truth.
///
/// # Errors
///
/// All methods return `StorageResult<_>`. `get` and `exists` never return
/// `StorageError::NotFound` — a missing object is represented as `None` /
/// `false` respectively; `NotFound` is reserved for higher-level semantics.
pub trait ObjectStore {
    /// Store `object` and return its content-addressed `ObjectId`.
    ///
    /// If an object with the same id already exists, this is a no-op and
    /// the existing id is returned.
    fn put(&self, object: RawObject) -> impl Future<Output = StorageResult<ObjectId>> + Send;

    /// Retrieve the object identified by `id`, or `None` if absent.
    fn get(&self, id: &ObjectId) -> impl Future<Output = StorageResult<Option<RawObject>>> + Send;

    /// Return `true` if an object with the given `id` exists in the store.
    fn exists(&self, id: &ObjectId) -> impl Future<Output = StorageResult<bool>> + Send;
}

use std::future::Future;
