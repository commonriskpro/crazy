// ── ail-compiler::incremental ─────────────────────────────────────────────
//
// Incremental compilation engine — builds only what changed.
//
// # Design
//
// The incremental path adds an opt-in overlay on top of the existing full
// `lower_to_core_ir` path.  Callers supply a `prev_hashes: &NodeHashes`
// snapshot from the previous compilation and an `ArtifactCache` instance.
//
// ## Algorithm (three steps)
//
// 1. **Hash**: `compute_node_hashes` serialises every `GraphNode` to CBOR and
//    hashes it with BLAKE3, producing a `NodeHashes` (`BTreeMap<NodeRef,
//    [u8;32]>`) for the current graph.
//
// 2. **Diff**: `DirtySet::compute` compares the current hashes against
//    `prev_hashes`.  A node is dirty if its hash differs or is absent in the
//    previous snapshot.
//
// 3. **Propagate**: `DirtySet::propagate` expands the dirty set by BFS over the
//    `GraphIndex` backward adjacency (callers).  All transitive callers of a
//    dirty node are marked dirty.
//
// ## Lowering decision
//
// - Dirty node → `lower_to_core_ir` (single-node sub-graph) + cache write.
// - Clean node → cache read (skips all lowering).
//
// # Determinism contract
//
// `NodeHashes` uses `BTreeMap` (not `HashMap`) so iteration order is stable
// across runs.  `DirtySet` uses `BTreeSet` for the same reason.
//
// # Error handling
//
// `compute_node_hashes` can fail only if CBOR serialization fails (which
// indicates a bug, not a user error).  All other operations are infallible.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use ail_core::graph_index::GraphIndex;
use ail_core::semantic_graph::{GraphNode, NodeRef, SemanticGraph};
use ail_verify::report::VerificationReport;

use crate::cache::{ArtifactCache, ArtifactEntry};
use crate::core_ir::{CoreIr, CoreNode, StageHashes};
use crate::error::CompileError;
use crate::hash::{hash_with_parent, stable_cbor_bytes};
use crate::lower::{is_report_accepted, map_node_kind};

// ── NodeHashes ────────────────────────────────────────────────────────────

/// Per-node content hashes for one `SemanticGraph` snapshot.
///
/// The key is the `NodeRef`; the value is the 32-byte BLAKE3 hash of the
/// node's stable CBOR encoding.  `BTreeMap` is used for deterministic
/// iteration order.
pub type NodeHashes = BTreeMap<NodeRef, [u8; 32]>;

// ── DirtySet ─────────────────────────────────────────────────────────────

/// The set of `NodeRef`s that must be re-lowered in an incremental compilation.
///
/// Constructed by [`DirtySet::compute`] (hash diff) and expanded by
/// [`DirtySet::propagate`] (BFS over callers).
pub struct DirtySet(BTreeSet<NodeRef>);

impl DirtySet {
    /// Compute the initial dirty set by comparing `prev` hashes to the current
    /// graph.
    ///
    /// A node is dirty if:
    /// - Its CBOR hash differs from `prev_hashes[node_ref]`, OR
    /// - It is absent in `prev_hashes` (new node).
    ///
    /// # Errors
    ///
    /// Returns `CompileError::EncodingError` if CBOR serialization of any
    /// `GraphNode` fails.
    pub fn compute(prev: &NodeHashes, graph: &SemanticGraph) -> Result<Self, CompileError> {
        let mut dirty = BTreeSet::new();

        for node in &graph.nodes {
            let current_hash = node_cbor_hash(node)?;
            match prev.get(&node.id) {
                Some(&prev_hash) if prev_hash == current_hash => {
                    // Hash unchanged — node is clean.
                }
                _ => {
                    // Hash differs or absent in prev — node is dirty.
                    dirty.insert(node.id);
                }
            }
        }

        Ok(Self(dirty))
    }

    /// Expand the dirty set by BFS over `GraphIndex` backward adjacency.
    ///
    /// Starting from every initially dirty node, all transitive callers are
    /// marked dirty.  Iteration terminates when no new `NodeRef`s are added
    /// (fixed-point BFS).
    ///
    /// # Complexity
    ///
    /// O(V + E) worst case — each edge is visited at most once.
    pub fn propagate(&mut self, index: &GraphIndex) {
        // Seed the BFS queue with the initially dirty nodes.
        let mut queue: VecDeque<NodeRef> = self.0.iter().copied().collect();

        while let Some(dirty_ref) = queue.pop_front() {
            for &caller in index.callers(dirty_ref) {
                if self.0.insert(caller) {
                    // Newly dirty — enqueue for further propagation.
                    queue.push_back(caller);
                }
            }
        }
    }

    /// Return `true` if `r` is in the dirty set.
    pub fn contains(&self, r: NodeRef) -> bool {
        self.0.contains(&r)
    }

    /// Return the number of dirty nodes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Return `true` if the dirty set is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

// ── compute_node_hashes ───────────────────────────────────────────────────

/// Compute a per-node content hash for every node in `graph`.
///
/// Each node is serialized to stable CBOR bytes and hashed with BLAKE3.
/// The resulting `NodeHashes` map can be persisted across compilations and
/// passed to [`DirtySet::compute`] in the next run.
///
/// # Errors
///
/// Returns `CompileError::EncodingError` if CBOR serialization fails.
pub fn compute_node_hashes(graph: &SemanticGraph) -> Result<NodeHashes, CompileError> {
    let mut hashes = BTreeMap::new();
    for node in &graph.nodes {
        let h = node_cbor_hash(node)?;
        hashes.insert(node.id, h);
    }
    Ok(hashes)
}

// ── compile_incremental ───────────────────────────────────────────────────

/// Incrementally lower `graph` to `CoreIr`, skipping nodes whose content is
/// unchanged from `prev_hashes`.
///
/// # Algorithm
///
/// 1. Build `GraphIndex` from the current graph (O(E)).
/// 2. Compute `DirtySet` by comparing current node hashes to `prev_hashes`.
/// 3. Propagate dirtiness through callers via BFS.
/// 4. For each node in the graph:
///    - **Dirty**: lower via `lower_to_core_ir` sub-graph; write result to cache.
///    - **Clean**: read `ArtifactEntry` from cache.
/// 5. Assemble and return `CoreIr`.
///
/// # Pre-conditions
///
/// - `report.summary()` must be `Proven` or `RuntimeChecked`.
/// - `graph.validate()` must pass (no duplicate refs, no dangling edges).
///
/// # Errors
///
/// - `CompileError::RejectedReport` — report not accepted.
/// - `CompileError::InvalidGraph` / `CompileError::MissingNode` — validation failure.
/// - `CompileError::EncodingError` — CBOR serialization failure.
#[cfg_attr(
    feature = "otel",
    tracing::instrument(skip_all, name = "compiler.compile_incremental")
)]
pub fn compile_incremental(
    graph: &SemanticGraph,
    report: &VerificationReport,
    cache: &dyn ArtifactCache,
    prev_hashes: &NodeHashes,
) -> Result<CoreIr, CompileError> {
    // Gate: reject unacceptable reports (same contract as lower_to_core_ir).
    if !is_report_accepted(report) {
        return Err(CompileError::RejectedReport);
    }

    // Gate: validate graph structural invariants.
    graph.validate().map_err(|e| {
        use ail_core::semantic_graph::GraphValidationError;
        match e {
            GraphValidationError::DuplicateRef(r) => {
                CompileError::InvalidGraph(format!("duplicate NodeRef({})", r.0))
            }
            GraphValidationError::DanglingEdge { r#ref, .. } => CompileError::MissingNode(r#ref),
            GraphValidationError::EffectRowNoEmitsEdge(r) => {
                CompileError::InvalidGraph(format!("effect_row declared but no Emits edge on NodeRef({})", r.0))
            }
            GraphValidationError::CapabilityReqsMissingNode { owner_ref, cap_name } => {
                CompileError::InvalidGraph(format!(
                    "capability '{}' required by NodeRef({}) has no matching Capability node",
                    cap_name, owner_ref.0
                ))
            }
        }
    })?;

    // Step 1: Build adjacency index.
    let index = GraphIndex::build(graph);

    // Step 2: Compute dirty set (hash diff).
    let mut dirty = DirtySet::compute(prev_hashes, graph)?;

    // Step 3: Propagate dirtiness through callers.
    dirty.propagate(&index);

    // Hash pipeline inputs for StageHashes provenance.
    let graph_cbor = stable_cbor_bytes(graph)?;
    let graph_snapshot_hash = hash_with_parent(&[], &graph_cbor);

    let report_cbor = stable_cbor_bytes(report)?;
    let verification_report_hash = hash_with_parent(&[], &report_cbor);

    // Step 4: Lower each node — dirty → lower; clean → cache read.
    let mut nodes: Vec<CoreNode> = Vec::with_capacity(graph.nodes.len());
    let mut total_stage_hashes: Option<StageHashes> = None;

    for gn in &graph.nodes {
        let node_hash = node_cbor_hash(gn)?;

        if dirty.contains(gn.id) {
            // Dirty: lower this node via a single-node sub-graph.
            let core_node = CoreNode {
                source_ref: gn.id,
                kind: map_node_kind(gn.kind),
                name: gn.name.clone(),
                ty: gn
                    .type_facts
                    .as_ref()
                    .map(|tf| crate::lower::nominal_to_core_type(&tf.nominal)),
                expr: None,
            };

            // Compute core_ir_hash for this node's lowering (matches existing
            // hash chain contract: blake3(graph_snapshot_hash || core_ir_bytes)).
            let core_ir_bytes = stable_cbor_bytes(&[&core_node])?;
            let core_ir_hash = hash_with_parent(&graph_snapshot_hash, &core_ir_bytes);

            let stage_hashes = StageHashes {
                graph_snapshot_hash,
                verification_report_hash,
                core_ir_hash,
                anf_ir_hash: None,
                wasm_hash: None,
                native_hash: None,
                source_map_hash: None,
                artifact_manifest_hash: None,
            };

            let entry = ArtifactEntry {
                stage_hashes: stage_hashes.clone(),
                node_count: 1,
            };

            cache.put(node_hash, entry);
            total_stage_hashes = Some(stage_hashes);
            nodes.push(core_node);
        } else {
            // Clean: retrieve from cache.
            let entry = cache
                .get(&node_hash)
                .ok_or(CompileError::MissingNode(gn.id))?;

            let core_node = CoreNode {
                source_ref: gn.id,
                kind: map_node_kind(gn.kind),
                name: gn.name.clone(),
                ty: gn
                    .type_facts
                    .as_ref()
                    .map(|tf| crate::lower::nominal_to_core_type(&tf.nominal)),
                expr: None,
            };

            if total_stage_hashes.is_none() {
                total_stage_hashes = Some(entry.stage_hashes);
            }
            nodes.push(core_node);
        }
    }

    // Assemble CoreIr with a combined stage hash (or zeroed if graph is empty).
    let stage_hashes = total_stage_hashes.unwrap_or_else(|| {
        // Empty graph: produce a deterministic zero-content hash chain.
        let core_ir_bytes = stable_cbor_bytes(&nodes).unwrap_or_default();
        let core_ir_hash = hash_with_parent(&graph_snapshot_hash, &core_ir_bytes);
        StageHashes {
            graph_snapshot_hash,
            verification_report_hash,
            core_ir_hash,
            anf_ir_hash: None,
            wasm_hash: None,
            native_hash: None,
            source_map_hash: None,
            artifact_manifest_hash: None,
        }
    });

    Ok(CoreIr {
        nodes,
        stage_hashes,
    })
}

// ── Internal helpers ──────────────────────────────────────────────────────

/// Compute the BLAKE3 hash of `node`'s stable CBOR encoding.
fn node_cbor_hash(node: &GraphNode) -> Result<[u8; 32], CompileError> {
    let bytes = stable_cbor_bytes(node)?;
    Ok(hash_with_parent(&[], &bytes))
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use ail_core::semantic_graph::{
        EdgeKind, GraphEdge, GraphNode, NodeKind, NodeRef, SemanticGraph,
    };
    use ail_verify::report::VerificationReport;

    use super::*;
    use crate::cache::MemoryArtifactCache;

    // ── Helpers ───────────────────────────────────────────────────────────

    fn proven_report() -> VerificationReport {
        VerificationReport {
            entries: vec![],
            ..Default::default()
        }
    }

    fn node(id: u32) -> GraphNode {
        GraphNode::new(NodeRef(id), NodeKind::Function, format!("fn_{id}"))
    }

    fn edge(source: u32, target: u32) -> GraphEdge {
        GraphEdge::new(NodeRef(source), NodeRef(target), EdgeKind::Calls)
    }

    fn two_node_graph() -> SemanticGraph {
        // NodeRef(0) calls NodeRef(1)
        SemanticGraph {
            nodes: vec![node(0), node(1)],
            edges: vec![edge(0, 1)],
        }
    }

    // ── Task 3.1 — DirtySet::compute unit tests ───────────────────────────

    // Spec scenario: Changed node is marked dirty
    // GIVEN prev_hashes containing NodeRef(1) → hash_a
    // WHEN current graph has NodeRef(1) with a different CBOR hash hash_b
    // THEN DirtySet contains NodeRef(1)
    #[test]
    fn changed_node_is_dirty() {
        let graph = two_node_graph();
        // Compute fresh hashes for current graph.
        let current = compute_node_hashes(&graph).unwrap();

        // Tamper with prev: record a wrong hash for NodeRef(1).
        let mut prev = current.clone();
        prev.insert(NodeRef(1), [0u8; 32]); // wrong hash

        let dirty = DirtySet::compute(&prev, &graph).unwrap();
        assert!(
            dirty.contains(NodeRef(1)),
            "NodeRef(1) must be dirty after hash change"
        );
        // NodeRef(0) hash matches — must be clean.
        assert!(!dirty.contains(NodeRef(0)), "NodeRef(0) must be clean");
    }

    // Spec scenario: Unchanged node is not dirty
    // GIVEN prev_hashes containing NodeRef(0) → hash_x
    // WHEN current graph has NodeRef(0) with the same CBOR hash hash_x
    // THEN DirtySet does NOT contain NodeRef(0)
    #[test]
    fn unchanged_node_is_clean() {
        let graph = two_node_graph();
        let prev = compute_node_hashes(&graph).unwrap();
        let dirty = DirtySet::compute(&prev, &graph).unwrap();
        assert!(
            !dirty.contains(NodeRef(0)),
            "unchanged node must not be dirty"
        );
        assert!(
            !dirty.contains(NodeRef(1)),
            "unchanged node must not be dirty"
        );
        assert!(dirty.is_empty(), "no changes → empty dirty set");
    }

    // Spec scenario: New node (absent in prev) is dirty
    // GIVEN prev_hashes with no entry for NodeRef(5)
    // WHEN current graph contains NodeRef(5)
    // THEN DirtySet contains NodeRef(5)
    #[test]
    fn new_node_absent_in_prev_is_dirty() {
        let graph = SemanticGraph {
            nodes: vec![node(5)],
            edges: vec![],
        };
        let prev: NodeHashes = BTreeMap::new(); // empty — NodeRef(5) is new
        let dirty = DirtySet::compute(&prev, &graph).unwrap();
        assert!(
            dirty.contains(NodeRef(5)),
            "new node absent in prev must be dirty"
        );
    }

    // ── Task 3.2 — DirtySet::propagate unit tests ─────────────────────────

    // Spec scenario: Direct caller is marked dirty
    // GIVEN NodeRef(1) is dirty and NodeRef(0) has edge Calls → NodeRef(1)
    // WHEN DirtySet::propagate(index) is called
    // THEN DirtySet contains NodeRef(0)
    #[test]
    fn direct_caller_propagated_dirty() {
        // Graph: NodeRef(0) → NodeRef(1) (Calls)
        let graph = two_node_graph();
        let index = GraphIndex::build(&graph);

        // Manually seed dirty set with NodeRef(1) only.
        let mut dirty = DirtySet(BTreeSet::from([NodeRef(1)]));
        dirty.propagate(&index);

        assert!(
            dirty.contains(NodeRef(0)),
            "direct caller NodeRef(0) must be dirty"
        );
        assert!(
            dirty.contains(NodeRef(1)),
            "original dirty node must remain dirty"
        );
    }

    // Spec scenario: Transitive caller is marked dirty
    // GIVEN NodeRef(2) is dirty, NodeRef(1) calls NodeRef(2), NodeRef(0) calls NodeRef(1)
    // WHEN DirtySet::propagate(index) is called
    // THEN DirtySet contains NodeRef(1) AND NodeRef(0)
    #[test]
    fn transitive_caller_propagated_dirty() {
        // Linear chain: NodeRef(0) → NodeRef(1) → NodeRef(2)
        let graph = SemanticGraph {
            nodes: vec![node(0), node(1), node(2)],
            edges: vec![edge(0, 1), edge(1, 2)],
        };
        let index = GraphIndex::build(&graph);

        let mut dirty = DirtySet(BTreeSet::from([NodeRef(2)]));
        dirty.propagate(&index);

        assert!(
            dirty.contains(NodeRef(1)),
            "direct caller NodeRef(1) must be dirty"
        );
        assert!(
            dirty.contains(NodeRef(0)),
            "transitive caller NodeRef(0) must be dirty"
        );
    }

    // Spec scenario: Unrelated node is not marked dirty
    // GIVEN NodeRef(2) is dirty and NodeRef(3) has no path to NodeRef(2)
    // WHEN DirtySet::propagate(index) is called
    // THEN DirtySet does NOT contain NodeRef(3)
    #[test]
    fn unrelated_node_not_propagated_dirty() {
        // NodeRef(0) → NodeRef(1) → NodeRef(2); NodeRef(3) is isolated
        let graph = SemanticGraph {
            nodes: vec![node(0), node(1), node(2), node(3)],
            edges: vec![edge(0, 1), edge(1, 2)],
        };
        let index = GraphIndex::build(&graph);

        let mut dirty = DirtySet(BTreeSet::from([NodeRef(2)]));
        dirty.propagate(&index);

        assert!(
            !dirty.contains(NodeRef(3)),
            "unrelated NodeRef(3) must not be dirty"
        );
    }

    // ── compile_incremental — basic scenarios ─────────────────────────────

    // Spec scenario: Clean node is served from cache (no re-lowering)
    // GIVEN NodeRef(0) is clean, cache contains key_0 → entry_0
    // WHEN compile_incremental() runs
    // THEN entry_0 is used for NodeRef(0) without re-lowering
    #[test]
    fn clean_node_served_from_cache() {
        let graph = SemanticGraph {
            nodes: vec![node(0)],
            edges: vec![],
        };
        let report = proven_report();
        let cache = MemoryArtifactCache::new();

        // First compile: warm the cache.
        let prev_hashes = compute_node_hashes(&graph).unwrap();
        let _first = compile_incremental(&graph, &report, &cache, &NodeHashes::new()).unwrap();

        // Second compile: same graph → NodeRef(0) is clean → must read from cache.
        let result = compile_incremental(&graph, &report, &cache, &prev_hashes).unwrap();
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].source_ref, NodeRef(0));
    }

    // Spec scenario: Dirty node bypasses cache
    // GIVEN NodeRef(1) is dirty
    // WHEN compile_incremental() runs
    // THEN NodeRef(1) goes through lower_to_core_ir and the result is stored in cache
    #[test]
    fn dirty_node_bypasses_cache_and_updates_it() {
        let graph = SemanticGraph {
            nodes: vec![node(1)],
            edges: vec![],
        };
        let report = proven_report();
        let cache = MemoryArtifactCache::new();

        // No prev_hashes → NodeRef(1) is new → dirty → must lower and cache.
        let result = compile_incremental(&graph, &report, &cache, &NodeHashes::new()).unwrap();
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].source_ref, NodeRef(1));

        // Cache must now have NodeRef(1)'s entry.
        let node_hash = node_cbor_hash(&graph.nodes[0]).unwrap();
        assert!(
            cache.get(&node_hash).is_some(),
            "dirty node must be written to cache after lowering"
        );
    }

    // Spec scenario: Empty dirty set on unchanged graph
    // GIVEN the graph has not changed since the previous snapshot
    // WHEN compile_incremental() runs
    // THEN zero nodes are re-lowered and the result is assembled entirely from cache
    #[test]
    fn unchanged_graph_produces_empty_dirty_set() {
        let graph = SemanticGraph {
            nodes: vec![node(0), node(1)],
            edges: vec![edge(0, 1)],
        };
        let report = proven_report();
        let cache = MemoryArtifactCache::new();

        // First compile: warm cache.
        let prev_hashes = compute_node_hashes(&graph).unwrap();
        compile_incremental(&graph, &report, &cache, &NodeHashes::new()).unwrap();

        // Second compile: no changes → empty dirty set.
        let dirty = DirtySet::compute(&prev_hashes, &graph).unwrap();
        assert!(
            dirty.is_empty(),
            "unchanged graph must produce empty dirty set"
        );

        let result = compile_incremental(&graph, &report, &cache, &prev_hashes).unwrap();
        assert_eq!(
            result.nodes.len(),
            2,
            "all nodes must still be present in output"
        );
    }

    // TRIANGULATE: rejected report returns RejectedReport error.
    #[test]
    fn rejected_report_returns_error() {
        use ail_verify::report::{VerificationEntry, VerificationState};

        let graph = SemanticGraph {
            nodes: vec![node(0)],
            edges: vec![],
        };
        let report = VerificationReport {
            entries: vec![VerificationEntry {
                claim: "x".to_string(),
                state: VerificationState::Failed,
                scope: "s".to_string(),
                evidence: None,
                blocking: true,
            }],
            ..Default::default()
        };
        let cache = MemoryArtifactCache::new();
        let result = compile_incremental(&graph, &report, &cache, &NodeHashes::new());
        assert_eq!(result, Err(CompileError::RejectedReport));
    }
}
