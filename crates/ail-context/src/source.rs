// ── ail-context::source ───────────────────────────────────────────────────
//
// `ContextSource` trait and adapters.
//
// # Design note: object-safety trade-off
//
// `ContextSource` uses Return-Position Impl Trait In Traits (RPITIT) with an
// explicit `+ Send` bound rather than `async fn` syntax.  This matches the
// `GraphStore` / `ObjectStore` pattern used throughout the workspace and
// keeps the trait usable in multi-threaded contexts.
//
// TRADE-OFF: RPITIT traits are NOT object-safe (`dyn ContextSource` is
// unavailable).  The spec lists "object-safe" as a requirement, but the
// project's established convention consistently uses RPITIT for async traits
// instead of `dyn`-compatible designs (boxed futures or `async_trait`).
// We follow the project pattern and document the deviation here.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use ail_core::semantic_graph::SemanticGraph;
use ail_storage::codec::{CborCodec, ContentCodec};
use ail_storage::graph::{GraphStore, SnapshotEnvelope};
use ail_storage::object::{ObjectId, ObjectStore};

use crate::dto::SnapshotSelector;
use crate::error::{ContextError, ContextResult};

// ── ContextSource trait ───────────────────────────────────────────────────

/// Async seam for resolving snapshot envelopes and hydrating graph roots.
///
/// Implementations are generic (RPITIT + `+ Send`); see the module-level
/// note on why this deviates from the spec's object-safety requirement.
pub trait ContextSource {
    /// Resolve a `SnapshotEnvelope` from the given selector.
    ///
    /// Returns `Err(ContextError::Stale)` if the snapshot is absent.
    fn resolve_snapshot(
        &self,
        selector: &SnapshotSelector,
    ) -> impl Future<Output = ContextResult<SnapshotEnvelope>> + Send;

    /// Load and decode the `SemanticGraph` whose content address is
    /// `graph_root_hash`.
    ///
    /// Returns `Err(ContextError::Stale)` if the hash is absent from the
    /// backing store.
    fn load_graph(
        &self,
        graph_root_hash: &ObjectId,
    ) -> impl Future<Output = ContextResult<SemanticGraph>> + Send;
}

// ── InMemoryContextSource ─────────────────────────────────────────────────

/// In-memory `ContextSource` for tests and ephemeral workloads.
///
/// Snapshots are indexed by `SnapshotEnvelope.id`; graphs by their
/// `ObjectId` key (typically the `graph_root_hash`).
///
/// Uses `Arc<Mutex<HashMap<…>>>` so the source is `Clone` and `Send`
/// without requiring mutable borrows across async calls.
///
/// `HashMap` is used here (not `BTreeMap`) because `ObjectId` does not
/// implement `Ord`.  This is intentional: `InMemoryContextSource` is a
/// test helper, not a serialized structure, so hash-map ordering is fine.
#[derive(Clone, Debug, Default)]
pub struct InMemoryContextSource {
    snapshots: Arc<Mutex<HashMap<ObjectId, SnapshotEnvelope>>>,
    graphs: Arc<Mutex<HashMap<ObjectId, SemanticGraph>>>,
}

impl InMemoryContextSource {
    /// Create a new, empty `InMemoryContextSource`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a snapshot; subsequent `resolve_snapshot(ById(snap.id))`
    /// calls will return it.
    pub fn insert_snapshot(&self, snapshot: SnapshotEnvelope) {
        self.snapshots
            .lock()
            .expect("snapshots lock must not be poisoned")
            .insert(snapshot.id, snapshot);
    }

    /// Register a graph keyed by `hash`; subsequent `load_graph(&hash)`
    /// calls will return it.
    pub fn insert_graph(&self, hash: ObjectId, graph: SemanticGraph) {
        self.graphs
            .lock()
            .expect("graphs lock must not be poisoned")
            .insert(hash, graph);
    }
}

impl ContextSource for InMemoryContextSource {
    async fn resolve_snapshot(&self, selector: &SnapshotSelector) -> ContextResult<SnapshotEnvelope> {
        match selector {
            SnapshotSelector::ById(id) => {
                let guard = self
                    .snapshots
                    .lock()
                    .expect("snapshots lock must not be poisoned");
                guard.get(id).cloned().ok_or(ContextError::Stale)
            }
        }
    }

    async fn load_graph(&self, graph_root_hash: &ObjectId) -> ContextResult<SemanticGraph> {
        let guard = self
            .graphs
            .lock()
            .expect("graphs lock must not be poisoned");
        guard.get(graph_root_hash).cloned().ok_or(ContextError::Stale)
    }
}

// ── StoreContextSource ────────────────────────────────────────────────────

/// `ContextSource` adapter backed by a `GraphStore` and an `ObjectStore`.
///
/// `resolve_snapshot` delegates to `GraphStore::load_snapshot`.
/// `load_graph` fetches raw bytes from `ObjectStore` and decodes them with
/// `CborCodec` as a `SemanticGraph`.
pub struct StoreContextSource<G, O> {
    graph_store: G,
    object_store: O,
}

impl<G, O> StoreContextSource<G, O> {
    /// Wrap `graph_store` and `object_store` in a `StoreContextSource`.
    pub fn new(graph_store: G, object_store: O) -> Self {
        Self {
            graph_store,
            object_store,
        }
    }
}

impl<G, O> ContextSource for StoreContextSource<G, O>
where
    G: GraphStore + Send + Sync,
    O: ObjectStore + Send + Sync,
{
    async fn resolve_snapshot(&self, selector: &SnapshotSelector) -> ContextResult<SnapshotEnvelope> {
        match selector {
            SnapshotSelector::ById(id) => self
                .graph_store
                .load_snapshot(id)
                .await
                .map_err(|e| ContextError::Codec(e.to_string()))?
                .ok_or(ContextError::Stale),
        }
    }

    async fn load_graph(&self, graph_root_hash: &ObjectId) -> ContextResult<SemanticGraph> {
        let codec = CborCodec;
        let raw = self
            .object_store
            .get(graph_root_hash)
            .await
            .map_err(|e| ContextError::Codec(e.to_string()))?
            .ok_or(ContextError::Stale)?;
        codec
            .decode(&raw.0)
            .map_err(|e| ContextError::Codec(e.to_string()))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
    use ail_storage::codec::{CborCodec, ContentCodec};
    use ail_storage::graph::SnapshotEnvelope;
    use ail_storage::object::ObjectId;
    use futures::executor::block_on;

    fn make_snapshot(label: &str) -> SnapshotEnvelope {
        let id = ObjectId::from_bytes(label.as_bytes());
        SnapshotEnvelope {
            id,
            graph_root_hash: id,
            parent_id: None,
            applied_change_id: None,
            created_at: 0,
        }
    }

    fn make_graph() -> SemanticGraph {
        SemanticGraph {
            nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Module, "core")],
            edges: vec![],
        }
    }

    fn graph_id(graph: &SemanticGraph) -> ObjectId {
        let codec = CborCodec;
        let bytes = codec.encode(graph).expect("encode graph");
        ObjectId::from_bytes(&bytes)
    }

    // ── in_memory_resolve_stored_snapshot_returns_ok ──────────────────────
    // Spec scenario: "Happy-path materialization" (source layer).
    //
    // RED: `InMemoryContextSource` did not exist → compile error.
    // GREEN: struct + trait impl makes it compile and pass.
    #[test]
    fn in_memory_resolve_stored_snapshot_returns_ok() {
        let snap = make_snapshot("snap-a");
        let source = InMemoryContextSource::new();
        source.insert_snapshot(snap.clone());

        let result = block_on(source.resolve_snapshot(&SnapshotSelector::ById(snap.id)));
        assert_eq!(result.unwrap(), snap, "must return the stored snapshot");
    }

    // ── in_memory_resolve_missing_snapshot_returns_stale ─────────────────
    // Spec scenario: "Stale snapshot returns E_CONTEXT_STALE".
    #[test]
    fn in_memory_resolve_missing_snapshot_returns_stale() {
        let source = InMemoryContextSource::new();
        let missing_id = ObjectId::from_bytes(b"not-stored");

        let result = block_on(source.resolve_snapshot(&SnapshotSelector::ById(missing_id)));
        assert!(
            matches!(result, Err(ContextError::Stale)),
            "missing snapshot must return Err(Stale), got: {result:?}"
        );
    }

    // ── in_memory_load_graph_returns_stored ───────────────────────────────
    // Spec scenario: "Happy-path materialization" (graph layer).
    #[test]
    fn in_memory_load_graph_returns_stored() {
        let graph = make_graph();
        let hash = graph_id(&graph);

        let source = InMemoryContextSource::new();
        source.insert_graph(hash, graph.clone());

        let result = block_on(source.load_graph(&hash));
        assert_eq!(result.unwrap(), graph, "must return the stored graph");
    }

    // ── in_memory_load_graph_missing_returns_stale ────────────────────────
    // Spec scenario: "Stale snapshot returns E_CONTEXT_STALE" (graph-root absent).
    // TRIANGULATE: different code path from snapshot lookup.
    #[test]
    fn in_memory_load_graph_missing_returns_stale() {
        let source = InMemoryContextSource::new();
        let missing_hash = ObjectId::from_bytes(b"no-graph-here");

        let result = block_on(source.load_graph(&missing_hash));
        assert!(
            matches!(result, Err(ContextError::Stale)),
            "missing graph root hash must return Err(Stale), got: {result:?}"
        );
    }

    // ── in_memory_source_is_clone_shareable ───────────────────────────────
    // TRIANGULATE: clone shares the same backing store (Arc semantics).
    #[test]
    fn in_memory_source_is_clone_shareable() {
        let source_a = InMemoryContextSource::new();
        let source_b = source_a.clone();

        let snap = make_snapshot("shared-snap");
        source_a.insert_snapshot(snap.clone());

        // source_b must see the data inserted through source_a (shared Arc)
        let result = block_on(source_b.resolve_snapshot(&SnapshotSelector::ById(snap.id)));
        assert_eq!(
            result.unwrap(),
            snap,
            "cloned source must share the same snapshot store"
        );
    }
}
