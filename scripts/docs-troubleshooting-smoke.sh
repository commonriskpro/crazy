#!/usr/bin/env bash
# Static smoke checks for the validation-stage troubleshooting guide.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DOC="$ROOT_DIR/docs/troubleshooting.md"
CLI_TEST="$ROOT_DIR/crates/ail-cli/tests/cli_subcommands.rs"
WORKFLOW_SRC="$ROOT_DIR/crates/ail-cli/src/workflow_commands.rs"
RUN_SRC="$ROOT_DIR/crates/ail-cli/src/run_commands.rs"
ERROR_SRC="$ROOT_DIR/crates/ail-cli/src/error.rs"
APPLY_TEST="$ROOT_DIR/crates/ail-cli/src/tests/apply.rs"
REPAIR_TEST="$ROOT_DIR/crates/ail-cli/tests/repair_loop.rs"

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
require_literal "$DOC" "not a production-readiness claim" "production caveat"
require_literal "$DOC" "capability denied: log.write" "capability-denied diagnostic"
require_literal "$DOC" "op grant target=fn.print_hello capability=log.write" "capability repair"
require_literal "$DOC" "no verification report" "missing verification diagnostic"
require_literal "$DOC" "rebase_required" "rebase repair code"
require_literal "$DOC" "rebase required: current snapshot is <id>" "human rebase diagnostic"
require_literal "$DOC" "preflight failed:" "preflight diagnostic"
require_literal "$DOC" "solver 'z3' requires the z3-solver cargo feature" "z3 feature diagnostic"
require_literal "$DOC" "unknown solver '<name>'; supported values: simple, z3" "unknown solver diagnostic"
require_literal "$DOC" "native linked execution not supported yet" "native run diagnostic"
require_literal "$DOC" "not found: change-id not found:" "change-id not-found diagnostic"
require_literal "$DOC" "not found: snapshot not found:" "snapshot not-found diagnostic"
require_literal "$DOC" "not found: capability not found:" "capability not-found diagnostic"

require_literal "$CLI_TEST" "capability denied: log.write" "capability denial assertion"
require_literal "$CLI_TEST" "op grant target=fn.print_hello capability=log.write" "capability grant fixture"
require_literal "$REPAIR_TEST" "no verification report" "missing verification assertion"
require_literal "$APPLY_TEST" "profile mismatch" "profile mismatch scenario"
require_literal "$APPLY_TEST" "preflight failed" "preflight assertion"
require_literal "$WORKFLOW_SRC" '"code": "rebase_required"' "rebase repair JSON"
require_literal "$WORKFLOW_SRC" "solver 'z3' requires the z3-solver cargo feature" "z3 feature source"
require_literal "$WORKFLOW_SRC" "unknown solver '{other}'; supported values: simple, z3" "unknown solver source"
require_literal "$RUN_SRC" "native linked execution not supported yet" "native run source"
require_literal "$ERROR_SRC" "not found: {msg}" "not found display source"

printf 'docs troubleshooting smoke passed\n'
