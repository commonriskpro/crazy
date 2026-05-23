#!/usr/bin/env bash
# perf-preflight.sh - CI-safe performance validation checks.

set -euo pipefail

usage() {
    cat >&2 <<'USAGE'
Usage:
  ./scripts/perf-preflight.sh

Runs deterministic performance regression evidence plus the large-graph
Criterion compile smoke. It intentionally does not enforce wall-clock timing.
USAGE
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "error: unknown argument: $1" >&2
            usage
            exit 1
            ;;
    esac
done

if [[ ! -f Cargo.toml || ! -d crates/ail-compiler ]]; then
    echo "error: run perf preflight from the workspace root" >&2
    exit 1
fi

cargo test -p ail-compiler incremental_performance_regression
cargo bench -p ail-compiler --bench large_graph -- --test
