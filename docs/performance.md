# Performance validation

Performance validation starts with deterministic regression evidence, not fragile wall-clock gates. CI should prove scalable behavior through counts and cache behavior; local benchmarks provide timing evidence for humans.

## Quick path

1. Run the deterministic compiler regression test:

   ```bash
   cargo test -p ail-compiler incremental_performance_regression
   ```

2. Run the Criterion compile smoke:

   ```bash
   cargo bench -p ail-compiler --bench large_graph -- --test
   ```

3. For local timing reports, run the full benchmark:

   ```bash
   cargo bench -p ail-compiler --bench large_graph
   ```

Criterion writes reports under `target/criterion/large_graph/`.

## Threshold Policy

| Environment | Gate | Rationale |
|-------------|------|-----------|
| CI default | Deterministic count thresholds only | Stable across machines and load. |
| CI smoke | `cargo bench ... -- --test` | Confirms the benchmark compiles and executes without enforcing timing. |
| Local review | Full Criterion benchmark | Produces timing evidence for humans before changing compiler/cache/index code. |
| Timing gate | Opt-in only | Wall-clock thresholds need a controlled baseline, variance window, and documented hardware before they can block CI. |

## Current Deterministic Thresholds

The `ail-compiler` incremental performance regression test uses a 1,000-node linear `Calls` graph from `ail-testkit::make_large_graph`.

| Scenario | Expected Evidence |
|----------|-------------------|
| Warm compile | One cache write per node. |
| One change at `NodeRef(500)` | Exactly 501 nodes re-lowered: the changed node plus transitive callers `NodeRef(0)..NodeRef(499)`. |
| Clean suffix | Exactly 499 cache hits for `NodeRef(501)..NodeRef(999)`. |
| Warmed cache | Zero cache misses for clean nodes. |
| Output assembly | Result still contains all 1,000 nodes. |

These thresholds catch accidental regressions from incremental behavior back toward full-graph work while avoiding machine-dependent timing assertions.

## Timing Gates

Do not add hard timing thresholds to default CI yet. If a future change needs timing gates, keep them opt-in and document:

- benchmark name and input fixture,
- baseline commit or release,
- hardware/runner profile,
- allowed variance,
- remediation path when the threshold fails.

Until then, use Criterion timing as review evidence and deterministic thresholds as the merge gate.
