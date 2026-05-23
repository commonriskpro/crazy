// ── ail-compiler::incremental_tests ──────────────────────────────────────
//
// End-to-end integration tests for `compile_incremental`.
//
// Spec scenarios covered:
//
// 3.3 — Three end-to-end scenarios:
//   - Clean node served from cache (no re-lowering).
//   - Dirty node bypasses cache (re-lowered and stored in cache).
//   - Empty dirty set returns all nodes from cache.
//
// 3.4 — 500-node large-graph integration:
//   - Compile once (warm cache), change NodeRef(250), verify re-lowered
//     count ≤ |{250} ∪ transitive_callers(250)|.

use std::sync::atomic::{AtomicUsize, Ordering};

use ail_compiler::{
    ArtifactCache, ArtifactEntry, MemoryArtifactCache, compile_incremental, compute_node_hashes,
};
use ail_core::semantic_graph::{EdgeKind, GraphEdge, GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_verify::report::VerificationReport;

// ── helpers ───────────────────────────────────────────────────────────────

fn proven_report() -> VerificationReport {
    VerificationReport {
        entries: vec![],
        ..Default::default()
    }
}

fn node(id: u32) -> GraphNode {
    GraphNode::new(NodeRef(id), NodeKind::Function, format!("fn_{id}"))
}

fn calls_edge(source: u32, target: u32) -> GraphEdge {
    GraphEdge::new(NodeRef(source), NodeRef(target), EdgeKind::Calls)
}

#[derive(Default)]
struct CountingCache {
    inner: MemoryArtifactCache,
    hits: AtomicUsize,
    misses: AtomicUsize,
    puts: AtomicUsize,
}

impl CountingCache {
    fn new() -> Self {
        Self::default()
    }

    fn reset_counters(&self) {
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
        self.puts.store(0, Ordering::Relaxed);
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::Relaxed)
    }

    fn misses(&self) -> usize {
        self.misses.load(Ordering::Relaxed)
    }

    fn puts(&self) -> usize {
        self.puts.load(Ordering::Relaxed)
    }
}

impl ArtifactCache for CountingCache {
    fn get(&self, key: &[u8; 32]) -> Option<ArtifactEntry> {
        let entry = self.inner.get(key);
        if entry.is_some() {
            self.hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
        }
        entry
    }

    fn put(&self, key: [u8; 32], entry: ArtifactEntry) {
        self.puts.fetch_add(1, Ordering::Relaxed);
        self.inner.put(key, entry);
    }
}

// ── Task 3.3 — end-to-end compile_incremental scenarios ──────────────────

// Spec scenario: Clean node is served from cache.
//
// GIVEN NodeRef(0) is clean (cache warmed by first compile)
// WHEN compile_incremental() runs with unchanged prev_hashes
// THEN NodeRef(0) appears in output without re-lowering (served from cache)
#[test]
fn clean_node_served_from_cache_end_to_end() {
    let graph = SemanticGraph {
        nodes: vec![node(0), node(1)],
        edges: vec![calls_edge(0, 1)],
    };
    let report = proven_report();
    let cache = MemoryArtifactCache::new();

    // First compile: warms the cache for both nodes.
    let prev_hashes = compute_node_hashes(&graph).unwrap();
    compile_incremental(&graph, &report, &cache, &Default::default()).unwrap();

    // Second compile: graph unchanged → both nodes should be clean.
    let result = compile_incremental(&graph, &report, &cache, &prev_hashes).unwrap();

    assert_eq!(
        result.nodes.len(),
        2,
        "all nodes must appear in output even when served from cache"
    );
    assert_eq!(result.nodes[0].source_ref, NodeRef(0));
    assert_eq!(result.nodes[1].source_ref, NodeRef(1));
}

// Spec scenario: Dirty node bypasses cache.
//
// GIVEN NodeRef(1) is dirty (modified between snapshots)
// WHEN compile_incremental() runs
// THEN NodeRef(1) is re-lowered and the result is stored in cache
#[test]
fn dirty_node_bypasses_cache_end_to_end() {
    // Warm cache with original graph.
    let original_graph = SemanticGraph {
        nodes: vec![node(0), node(1)],
        edges: vec![],
    };
    let report = proven_report();
    let cache = MemoryArtifactCache::new();

    let prev_hashes = compute_node_hashes(&original_graph).unwrap();
    compile_incremental(&original_graph, &report, &cache, &Default::default()).unwrap();

    // Mutate the graph: replace NodeRef(1) with a different node name.
    let modified_graph = SemanticGraph {
        nodes: vec![
            node(0),
            GraphNode::new(NodeRef(1), NodeKind::Function, "fn_1_modified"),
        ],
        edges: vec![],
    };

    // Second compile: NodeRef(1) hash changed → must be re-lowered.
    let result = compile_incremental(&modified_graph, &report, &cache, &prev_hashes).unwrap();
    assert_eq!(result.nodes.len(), 2);
    // Result must still include both nodes.
    assert!(result.nodes.iter().any(|n| n.source_ref == NodeRef(0)));
    assert!(result.nodes.iter().any(|n| n.source_ref == NodeRef(1)));
}

// Spec scenario: Empty dirty set returns all nodes from cache.
//
// GIVEN the graph has not changed between compilations
// WHEN compile_incremental() runs for the second time
// THEN all nodes are assembled from cache (dirty set is empty)
#[test]
fn unchanged_graph_empty_dirty_set_all_from_cache() {
    let graph = SemanticGraph {
        nodes: vec![node(0), node(1), node(2)],
        edges: vec![calls_edge(0, 1), calls_edge(1, 2)],
    };
    let report = proven_report();
    let cache = MemoryArtifactCache::new();

    // First compile: warm cache.
    let prev_hashes = compute_node_hashes(&graph).unwrap();
    compile_incremental(&graph, &report, &cache, &Default::default()).unwrap();

    // Second compile: unchanged graph → empty dirty set → all from cache.
    let result = compile_incremental(&graph, &report, &cache, &prev_hashes).unwrap();
    assert_eq!(
        result.nodes.len(),
        3,
        "all nodes must be present in output assembled from cache"
    );
}

// ── Task 3.4 — 500-node large-graph integration ───────────────────────────

// Spec scenario: 500-node graph — only affected nodes are re-lowered.
//
// GIVEN a 500-node linear-chain graph compiled once (warm cache)
// WHEN NodeRef(250) is modified and compile_incremental() is called
// THEN the number of re-lowered nodes ≤ |{250} ∪ transitive_callers(250)|
//
// In a linear chain NodeRef(0) → NodeRef(1) → … → NodeRef(499):
//   callers of NodeRef(250) = {NodeRef(0), NodeRef(1), …, NodeRef(249)}
//   so re-lowered count = 251 (NodeRef(0)..NodeRef(250) inclusive).
#[test]
fn large_graph_incremental_one_change_recompiles_only_callers() {
    let n: usize = 500;
    let graph = ail_testkit::make_large_graph(n);
    let report = proven_report();
    let cache = MemoryArtifactCache::new();

    // Warm the cache with a full compile (all nodes dirty on first pass).
    let prev_hashes = compute_node_hashes(&graph).unwrap();
    compile_incremental(&graph, &report, &cache, &Default::default()).unwrap();

    // Modify NodeRef(250): change its name to produce a different CBOR hash.
    let mut modified_graph = graph.clone();
    modified_graph.nodes[250] = GraphNode::new(NodeRef(250), NodeKind::Function, "fn_250_modified");

    // Re-compute hashes for modified graph to count dirty nodes.
    let new_hashes = compute_node_hashes(&modified_graph).unwrap();

    // Compute dirty set (diff only, before propagation).
    let mut dirty =
        ail_compiler::incremental::DirtySet::compute(&prev_hashes, &modified_graph).unwrap();

    // Propagate through callers.
    let index = ail_core::graph_index::GraphIndex::build(&modified_graph);
    dirty.propagate(&index);

    // In a linear chain 0→1→…→499, callers of NodeRef(250) are NodeRef(0..249).
    // After propagation: dirty = {0, 1, …, 250}.
    let expected_dirty_count = 251; // NodeRef(0) through NodeRef(250) inclusive

    assert_eq!(
        dirty.len(),
        expected_dirty_count,
        "re-lowered count must equal |{{250}} ∪ transitive_callers(250)| = {expected_dirty_count}"
    );

    // The actual compile must succeed and return all 500 nodes.
    let _ = new_hashes; // silence unused warning — used for docs above
    let result = compile_incremental(&modified_graph, &report, &cache, &prev_hashes).unwrap();
    assert_eq!(result.nodes.len(), n, "output must contain all {n} nodes");
}

// ── Performance regression evidence — deterministic thresholds ────────────

// Spec scenario: 1,000-node graph — clean suffix is served from cache.
//
// GIVEN a warmed 1,000-node linear-chain graph
// WHEN NodeRef(500) changes and dirtiness propagates to transitive callers
// THEN exactly 501 nodes are re-lowered and the remaining 499 clean nodes are
//      cache hits with no cache misses.
//
// This is intentionally count-based, not wall-clock based. It turns the
// Criterion smoke benchmark into CI-safe regression evidence for scalable
// incremental behavior without binding tests to local machine timing.
#[test]
fn large_graph_incremental_performance_regression_one_change_uses_cache_for_clean_suffix() {
    const NODE_COUNT: usize = 1_000;
    const CHANGED_NODE_INDEX: usize = NODE_COUNT / 2;

    let graph = ail_testkit::make_large_graph(NODE_COUNT);
    let report = proven_report();
    let cache = CountingCache::new();

    let prev_hashes = compute_node_hashes(&graph).unwrap();
    compile_incremental(&graph, &report, &cache, &Default::default()).unwrap();
    assert_eq!(
        cache.puts(),
        NODE_COUNT,
        "warm compile should populate one artifact per node"
    );

    let mut modified_graph = graph.clone();
    modified_graph.nodes[CHANGED_NODE_INDEX] = GraphNode::new(
        NodeRef(CHANGED_NODE_INDEX as u32),
        NodeKind::Function,
        format!("fn_{CHANGED_NODE_INDEX}_modified"),
    );

    cache.reset_counters();
    let result = compile_incremental(&modified_graph, &report, &cache, &prev_hashes).unwrap();

    let expected_dirty_count = CHANGED_NODE_INDEX + 1;
    let expected_clean_count = NODE_COUNT - expected_dirty_count;

    assert_eq!(
        result.nodes.len(),
        NODE_COUNT,
        "incremental compile must still assemble the complete graph"
    );
    assert_eq!(
        cache.puts(),
        expected_dirty_count,
        "only changed node plus transitive callers should be re-lowered"
    );
    assert_eq!(
        cache.hits(),
        expected_clean_count,
        "clean suffix should be served from warmed cache"
    );
    assert_eq!(
        cache.misses(),
        0,
        "a warmed cache should not miss for clean nodes"
    );
}
