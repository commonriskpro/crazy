#!/usr/bin/env bash
# Static smoke checks for the validation-stage onboarding tutorial.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DOC="$ROOT_DIR/docs/getting-started.md"
CLI_TEST="$ROOT_DIR/crates/ail-cli/tests/cli_subcommands.rs"

require_literal() {
  local file="$1"
  local literal="$2"
  local label="$3"

  if ! grep -qF "$literal" "$file"; then
    printf 'missing %s in %s: %s\n' "$label" "$file" "$literal" >&2
    return 1
  fi
}

require_order() {
  python3 - "$DOC" <<'PY'
import sys
from pathlib import Path

doc = Path(sys.argv[1]).read_text(encoding="utf-8")
sequence = [
    "ail-cli -- init",
    "change --file hello-text.acl --json",
    'verify "$CHANGE_ID"',
    'apply "$CHANGE_ID" --yes',
    "compile --profile dev --target wasm",
    "run --profile dev --target wasm fn.hello",
    "change --file print-hello.acl --json",
    'verify "$PRINT_CHANGE_ID"',
    'apply "$PRINT_CHANGE_ID" --yes',
    "run --profile dev --target wasm --grant log.write fn.print_hello",
]

position = -1
for needle in sequence:
    next_position = doc.find(needle, position + 1)
    if next_position == -1:
        raise SystemExit(f"getting-started.md is missing ordered step: {needle}")
    position = next_position
PY
}

require_literal "$DOC" "<!-- Status: Implemented subset." "implemented-subset status"
require_literal "$DOC" "not a production-readiness claim" "production caveat"
require_literal "$DOC" "First \`cargo run\` may build the Rust workspace" "build caveat"
require_literal "$DOC" "\`ail new\` is validation-stage scaffolding" "ail new caveat"
require_literal "$DOC" "fn main() -> Int = add(20, 22)" "source main example"
require_literal "$DOC" "fn add_pair(x: Int, y: Int) -> Int = add(x, y)" "source params example"
require_literal "$DOC" "fn with_local() -> Int {" "source block example"
require_literal "$DOC" "let base = add(20, 20)" "source let example"
require_literal "$DOC" "if gt(base, 40) { add(base, 2) } else { 0 }" "source if example"
require_literal "$DOC" "test main_addition = eq(add_pair(20, 22), 42)" "source test example"
require_literal "$DOC" "run --file main.ail" "source run command"
require_literal "$DOC" "fn.add_pair 20 22" "source run args command"
require_literal "$DOC" "fn.with_local" "source run block command"
require_literal "$DOC" "test --file main.ail" "source test command"
require_literal "$DOC" "op create_function id=fn.hello return=Text body=let(s, \"Hello, world!\", s)" "text hello ACL"
require_literal "$DOC" "op create_capability id=log.write" "capability declaration"
require_literal "$DOC" "op grant target=fn.print_hello capability=log.write" "graph capability grant"
require_literal "$DOC" "capability-denied" "capability denial explanation"
require_literal "$DOC" "result: Hello, world!" "text result expectation"
require_literal "$DOC" "output:" "print output expectation"
require_literal "$DOC" "result: 0" "print result expectation"

require_literal "$CLI_TEST" "fn run_text_return_prints_human_readable_result()" "text-return CLI test"
require_literal "$CLI_TEST" "fn new_creates_project_scaffold_with_starter_source_and_acl()" "new project CLI test"
require_literal "$CLI_TEST" "fn run_file_executes_ail_source_main_without_acl_authoring()" "source run CLI test"
require_literal "$CLI_TEST" "fn run_file_executes_ail_source_function_with_typed_params()" "source params CLI test"
require_literal "$CLI_TEST" "fn run_file_executes_ail_source_block_with_let_statement()" "source block CLI test"
require_literal "$CLI_TEST" "fn run_file_executes_ail_source_if_else_expression()" "source if CLI test"
require_literal "$CLI_TEST" "fn test_file_runs_ail_source_tests_without_acl_authoring()" "source test CLI test"
require_literal "$CLI_TEST" "fn run_print_requires_log_write_grant_and_captures_output()" "capability print CLI test"
require_literal "$CLI_TEST" "op create_test id=test.main_addition" "starter ACL test fixture"
require_literal "$CLI_TEST" "op create_function id=fn.hello return=Text body=let(s, \"Hello, world!\", s)" "text ACL test fixture"
require_literal "$CLI_TEST" "op create_function id=fn.print_hello return=Int body=print(\"Hello, world!\")" "print ACL test fixture"
require_literal "$CLI_TEST" "capability denied: log.write" "capability denial assertion"
require_literal "$CLI_TEST" "result: Hello, world!" "text result assertion"

require_order

printf 'docs onboarding smoke passed\n'
