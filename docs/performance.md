# Performance validation

<!-- Status: Implemented subset. This document describes current deterministic regression evidence, benchmark fixtures, and review policy. It is not a production performance guarantee. -->

Performance validation starts with deterministic regression evidence, not fragile wall-clock gates. CI-safe checks should prove scalable behavior through counts, cache behavior, and benchmark wiring; local benchmarks provide timing evidence for humans.

## Quick path

1. For compiler/cache/index changes, use the deterministic compiler regression test:

   ```bash
   cargo test -p ail-compiler incremental_performance_regression
   ```

2. Smoke the large-graph compiler benchmark without enforcing timing:

   ```bash
   cargo bench -p ail-compiler --bench large_graph -- --test
   ```

3. For storage/CAS/snapshot/GC changes, compile the storage benchmark without running timing loops:

   ```bash
   cargo bench -p ail-storage --no-run
   ```

4. For local timing reports, run the relevant Criterion benchmark:

   ```bash
   cargo bench -p ail-compiler --bench large_graph
   cargo bench -p ail-storage
   ```

Criterion writes reports under `target/criterion/`.

## Current evidence map

| Area | Evidence | Current role |
|---|---|---|
| Compiler incremental behavior | `crates/ail-compiler/tests/incremental_tests.rs` | Deterministic count-based regression gate. |
| Compiler large graph timing | `crates/ail-compiler/benches/large_graph.rs` | Criterion timing fixture for local review. |
| Storage CAS/GraphStore/GC/compaction/CBOR | `crates/ail-storage/benches/storage_perf.rs` | Criterion timing fixture and compile-smoke target. |
| Shared large graph fixture | `crates/ail-testkit/src/lib.rs` | Reusable large graph/project fixture source. |
| Performance preflight | `scripts/perf-preflight.sh` | Build-heavy local/CI opt-in command; not run by lightweight docs/release metadata smokes. |

## Threshold policy

| Environment | Gate | Rationale |
|---|---|---|
| CI default | Deterministic count thresholds only | Stable across machines and load. |
| CI smoke | `cargo bench ... -- --test` or `--no-run` | Confirms benchmark wiring without enforcing timing. |
| Local review | Full Criterion benchmark | Produces timing evidence for humans before changing compiler/cache/index/storage code. |
| Timing gate | Opt-in only | Wall-clock thresholds need controlled baseline, variance window, and documented hardware before they can block CI. |

## Current deterministic thresholds

The `ail-compiler` incremental performance regression test uses a 1,000-node linear `Calls` graph from `ail-testkit::make_large_graph`.

| Scenario | Expected evidence |
|---|---|
| Warm compile | One cache write per node. |
| One change at `NodeRef(500)` | Exactly 501 nodes re-lowered: the changed node plus transitive callers `NodeRef(0)..NodeRef(499)`. |
| Clean suffix | Exactly 499 cache hits for `NodeRef(501)..NodeRef(999)`. |
| Warmed cache | Zero cache misses for clean nodes. |
| Output assembly | Result still contains all 1,000 nodes. |

These thresholds catch accidental regressions from incremental behavior back toward full-graph work while avoiding machine-dependent timing assertions.

## Benchmark fixtures

| Benchmark | Measures | Review use |
|---|---|---|
| `large_graph_compiler/compute_node_hashes_1000_nodes` | Per-node CBOR/BLAKE3 hashing. | Detect hashing/index costs that affect every incremental compile. |
| `large_graph_compiler/full_compile_1000_nodes` | Cold full compile of a 1,000-node graph. | Baseline for comparing incremental compile. |
| `large_graph_compiler/incremental_compile_1000_nodes_one_change` | Warm-cache incremental compile after one mid-graph change. | Evidence that clean suffix work stays cached. |
| `storage_perf/object_id_hash` | BLAKE3/ObjectId hash cost for multiple payload sizes. | CAS identity cost evidence. |
| `storage_perf/cas_put`, `cas_get`, `cas_put_idempotent` | Object-store read/write paths. | Storage hot-path evidence. |
| `storage_perf/cbor_*` | Snapshot encode/decode/roundtrip. | Wire/storage codec evidence. |
| `storage_perf/snapshot_save`, `snapshot_list`, `snapshot_load_by_id` | GraphStore snapshot operations. | Project history scale evidence. |
| `storage_perf/gc_unreferenced`, `compact_50_snapshots` | Retention/GC/compaction paths. | Operational storage evidence. |

## Timing gates

Do not add hard timing thresholds to default CI yet. If a future change needs timing gates, keep them opt-in and document:

- benchmark name and input fixture;
- baseline commit or release;
- hardware/runner profile;
- allowed variance;
- remediation path when the threshold fails.

Until then, use Criterion timing as review evidence and deterministic thresholds as the merge gate.

## Review checklist

Before approving performance-sensitive changes, confirm:

- [ ] The PR names the touched maturity gate as `Performance` or explains why another gate is primary.
- [ ] The PR lists deterministic evidence or explains why only docs/process changed.
- [ ] Compiler/cache/index changes mention the incremental regression test or why it was not run.
- [ ] Storage/CAS/snapshot/GC changes mention the storage benchmark smoke or why it was not run.
- [ ] Any wall-clock claim includes benchmark name, fixture, hardware, baseline, and variance.
- [ ] No production performance claim is made from docs-only or smoke-only evidence.
