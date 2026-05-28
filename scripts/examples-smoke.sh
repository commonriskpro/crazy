#!/usr/bin/env bash
# examples-smoke.sh - Run public CLI examples in fresh temporary projects.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

passes=()
failures=()

usage() {
    cat >&2 <<'USAGE'
Usage:
  ./scripts/examples-smoke.sh
  AIL_CARGO=1 ./scripts/examples-smoke.sh
  AIL=/path/to/ail ./scripts/examples-smoke.sh

Runs examples/*.acl through the public AIL CLI in fresh temporary projects.
USAGE
}

record_pass() {
    passes+=("$1")
    printf 'PASS %s\n' "$1"
}

record_fail() {
    failures+=("$1")
    printf 'FAIL %s\n' "$1"
}

contains() {
    local haystack="$1"
    local needle="$2"
    [[ "$haystack" == *"$needle"* ]]
}

expect_contains() {
    local label="$1"
    local output="$2"
    local expected="$3"

    if contains "$output" "$expected"; then
        record_pass "$label"
    else
        record_fail "$label: expected output to contain '$expected'; got: $output"
    fi
}

json_change_id() {
    sed -n 's/.*"change_id":"\([^"]*\)".*/\1/p' | head -n 1
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
fi

if [[ $# -gt 0 ]]; then
    printf 'error: unknown argument: %s\n' "$1" >&2
    usage
    exit 1
fi

if [[ ! -f "$ROOT_DIR/Cargo.toml" || ! -d "$ROOT_DIR/crates/ail-cli" ]]; then
    printf 'error: run from the AIL workspace checkout\n' >&2
    exit 1
fi

if [[ "${AIL_CARGO:-0}" == "1" ]]; then
    AIL_CMD=(cargo run --quiet --manifest-path "$ROOT_DIR/Cargo.toml" -p ail-cli --)
elif [[ -n "${AIL:-}" ]]; then
    AIL_CMD=("$AIL")
elif [[ -x "$ROOT_DIR/target/debug/ail" ]]; then
    AIL_CMD=("$ROOT_DIR/target/debug/ail")
else
    AIL_CMD=(cargo run --quiet --manifest-path "$ROOT_DIR/Cargo.toml" -p ail-cli --)
fi

run_in() {
    local dir="$1"
    shift
    (cd "$dir" && "${AIL_CMD[@]}" "$@")
}

capture_in() {
    local __var="$1"
    local dir="$2"
    shift 2
    local captured

    if captured="$(run_in "$dir" "$@" 2>&1)"; then
        printf -v "$__var" '%s' "$captured"
        return 0
    fi

    printf -v "$__var" '%s' "$captured"
    return 1
}

run_example() {
    local label="$1"
    local acl_file="$2"
    local target="$3"
    local expected="$4"
    shift 4

    local workspace
    workspace="$(mktemp -d "${TMPDIR:-/tmp}/ail-example-$label.XXXXXX")"
    if [[ "${KEEP_EXAMPLE_WORKSPACE:-0}" != "1" ]]; then
        trap 'rm -rf "$workspace"' RETURN
    fi

    printf '\nExample: %s\n' "$label"
    printf 'Workspace: %s\n' "$workspace"

    local output=""
    if capture_in output "$workspace" --json init; then
        expect_contains "$label init" "$output" '"initialized":true'
    else
        record_fail "$label init failed: $output"
        return
    fi

    local change_id=""
    if capture_in output "$workspace" --json change --file "$acl_file"; then
        change_id="$(printf '%s\n' "$output" | json_change_id)"
        if [[ -n "$change_id" ]]; then
            record_pass "$label change creates draft $change_id"
        else
            record_fail "$label change did not emit change_id: $output"
            return
        fi
    else
        record_fail "$label change failed: $output"
        return
    fi

    if capture_in output "$workspace" --json verify "$change_id"; then
        expect_contains "$label verify" "$output" '"next_action":"apply"'
    else
        record_fail "$label verify failed: $output"
        return
    fi

    if capture_in output "$workspace" --json apply "$change_id" --yes; then
        expect_contains "$label apply" "$output" '"next_action":"complete"'
    else
        record_fail "$label apply failed: $output"
        return
    fi

    if capture_in output "$workspace" --json compile --profile dev --target wasm; then
        expect_contains "$label compile" "$output" '"target":"wasm"'
    else
        record_fail "$label compile failed: $output"
        return
    fi

    if capture_in output "$workspace" --json run --profile dev --target wasm "$@" "$target"; then
        expect_contains "$label run" "$output" "$expected"
    else
        record_fail "$label run failed: $output"
    fi
}

printf 'AIL examples smoke command: %s\n' "${AIL_CMD[*]}"

run_example \
    "text-hello" \
    "$ROOT_DIR/examples/text-hello.acl" \
    "fn.hello" \
    '"invoke_result":"result: Hello, world!"'

run_example \
    "print-log-write" \
    "$ROOT_DIR/examples/print-log-write.acl" \
    "fn.print_hello" \
    '"output":["Hello, world!"]' \
    --grant log.write

printf '\nSummary:\n'
printf '  passes: %d\n' "${#passes[@]}"
printf '  failures: %d\n' "${#failures[@]}"

if [[ "${#failures[@]}" -gt 0 ]]; then
    printf 'Result: FAIL\n'
    exit 1
fi

printf 'Result: PASS\n'
