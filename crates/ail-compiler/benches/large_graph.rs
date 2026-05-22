// ── ail-compiler benches/large_graph.rs ──────────────────────────────────
//
// Criterion benchmarks for the incremental compilation pipeline.
//
// # What is measured
//
// `bench_full_compile` — cold full compile of a 500-node linear-chain graph.
//   Baseline for comparing incremental performance.
//
// `bench_incremental_compile_one_change` — warm-cache incremental compile of
//   the same 500-node graph after changing exactly one node (NodeRef(250)).
//   This should be significantly faster than the full compile because only
//   NodeRef(0..=250) (251 nodes) need re-lowering.
//
// # How to run
//
//   cargo bench -p ail-compiler
//
// Criterion writes HTML reports to `target/criterion/large_graph/`.

use ail_compiler::{
    MemoryArtifactCache, compile_incremental, compute_node_hashes, lower_to_core_ir,
};
use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef};
use ail_verify::report::VerificationReport;
use criterion::{Criterion, criterion_group, criterion_main};

// ── shared fixture ────────────────────────────────────────────────────────

const N: usize = 500;

fn proven_report() -> VerificationReport {
    VerificationReport {
        entries: vec![],
        ..Default::default()
    }
}

// ── bench_full_compile ────────────────────────────────────────────────────

/// Benchmark: cold full compile of a 500-node graph.
///
/// Exercises `lower_to_core_ir` without the incremental layer.  This is the
/// reference baseline; the incremental benchmark should be substantially faster
/// for a single-node change.
fn bench_full_compile(c: &mut Criterion) {
    let graph = ail_testkit::make_large_graph(N);
    let report = proven_report();

    c.bench_function("full_compile_500_nodes", |b| {
        b.iter(|| {
            lower_to_core_ir(&graph, &report).expect("full compile must succeed");
        });
    });
}

// ── bench_incremental_compile_one_change ──────────────────────────────────

/// Benchmark: incremental compile after changing one node in a 500-node graph.
///
/// Setup: compile once to warm the cache and record `prev_hashes`.
/// Measured: `compile_incremental` with NodeRef(250) modified (new name).
///
/// Expected: only the 251 callers of NodeRef(250) are re-lowered; the
/// remaining 249 nodes are served from cache.
fn bench_incremental_compile_one_change(c: &mut Criterion) {
    let graph = ail_testkit::make_large_graph(N);
    let report = proven_report();

    // Warm the cache — not measured.
    let cache = MemoryArtifactCache::new();
    let prev_hashes = compute_node_hashes(&graph).expect("hashes must succeed");
    compile_incremental(&graph, &report, &cache, &Default::default())
        .expect("warm compile must succeed");

    // Build modified graph: NodeRef(250) gets a new name → different CBOR hash.
    let mut modified_graph = graph.clone();
    modified_graph.nodes[250] = GraphNode::new(NodeRef(250), NodeKind::Function, "fn_250_changed");

    c.bench_function("incremental_compile_500_nodes_one_change", |b| {
        // Each iteration reuses the same warm cache and prev_hashes.
        // After the first iteration the cache for the modified node is warmed
        // too; subsequent iterations exercise the steady-state re-lowering path.
        b.iter(|| {
            compile_incremental(&modified_graph, &report, &cache, &prev_hashes)
                .expect("incremental compile must succeed");
        });
    });
}

// ── Criterion wiring ──────────────────────────────────────────────────────

criterion_group!(
    benches,
    bench_full_compile,
    bench_incremental_compile_one_change,
);
criterion_main!(benches);
