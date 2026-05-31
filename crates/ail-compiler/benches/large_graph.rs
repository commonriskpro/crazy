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

/// Versioned metadata schema for deterministic performance baselines.
///
/// Future CI can compare Criterion output against this fixture without guessing
/// which benchmark names, graph sizes, and change shapes are intended to be
/// stable production-maturity gates.
pub const LARGE_GRAPH_BASELINE_SCHEMA_VERSION: u32 = 1;

/// Deterministic large-project-ish compiler benchmark contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LargeGraphBaseline {
    pub name: &'static str,
    pub criterion_group: &'static str,
    pub hash_benchmark_id: &'static str,
    pub full_compile_benchmark_id: &'static str,
    pub incremental_benchmark_id: &'static str,
    pub node_count: usize,
    pub changed_node_index: usize,
    pub change_count: usize,
    pub max_incremental_to_full_ratio_percent: u8,
}

impl LargeGraphBaseline {
    fn validate(self) -> Result<(), &'static str> {
        if self.name.is_empty() {
            return Err("baseline name must not be empty");
        }
        if self.criterion_group.is_empty()
            || self.hash_benchmark_id.is_empty()
            || self.full_compile_benchmark_id.is_empty()
            || self.incremental_benchmark_id.is_empty()
        {
            return Err("baseline criterion names must not be empty");
        }
        if self.node_count == 0 {
            return Err("baseline node_count must be non-zero");
        }
        if self.changed_node_index >= self.node_count {
            return Err("baseline changed_node_index must be inside node_count");
        }
        if self.change_count == 0 || self.change_count > self.node_count {
            return Err("baseline change_count must be within node_count");
        }
        if !(1..=100).contains(&self.max_incremental_to_full_ratio_percent) {
            return Err("baseline max ratio must be 1..=100 percent");
        }
        Ok(())
    }
}

/// Active baseline used by the benchmark below.
///
/// The ratio is intentionally metadata-only: this bench file records the
/// production expectation, while a future CI gate can enforce it using recorded
/// Criterion measurements on controlled hardware.
pub const LINEAR_CHAIN_1K_ONE_MID_CHANGE: LargeGraphBaseline = LargeGraphBaseline {
    name: "linear_chain_1k_one_mid_change",
    criterion_group: "large_graph_compiler",
    hash_benchmark_id: "compute_node_hashes_1000_nodes",
    full_compile_benchmark_id: "full_compile_1000_nodes",
    incremental_benchmark_id: "incremental_compile_1000_nodes_one_change",
    node_count: 1_000,
    changed_node_index: 500,
    change_count: 1,
    max_incremental_to_full_ratio_percent: 35,
};

pub const LARGE_GRAPH_BASELINES: &[LargeGraphBaseline] = &[LINEAR_CHAIN_1K_ONE_MID_CHANGE];

const ACTIVE_BASELINE: LargeGraphBaseline = LINEAR_CHAIN_1K_ONE_MID_CHANGE;
const NODE_COUNT: usize = ACTIVE_BASELINE.node_count;
const CHANGED_NODE_INDEX: usize = ACTIVE_BASELINE.changed_node_index;

fn proven_report() -> VerificationReport {
    VerificationReport {
        entries: vec![],
        ..Default::default()
    }
}

// ── bench_large_graph_compiler ─────────────────────────────────────────────

/// Benchmarks the hot paths that make incremental compiler performance viable.
fn bench_large_graph_compiler(c: &mut Criterion) {
    ACTIVE_BASELINE
        .validate()
        .expect("large graph baseline metadata must be valid");

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

    let mut group = c.benchmark_group(ACTIVE_BASELINE.criterion_group);
    group.throughput(Throughput::Elements(NODE_COUNT as u64));

    group.bench_function(ACTIVE_BASELINE.hash_benchmark_id, |b| {
        b.iter(|| {
            let hashes = compute_node_hashes(black_box(&graph)).expect("hashes must succeed");
            black_box(hashes);
        });
    });

    group.bench_function(ACTIVE_BASELINE.full_compile_benchmark_id, |b| {
        b.iter(|| {
            let core = lower_to_core_ir(black_box(&graph), black_box(&report))
                .expect("full compile must succeed");
            black_box(core);
        });
    });

    group.bench_function(ACTIVE_BASELINE.incremental_benchmark_id, |b| {
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

// ── metadata validation ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_graph_baseline_metadata_is_valid() {
        assert_eq!(LARGE_GRAPH_BASELINE_SCHEMA_VERSION, 1);
        assert!(!LARGE_GRAPH_BASELINES.is_empty());

        for baseline in LARGE_GRAPH_BASELINES {
            baseline.validate().expect("baseline must be valid");
        }
    }

    #[test]
    fn large_graph_baseline_names_are_unique() {
        for (left_index, left) in LARGE_GRAPH_BASELINES.iter().enumerate() {
            for right in LARGE_GRAPH_BASELINES.iter().skip(left_index + 1) {
                assert_ne!(left.name, right.name, "baseline names must be unique");
            }
        }
    }
}
