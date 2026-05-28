#!/usr/bin/env bash
# Static smoke checks for the validation-stage performance documentation.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DOC="$ROOT_DIR/docs/performance.md"
PERF_PREFLIGHT="$ROOT_DIR/scripts/perf-preflight.sh"
COMPILER_TEST="$ROOT_DIR/crates/ail-compiler/tests/incremental_tests.rs"
COMPILER_BENCH="$ROOT_DIR/crates/ail-compiler/benches/large_graph.rs"
STORAGE_BENCH="$ROOT_DIR/crates/ail-storage/benches/storage_perf.rs"
TESTKIT="$ROOT_DIR/crates/ail-testkit/src/lib.rs"
MATURITY="$ROOT_DIR/docs/maturity-model.md"

require_literal() {
  local file="$1"
  local literal="$2"
  local label="$3"

  if ! grep -qF "$literal" "$file"; then
    printf 'missing %s in %s: %s\n' "$label" "$file" "$literal" >&2
    return 1
  fi
}

require_literal "$DOC" "<!-- Status: Implemented subset." "implemented-subset status"
require_literal "$DOC" "not a production performance guarantee" "production caveat"
require_literal "$DOC" "deterministic regression evidence" "deterministic evidence framing"
require_literal "$DOC" "cargo test -p ail-compiler incremental_performance_regression" "compiler regression command"
require_literal "$DOC" "cargo bench -p ail-compiler --bench large_graph -- --test" "compiler bench smoke command"
require_literal "$DOC" "cargo bench -p ail-storage --no-run" "storage bench smoke command"
require_literal "$DOC" "large_graph_compiler/incremental_compile_1000_nodes_one_change" "compiler benchmark fixture"
require_literal "$DOC" "storage_perf/cas_put" "storage benchmark fixture"
require_literal "$DOC" "No production performance claim" "claim discipline"

require_literal "$PERF_PREFLIGHT" "cargo test -p ail-compiler incremental_performance_regression" "perf preflight compiler test"
require_literal "$PERF_PREFLIGHT" "cargo bench -p ail-compiler --bench large_graph -- --test" "perf preflight compiler bench smoke"
require_literal "$COMPILER_TEST" "large_graph_incremental_performance_regression_one_change_uses_cache_for_clean_suffix" "compiler deterministic perf test"
require_literal "$COMPILER_TEST" "NODE_COUNT: usize = 1_000" "compiler node count threshold"
require_literal "$COMPILER_TEST" "expected_dirty_count = CHANGED_NODE_INDEX + 1" "compiler dirty-count threshold"
require_literal "$COMPILER_TEST" "expected_clean_count = NODE_COUNT - expected_dirty_count" "compiler clean-count threshold"
require_literal "$COMPILER_BENCH" "compute_node_hashes_1000_nodes" "compiler hash benchmark"
require_literal "$COMPILER_BENCH" "full_compile_1000_nodes" "compiler full benchmark"
require_literal "$COMPILER_BENCH" "incremental_compile_1000_nodes_one_change" "compiler incremental benchmark"
require_literal "$STORAGE_BENCH" "Storage performance benchmarks: CAS, GraphStore, GC, compaction, CBOR codec." "storage benchmark scope"
require_literal "$STORAGE_BENCH" "bench_cas_put" "storage cas put benchmark"
require_literal "$STORAGE_BENCH" "bench_snapshot_save" "storage snapshot benchmark"
require_literal "$STORAGE_BENCH" "bench_gc" "storage gc benchmark"
require_literal "$STORAGE_BENCH" "bench_compact" "storage compact benchmark"
require_literal "$TESTKIT" "make_large_graph" "testkit large graph fixture"
require_literal "$MATURITY" "| Performance |" "maturity performance gate"

printf 'docs performance smoke passed\n'
