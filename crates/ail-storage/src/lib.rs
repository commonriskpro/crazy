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
/// - [`branch`]    — `Branch`, `BranchStore`, `BranchRegistry` (G28).
/// - [`tag`]       — `Tag`, `ReleaseMetadata`, `TagStore`, `TagRegistry` (G28).
/// - [`approval`]  — `ApprovalRecord`, `AssumptionRecord`, stores (G28).
/// - [`export`]    — `ExportBundle`, `ExportScope`, `build_export_bundle` (G28).
/// - [`integrity`] — `IntegrityReport`, `IntegrityIssue`, `verify_integrity` (G28).
/// - [`tombstone`] — `Tombstone`, `TombstoneStore`, `ObjectBackedTombstoneStore`
///   for logical-delete records (Track B Gap 1).
/// - [`diff`]      — `StructuralDiff`, `StructuralDiffStore`,
///   `ObjectBackedStructuralDiffStore` (Track B Gap 3).
pub mod approval;
pub mod backends;
pub mod branch;
pub mod codec;
pub mod diff;
pub mod error;
pub mod export;
pub mod graph;
pub mod integrity;
pub mod migration;
pub mod object;
pub mod retention;
pub mod tag;
pub mod tombstone;

pub use approval::{
    ApprovalRecord, ApprovalRegistry, ApprovalStore, AssumptionRecord, AssumptionRegistry,
    AssumptionStatus, AssumptionStore, ObjectBackedApprovalStore, ObjectBackedAssumptionStore,
    VerificationGateResult, approval_is_valid, evaluate_verification_gate,
    validate_assumption_boundary,
};
pub use backends::postgres::PostgresGraphStore;
pub use branch::{Branch, BranchRegistry, BranchStore};
pub use export::{ExportBundle, ExportScope, build_export_bundle};
pub use graph::{ChangeSetLogEntry, GraphStore, ObjectBackedGraphStore, SnapshotEnvelope};
pub use integrity::{IntegrityInput, IntegrityIssue, IntegrityReport, verify_integrity};
pub use migration::{
    DomainMigration, DomainVersions, Migration, MigrationCatalog, MigrationError, MigrationOutput,
    MigrationReport, V0ToV1Migration, V1ToV2Migration, V2ToV3Migration,
};
pub use retention::{
    CompactionReport, GcReport, MutableGraphStore, RetentionPolicy, compact_snapshots,
    gc_unreferenced,
};
pub use tag::{ReleaseMetadata, Tag, TagRegistry, TagStore};
pub use tombstone::{ObjectBackedTombstoneStore, Tombstone, TombstoneStore};
pub use diff::{ObjectBackedStructuralDiffStore, StructuralDiff, StructuralDiffStore};
