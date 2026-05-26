#!/usr/bin/env bash
# dogfood-conformance.sh - Exercise AIL through the public CLI only.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

passes=()
skips=()
failures=()

usage() {
    cat >&2 <<'USAGE'
Usage:
  ./scripts/dogfood-conformance.sh
  AIL=/path/to/ail ./scripts/dogfood-conformance.sh

Runs a public-CLI dogfood suite in a temporary .ail workspace. The suite uses
ail init, change --stdin, change --file, verify, apply, compile, run, and eval.
Unsupported CLI surfaces are reported as SKIP rather than hidden.
USAGE
}

record_pass() {
    passes+=("$1")
    printf 'PASS %s\n' "$1"
}

record_skip() {
    skips+=("$1")
    printf 'SKIP %s\n' "$1"
}

record_fail() {
    failures+=("$1")
    printf 'FAIL %s\n' "$1"
}

json_change_id() {
    sed -n 's/.*"change_id":"\([^"]*\)".*/\1/p' | head -n 1
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

if [[ -n "${AIL:-}" ]]; then
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

capture_stdin_in() {
    local __var="$1"
    local dir="$2"
    local input_file="$3"
    shift 3
    local captured

    if captured="$(run_in "$dir" "$@" < "$input_file" 2>&1)"; then
        printf -v "$__var" '%s' "$captured"
        return 0
    fi

    printf -v "$__var" '%s' "$captured"
    return 1
}

workspace="$(mktemp -d "${TMPDIR:-/tmp}/ail-dogfood-conformance.XXXXXX")"
if [[ "${KEEP_DOGFOOD_WORKSPACE:-0}" != "1" ]]; then
    trap 'rm -rf "$workspace"' EXIT
fi

printf 'AIL dogfood conformance workspace: %s\n' "$workspace"
printf 'AIL command: %s\n' "${AIL_CMD[*]}"

output=""

if capture_in output "$workspace" --help; then
    expect_contains "cli help lists eval" "$output" "eval"
else
    record_fail "cli help failed: $output"
fi

if capture_in output "$workspace" --json init; then
    expect_contains "ail init creates project" "$output" '"initialized":true'
else
    record_fail "ail init failed: $output"
fi

if capture_in output "$workspace" --json status; then
    expect_contains "ail status reads initialized workspace" "$output" '"status":"ok"'
else
    record_fail "ail status failed: $output"
fi

stdin_acl="$workspace/stdin-smoke.acl"
cat > "$stdin_acl" <<'ACL'
change stdin_smoke
author dogfood
description stdin parser smoke
base 0
op verify
end
ACL

if capture_stdin_in output "$workspace" "$stdin_acl" --json change --stdin; then
    expect_contains "ail change --stdin creates draft" "$output" '"status":"draft"'
else
    record_fail "ail change --stdin failed: $output"
fi

suite_acl="$workspace/dogfood-suite.acl"
cat > "$suite_acl" <<'ACL'
change dogfood_suite
author dogfood
description public CLI language conformance suite
base 0
op create_function id=fn.arithmetic return=Int body=add(mul(x,x),mul(y,y))
op add_param target=fn.arithmetic name=x type=Int
op add_param target=fn.arithmetic name=y type=Int
op create_function id=fn.bool return=Int body=if(and(gt(x,0),not(eq(y,0))),1,0)
op add_param target=fn.bool name=x type=Int
op add_param target=fn.bool name=y type=Int
op create_function id=fn.record return=Int body=field(record(age,30,score,add(10,5)),score)
op create_function id=fn.variant return=Int body=match(some(42),Some(v),v,None,0)
op create_function id=fn.loop return=Int body=loop(break(42))
op create_function id=fn.foreach return=Unit body=foreach(item,list(1,2,3),item)
op create_function id=fn.add_item return=Int body=add(acc,item)
op add_param target=fn.add_item name=acc type=Int
op add_param target=fn.add_item name=item type=Int
op create_function id=fn.fold return=Int body=fold(0,list(1,2,3),add_item)
end
ACL

change_id=""
if capture_in output "$workspace" --json change --file "$suite_acl"; then
    change_id="$(printf '%s\n' "$output" | json_change_id)"
    if [[ -n "$change_id" ]]; then
        record_pass "ail change --file creates canonical ChangeSet $change_id"
    else
        record_fail "ail change --file did not emit change_id: $output"
    fi
else
    record_fail "ail change --file failed: $output"
fi

if [[ -n "$change_id" ]]; then
    if capture_in output "$workspace" --json verify "$change_id"; then
        expect_contains "ail verify reaches public pipeline" "$output" '"next_action":"apply"'
        expect_contains "ail verify reaches Stage 23" "$output" '23-emit-verification-report'
    else
        record_fail "ail verify failed: $output"
    fi

    if capture_in output "$workspace" --json apply "$change_id" --yes; then
        expect_contains "ail apply completes" "$output" '"next_action":"complete"'
    else
        record_fail "ail apply failed: $output"
    fi

    if capture_in output "$workspace" --json compile --profile dev --target wasm; then
        expect_contains "ail compile emits wasm" "$output" '"target":"wasm"'
        expect_contains "ail compile persists artifact" "$output" '"wasm_path"'
    else
        record_fail "ail compile failed: $output"
    fi

    if capture_in output "$workspace" --json run --profile dev --target wasm fn.arithmetic 3 4; then
        expect_contains "ail run arithmetic" "$output" '"invoke_result":"result: 25"'
    else
        record_fail "ail run arithmetic failed: $output"
    fi

    if capture_in output "$workspace" --json run --profile dev --target wasm fn.bool 2 5; then
        expect_contains "ail run bool/comparison/control" "$output" '"invoke_result":"result: 1"'
    else
        record_fail "ail run bool failed: $output"
    fi

    if capture_in output "$workspace" --json run --profile dev --target wasm fn.record; then
        expect_contains "ail run records" "$output" '"invoke_result":"result: 15"'
    else
        record_fail "ail run records failed: $output"
    fi

    if capture_in output "$workspace" --json run --profile dev --target wasm fn.variant; then
        expect_contains "ail run variants/match" "$output" '"invoke_result":"result: 42"'
    else
        record_fail "ail run variants failed: $output"
    fi

    if capture_in output "$workspace" --json run --profile dev --target wasm fn.loop; then
        expect_contains "ail run loop/break" "$output" '"invoke_result":"result: 42"'
    else
        record_fail "ail run loop failed: $output"
    fi

    if capture_in output "$workspace" --json run --profile dev --target wasm fn.foreach; then
        expect_contains "ail run foreach" "$output" '"invoke_result":"result: 0"'
    else
        record_skip "foreach public run unavailable: $output"
    fi

    if capture_in output "$workspace" --json run --profile dev --target wasm fn.fold; then
        expect_contains "ail run fold" "$output" '"invoke_result":"result: 6"'
    else
        record_skip "fold public run unavailable: $output"
    fi
fi

if capture_in output "$workspace" --json eval 'add(20, 22)'; then
    expect_contains "ail eval arithmetic" "$output" '"result":"42"'
else
    record_fail "ail eval arithmetic failed: $output"
fi

if capture_in output "$workspace" --json eval 'mod(43, 5)'; then
    expect_contains "ail eval modulo" "$output" '"result":"3"'
else
    record_fail "ail eval modulo failed: $output"
fi

if capture_in output "$workspace" --json eval 'true'; then
    record_skip "ail eval bool unexpectedly accepted; add an assertion when result contract is defined"
else
    record_skip "ail eval bool literals are not public yet: $output"
fi

if capture_in output "$workspace" --json eval 'field(record(score, 15), score)'; then
    record_skip "ail eval records unexpectedly accepted; add an assertion when result contract is defined"
else
    record_skip "ail eval records are not public yet: $output"
fi

if capture_in output "$workspace" --json eval 'fold(0, list(1, 2, 3), add_item)'; then
    record_skip "ail eval fold unexpectedly accepted; add an assertion when result contract is defined"
else
    record_skip "ail eval fold is not public yet: $output"
fi

printf '\nSummary:\n'
printf '  passes: %d\n' "${#passes[@]}"
printf '  skips: %d\n' "${#skips[@]}"
printf '  failures: %d\n' "${#failures[@]}"

if [[ "${#failures[@]}" -gt 0 ]]; then
    printf 'Result: FAIL\n'
    exit 1
fi

if [[ "${#skips[@]}" -gt 0 ]]; then
    printf 'Result: PARTIAL\n'
    exit 0
fi

printf 'Result: PASS\n'
