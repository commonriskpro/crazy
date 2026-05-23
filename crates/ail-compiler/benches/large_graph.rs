// ── ail-compiler benches/large_graph.rs ──────────────────────────────────
//
// Criterion smoke benchmarks for large-graph compiler behavior.
//
// # What is measured
//
// `compute_node_hashes` — per-node CBOR/BLAKE3 hashing used by every
//   incremental compile.
//
// `lower_to_core_ir` — cold full compile of a 1,000-node linear-chain graph.
//   Baseline for comparing incremental performance.
//
// `compile_incremental` — warm-cache incremental compile of the same graph
//   after changing exactly one mid-graph node.
//   This should be significantly faster than the full compile because only
//   the changed node and its callers need re-lowering.
//
// # How to run
//
//   cargo bench -p ail-compiler --bench large_graph
//
// For a quick CI/local compile smoke without benchmark timing:
//
//   cargo bench -p ail-compiler --bench large_graph -- --test
//
// Criterion writes HTML reports to `target/criterion/large_graph/`.

use std::time::Duration;

use ail_compiler::{
    MemoryArtifactCache, compile_incremental, compute_node_hashes, lower_to_core_ir,
};
use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef};
use ail_verify::report::VerificationReport;
use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};

// ── shared fixture ────────────────────────────────────────────────────────

const NODE_COUNT: usize = 1_000;
const CHANGED_NODE_INDEX: usize = NODE_COUNT / 2;

fn proven_report() -> VerificationReport {
    VerificationReport {
        entries: vec![],
        ..Default::default()
    }
}

// ── bench_large_graph_compiler ─────────────────────────────────────────────

/// Benchmarks the hot paths that make incremental compiler performance viable.
fn bench_large_graph_compiler(c: &mut Criterion) {
    let graph = ail_testkit::make_large_graph(NODE_COUNT);
    let report = proven_report();

    // Warm the cache — not measured.
    let cache = MemoryArtifactCache::new();
    let prev_hashes = compute_node_hashes(&graph).expect("hashes must succeed");
    compile_incremental(&graph, &report, &cache, &Default::default())
        .expect("warm compile must succeed");

    // Build modified graph: one node gets a new name, producing a new CBOR hash.
    let mut modified_graph = graph.clone();
    modified_graph.nodes[CHANGED_NODE_INDEX] = GraphNode::new(
        NodeRef(CHANGED_NODE_INDEX as u32),
        NodeKind::Function,
        format!("fn_{CHANGED_NODE_INDEX}_changed"),
    );

    let mut group = c.benchmark_group("large_graph_compiler");
    group.throughput(Throughput::Elements(NODE_COUNT as u64));

    group.bench_function("compute_node_hashes_1000_nodes", |b| {
        b.iter(|| {
            let hashes = compute_node_hashes(black_box(&graph)).expect("hashes must succeed");
            black_box(hashes);
        });
    });

    group.bench_function("full_compile_1000_nodes", |b| {
        b.iter(|| {
            let core = lower_to_core_ir(black_box(&graph), black_box(&report))
                .expect("full compile must succeed");
            black_box(core);
        });
    });

    group.bench_function("incremental_compile_1000_nodes_one_change", |b| {
        // Each iteration reuses the same warm cache and previous hash snapshot.
        b.iter(|| {
            let core = compile_incremental(
                black_box(&modified_graph),
                black_box(&report),
                &cache,
                black_box(&prev_hashes),
            )
            .expect("incremental compile must succeed");
            black_box(core);
        });
    });

    group.finish();
}

// ── Criterion wiring ──────────────────────────────────────────────────────

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(2));
    targets = bench_large_graph_compiler
}
criterion_main!(benches);
