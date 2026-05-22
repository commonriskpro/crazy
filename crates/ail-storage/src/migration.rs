// Schema migration runner.
//
// # Design
//
// Schema version is stored as a CBOR-encoded `u32` in a `RawObject` whose
// `ObjectId` is the BLAKE3 hash of the CBOR encoding of that version number.
// Version 0 is implicit: no version record exists in the store.
// Version N is present when `blake3(CBOR(N))` exists in the store.
//
// To read the current version we check known version ids in descending order
// and return the highest one that exists. This is O(N) in the number of
// registered migrations; for ≤100 schema versions that is negligible.
//
// # Multi-domain schema versioning
//
// `DomainVersions` tracks separate version tracks for the six schema domains
// mandated by storage.md: `graph`, `core_ir`, `acl`, `verification`, `runtime`,
// and `artifact`.  Each domain is versioned independently so a breaking
// graph-schema change does not force a re-migration of the ACL schema.
//
// # Migration creates a new snapshot
//
// When a migration produces a new graph state, `MigrationOutput` carries the
// new `SnapshotEnvelope`.  The old snapshot is never overwritten; both the
// pre-migration and post-migration snapshots remain readable.
//
// # MigrationReport
//
// `MigrationReport` records the structural equivalence assertion and any
// preserved semantics documented by the migration.  It is stored as a content-
// addressed object alongside the new snapshot.
//
// # Dyn-compatibility
//
// The `Migration` trait must be dyn-compatible so `MigrationCatalog` can hold
// a `Vec<Box<dyn Migration>>`. The `up` method therefore cannot be generic over
// the store type. Instead, `up` receives a concrete `MigrationStore` which
// holds a type-erased reference to any `ObjectStore`.
//
// `MigrationStore` uses `Arc<dyn ErasedObjectStore>`, where `ErasedObjectStore`
// is an internal object-safe async wrapper. This avoids the generic-on-method
// problem entirely.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::codec::{CborCodec, ContentCodec};
use crate::error::{StorageError, StorageResult};
use crate::graph::SnapshotEnvelope;
use crate::object::{ObjectId, ObjectStore, RawObject};

// ── DomainVersions ────────────────────────────────────────────────────────────

/// Per-domain schema versions.
///
/// Storage.md mandates separate version tracks for each of the six schema
/// domains.  A migration in one domain does not affect the version of any
/// other domain.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DomainVersions {
    /// Version of the graph schema (nodes, edges, snapshots).
    pub graph: u32,
    /// Version of the Core IR schema (expression/type representation).
    pub core_ir: u32,
    /// Version of the ACL/capability schema.
    pub acl: u32,
    /// Version of the verification schema (reports, policy artifacts).
    pub verification: u32,
    /// Version of the runtime schema (profiles, WASM contracts).
    pub runtime: u32,
    /// Version of the artifact schema (WASM blobs, SDKs, manifests).
    pub artifact: u32,
}

// ── MigrationOutput ───────────────────────────────────────────────────────────

/// The output produced by a schema migration step.
///
/// When the migration produces a new graph state, `new_snapshot` carries the
/// freshly created [`SnapshotEnvelope`].  The old snapshot is **never**
/// overwritten; both the pre- and post-migration snapshots remain in the store.
#[derive(Clone, Debug, Default)]
pub struct MigrationOutput {
    /// The snapshot created by this migration, if any.
    ///
    /// `None` when the migration is structural-only and produces no new graph
    /// state (e.g. a version-marker-only migration).
    pub new_snapshot: Option<SnapshotEnvelope>,
    /// The `MigrationReport` produced by this migration, if any.
    pub report: Option<MigrationReport>,
}

// ── MigrationReport ───────────────────────────────────────────────────────────

/// Documents the outcome of a single schema migration step.
///
/// Records whether the migration asserts structural equivalence between the
/// pre- and post-migration snapshots, and which semantic properties were
/// preserved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MigrationReport {
    /// Human-readable description of what the migration did.
    pub description: String,
    /// When `true` the migration asserts that the pre- and post-migration
    /// graph states are structurally equivalent (same semantics, different
    /// encoding).
    pub structural_equivalence: bool,
    /// A free-text list of semantic properties that were preserved by this
    /// migration (e.g. `"all function signatures preserved"`).
    pub preserved_semantics: Vec<String>,
    /// `ObjectId` of the pre-migration snapshot, if one existed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_snapshot_id: Option<ObjectId>,
    /// `ObjectId` of the post-migration snapshot, if one was created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_snapshot_id: Option<ObjectId>,
}

// ── Internal object-safe trait ────────────────────────────────────────────────

/// Object-safe, async-capable object store abstraction.
///
/// This is internal only. Callers use [`MigrationStore`].
trait ErasedObjectStore: Send + Sync {
    fn put_erased<'a>(
        &'a self,
        object: RawObject,
    ) -> Pin<Box<dyn Future<Output = StorageResult<ObjectId>> + Send + 'a>>;

    fn get_erased<'a>(
        &'a self,
        id: ObjectId,
    ) -> Pin<Box<dyn Future<Output = StorageResult<Option<RawObject>>> + Send + 'a>>;

    fn exists_erased<'a>(
        &'a self,
        id: ObjectId,
    ) -> Pin<Box<dyn Future<Output = StorageResult<bool>> + Send + 'a>>;
}

/// Blanket implementation for any concrete `ObjectStore`.
impl<S: ObjectStore + Send + Sync> ErasedObjectStore for S {
    fn put_erased<'a>(
        &'a self,
        object: RawObject,
    ) -> Pin<Box<dyn Future<Output = StorageResult<ObjectId>> + Send + 'a>> {
        Box::pin(self.put(object))
    }

    fn get_erased<'a>(
        &'a self,
        id: ObjectId,
    ) -> Pin<Box<dyn Future<Output = StorageResult<Option<RawObject>>> + Send + 'a>> {
        Box::pin(async move { self.get(&id).await })
    }

    fn exists_erased<'a>(
        &'a self,
        id: ObjectId,
    ) -> Pin<Box<dyn Future<Output = StorageResult<bool>> + Send + 'a>> {
        Box::pin(async move { self.exists(&id).await })
    }
}

// ── MigrationStore ────────────────────────────────────────────────────────────

/// A type-erased, cloneable handle to any `ObjectStore`.
///
/// Passed to [`Migration::up`] so the trait remains dyn-compatible.
#[derive(Clone)]
pub struct MigrationStore {
    inner: Arc<dyn ErasedObjectStore>,
}

impl MigrationStore {
    /// Wrap any concrete `ObjectStore` in a `MigrationStore`.
    pub fn new<S: ObjectStore + Send + Sync + 'static>(store: Arc<S>) -> Self {
        MigrationStore { inner: store }
    }

    /// Store `object` and return its content-addressed `ObjectId`.
    pub async fn put(&self, object: RawObject) -> StorageResult<ObjectId> {
        self.inner.put_erased(object).await
    }

    /// Retrieve the object identified by `id`, or `None` if absent.
    pub async fn get(&self, id: ObjectId) -> StorageResult<Option<RawObject>> {
        self.inner.get_erased(id).await
    }

    /// Return `true` if an object with the given `id` exists.
    pub async fn exists(&self, id: ObjectId) -> StorageResult<bool> {
        self.inner.exists_erased(id).await
    }
}

// ── MigrationError ────────────────────────────────────────────────────────────

/// Errors that can occur during schema migration.
#[derive(Debug)]
pub enum MigrationError {
    /// The store schema version does not match the migration's source version.
    VersionMismatch {
        /// Version the migration expected to find.
        expected: u32,
        /// Version actually found in the store.
        actual: u32,
    },
    /// An underlying storage operation failed.
    StorageError(StorageError),
    /// The store is already at the latest schema version.
    AlreadyAtVersion(u32),
}

impl std::fmt::Display for MigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MigrationError::VersionMismatch { expected, actual } => write!(
                f,
                "migration version mismatch: expected source version {expected}, store is at {actual}"
            ),
            MigrationError::StorageError(e) => write!(f, "storage error during migration: {e}"),
            MigrationError::AlreadyAtVersion(v) => {
                write!(f, "store is already at the latest schema version ({v})")
            }
        }
    }
}

impl std::error::Error for MigrationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            MigrationError::StorageError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<StorageError> for MigrationError {
    fn from(e: StorageError) -> Self {
        MigrationError::StorageError(e)
    }
}

// ── Migration trait ───────────────────────────────────────────────────────────

/// A single schema migration step.
///
/// Each implementation advances the store from [`source_version`](Migration::source_version)
/// to [`target_version`](Migration::target_version).
///
/// The `up` method receives a [`MigrationStore`] (a cloneable, type-erased
/// handle) so the trait remains dyn-compatible.
pub trait Migration: Send + Sync {
    /// The schema version this migration requires as its input.
    fn source_version(&self) -> u32;

    /// The schema version this migration produces.
    fn target_version(&self) -> u32;

    /// Apply the migration to `store`.
    ///
    /// Implementations MUST write the new schema version before returning.
    /// Use [`write_version`] to do this.  Returns a [`MigrationOutput`]
    /// describing the new snapshot and report produced, if any.
    fn up(
        &self,
        store: MigrationStore,
    ) -> Pin<Box<dyn Future<Output = Result<MigrationOutput, MigrationError>> + Send + '_>>;
}

// ── DomainMigration trait ─────────────────────────────────────────────────────

/// A migration step scoped to a specific schema domain.
///
/// This trait supplements [`Migration`] with domain-awareness so that the
/// migration catalog can track per-domain versions independently.
pub trait DomainMigration: Migration {
    /// The schema domain this migration operates on.
    ///
    /// Returns one of `"graph"`, `"core_ir"`, `"acl"`, `"verification"`,
    /// `"runtime"`, or `"artifact"`.
    fn domain(&self) -> &'static str;
}

// ── Version I/O helpers ───────────────────────────────────────────────────────

/// Compute the `ObjectId` for a given schema version number.
///
/// The id is the BLAKE3 hash of the CBOR encoding of `version`. Version 0 has
/// no associated object; its absence is the sentinel for "no version written".
fn version_id(version: u32) -> Result<ObjectId, MigrationError> {
    let codec = CborCodec;
    let bytes = codec.encode(&version).map_err(MigrationError::from)?;
    Ok(ObjectId::from_bytes(&bytes))
}

/// Read the current schema version from `store`.
///
/// Returns `0` if no version record is found.
async fn read_version(store: &MigrationStore, max_version: u32) -> Result<u32, MigrationError> {
    let mut highest: u32 = 0;
    for v in 1..=max_version {
        let id = version_id(v)?;
        if store.exists(id).await? {
            highest = v;
        }
    }
    Ok(highest)
}

/// Write `version` as the schema version in `store`.
///
/// The version object's id is the blake3 hash of the CBOR encoding of `version`.
/// This is idempotent: writing the same version twice is a no-op in a CAS store.
pub async fn write_version(store: &MigrationStore, version: u32) -> Result<(), MigrationError> {
    let codec = CborCodec;
    let bytes = codec.encode(&version).map_err(MigrationError::from)?;
    store.put(RawObject(bytes)).await?;
    Ok(())
}

// ── MigrationCatalog ──────────────────────────────────────────────────────────

/// An ordered registry of [`Migration`] steps.
///
/// Call [`apply`](MigrationCatalog::apply) to advance the store from its
/// current schema version to the highest available target.
pub struct MigrationCatalog {
    migrations: Vec<Box<dyn Migration>>,
}

impl MigrationCatalog {
    /// Create an empty catalog.
    pub fn new() -> Self {
        MigrationCatalog {
            migrations: Vec::new(),
        }
    }

    /// Register a migration step.
    ///
    /// Migrations should be registered in ascending source-version order.
    pub fn register(&mut self, migration: impl Migration + 'static) {
        self.migrations.push(Box::new(migration));
    }

    fn max_target(&self) -> u32 {
        self.migrations
            .iter()
            .map(|m| m.target_version())
            .max()
            .unwrap_or(0)
    }

    fn make_store<S: ObjectStore + Send + Sync + 'static>(store: Arc<S>) -> MigrationStore {
        MigrationStore::new(store)
    }

    /// Return the current schema version of `store`.
    ///
    /// Returns `0` if no version record is found.
    pub async fn current_version<S: ObjectStore + Send + Sync + 'static>(
        &self,
        store: Arc<S>,
    ) -> Result<u32, MigrationError> {
        let ms = Self::make_store(store);
        read_version(&ms, self.max_target()).await
    }

    /// Apply all pending migrations to advance `store` to the latest schema version.
    ///
    /// Returns the new schema version on success.
    ///
    /// # Errors
    ///
    /// - [`MigrationError::AlreadyAtVersion`] — store is already at the latest version.
    /// - [`MigrationError::StorageError`] — a storage operation failed.
    pub async fn apply<S: ObjectStore + Send + Sync + 'static>(
        &self,
        store: Arc<S>,
    ) -> Result<u32, MigrationError> {
        let ms = Self::make_store(store);
        let max = self.max_target();

        if self.migrations.is_empty() {
            return Err(MigrationError::AlreadyAtVersion(0));
        }

        let current = read_version(&ms, max).await?;

        if current >= max {
            return Err(MigrationError::AlreadyAtVersion(current));
        }

        let mut version = current;
        for migration in &self.migrations {
            if migration.source_version() != version {
                continue;
            }
            migration.up(ms.clone()).await?;
            version = migration.target_version();
        }

        Ok(version)
    }

    /// Apply all pending migrations and return each step's [`MigrationOutput`].
    ///
    /// Unlike [`apply`](Self::apply) this preserves the full output (new
    /// snapshot + report) from every migration step executed.
    pub async fn apply_with_output<S: ObjectStore + Send + Sync + 'static>(
        &self,
        store: Arc<S>,
    ) -> Result<(u32, Vec<MigrationOutput>), MigrationError> {
        let ms = Self::make_store(store);
        let max = self.max_target();

        if self.migrations.is_empty() {
            return Err(MigrationError::AlreadyAtVersion(0));
        }

        let current = read_version(&ms, max).await?;
        if current >= max {
            return Err(MigrationError::AlreadyAtVersion(current));
        }

        let mut version = current;
        let mut outputs = Vec::new();
        for migration in &self.migrations {
            if migration.source_version() != version {
                continue;
            }
            let output = migration.up(ms.clone()).await?;
            outputs.push(output);
            version = migration.target_version();
        }

        Ok((version, outputs))
    }
}

impl Default for MigrationCatalog {
    fn default() -> Self {
        Self::new()
    }
}

// ── V0ToV1Migration ───────────────────────────────────────────────────────────

/// Structural no-op migration: advances store from schema version 0 to 1.
///
/// No data is transformed. The migration only writes the version 1 record,
/// establishing the baseline for all subsequent migrations.
pub struct V0ToV1Migration;

impl Migration for V0ToV1Migration {
    fn source_version(&self) -> u32 {
        0
    }

    fn target_version(&self) -> u32 {
        1
    }

    fn up(
        &self,
        store: MigrationStore,
    ) -> Pin<Box<dyn Future<Output = Result<MigrationOutput, MigrationError>> + Send + '_>> {
        Box::pin(async move {
            write_version(&store, 1).await?;
            Ok(MigrationOutput {
                new_snapshot: None,
                report: Some(MigrationReport {
                    description: "Establish baseline schema version 1".to_owned(),
                    structural_equivalence: true,
                    preserved_semantics: vec!["All existing graph objects preserved".to_owned()],
                    pre_snapshot_id: None,
                    post_snapshot_id: None,
                }),
            })
        })
    }
}

// ── V1ToV2Migration ───────────────────────────────────────────────────────────

/// Structural no-op migration: advances store from schema version 1 to 2.
///
/// Writes the version 2 record, establishing baseline for subsequent migrations.
pub struct V1ToV2Migration;

impl Migration for V1ToV2Migration {
    fn source_version(&self) -> u32 {
        1
    }

    fn target_version(&self) -> u32 {
        2
    }

    fn up(
        &self,
        store: MigrationStore,
    ) -> Pin<Box<dyn Future<Output = Result<MigrationOutput, MigrationError>> + Send + '_>> {
        Box::pin(async move {
            write_version(&store, 2).await?;
            Ok(MigrationOutput {
                new_snapshot: None,
                report: Some(MigrationReport {
                    description: "Advance schema to version 2".to_owned(),
                    structural_equivalence: true,
                    preserved_semantics: vec![
                        "All function signatures preserved".to_owned(),
                        "All capability grants preserved".to_owned(),
                    ],
                    pre_snapshot_id: None,
                    post_snapshot_id: None,
                }),
            })
        })
    }
}

// ── V2ToV3Migration ───────────────────────────────────────────────────────────

/// Structural no-op migration: advances store from schema version 2 to 3.
///
/// Writes the version 3 record.
pub struct V2ToV3Migration;

impl Migration for V2ToV3Migration {
    fn source_version(&self) -> u32 {
        2
    }

    fn target_version(&self) -> u32 {
        3
    }

    fn up(
        &self,
        store: MigrationStore,
    ) -> Pin<Box<dyn Future<Output = Result<MigrationOutput, MigrationError>> + Send + '_>> {
        Box::pin(async move {
            write_version(&store, 3).await?;
            Ok(MigrationOutput {
                new_snapshot: None,
                report: Some(MigrationReport {
                    description: "Advance schema to version 3".to_owned(),
                    structural_equivalence: true,
                    preserved_semantics: vec![
                        "All graph nodes and edges preserved".to_owned(),
                        "All verification reports preserved".to_owned(),
                        "All ACL entries preserved".to_owned(),
                    ],
                    pre_snapshot_id: None,
                    post_snapshot_id: None,
                }),
            })
        })
    }
}

// ── default_catalog ───────────────────────────────────────────────────────────

/// Return a [`MigrationCatalog`] pre-loaded with all built-in migrations.
///
/// Includes: [`V0ToV1Migration`], [`V1ToV2Migration`], [`V2ToV3Migration`].
pub fn default_catalog() -> MigrationCatalog {
    let mut catalog = MigrationCatalog::new();
    catalog.register(V0ToV1Migration);
    catalog.register(V1ToV2Migration);
    catalog.register(V2ToV3Migration);
    catalog
}
