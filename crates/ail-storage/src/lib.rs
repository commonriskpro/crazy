/// `ail-storage` — async-native storage contracts and deterministic CBOR codec.
///
/// # Module overview
///
/// - [`codec`]    — [`ContentCodec`](codec::ContentCodec) trait + [`CborCodec`](codec::CborCodec) impl.
/// - [`error`]    — [`StorageError`](error::StorageError) and [`StorageResult`](error::StorageResult).
/// - [`object`]   — Content-addressed `ObjectId`, `RawObject`, and `ObjectStore` (Phase 2).
/// - [`graph`]    — Snapshot/log envelopes and `GraphStore` (Phase 3).
/// - [`backends`] — Test-only `MemoryObjectStore` and `TempfileObjectStore` (Phase 2).
pub mod backends;
pub mod codec;
pub mod error;
pub mod graph;
pub mod object;

pub use graph::{ChangeSetLogEntry, GraphStore, ObjectBackedGraphStore, SnapshotEnvelope};
