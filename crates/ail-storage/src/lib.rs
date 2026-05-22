/// `ail-storage` — async-native storage contracts and deterministic CBOR codec.
///
/// # Module overview
///
/// - [`codec`]     — [`ContentCodec`](codec::ContentCodec) trait + [`CborCodec`](codec::CborCodec) impl.
/// - [`error`]     — [`StorageError`](error::StorageError) and [`StorageResult`](error::StorageResult).
/// - [`object`]    — Content-addressed `ObjectId`, `RawObject`, and `ObjectStore` (Phase 2).
/// - [`graph`]     — Snapshot/log envelopes and `GraphStore` (Phase 3).
/// - [`backends`]  — Test-only `MemoryObjectStore` and `TempfileObjectStore` (Phase 2).
/// - [`retention`] — `RetentionPolicy`, `GcReport`, `CompactionReport`,
///   `gc_unreferenced`, `compact_snapshots` (G18).
/// - [`migration`] — Schema `Migration` trait, `MigrationCatalog`, `MigrationError`,
///   and `default_catalog()` (Phase 18 / PR3).
pub mod backends;
pub mod codec;
pub mod error;
pub mod graph;
pub mod migration;
pub mod object;
pub mod retention;

pub use backends::postgres::PostgresGraphStore;
pub use migration::{Migration, MigrationCatalog, MigrationError};
pub use graph::{ChangeSetLogEntry, GraphStore, ObjectBackedGraphStore, SnapshotEnvelope};
pub use retention::{
    CompactionReport, GcReport, MutableGraphStore, RetentionPolicy, compact_snapshots,
    gc_unreferenced,
};
