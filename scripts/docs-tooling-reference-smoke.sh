#!/usr/bin/env bash
# Static smoke checks for validation-stage tooling reference docs.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DOC="$ROOT_DIR/docs/tooling-reference.md"
CLI="$ROOT_DIR/crates/ail-cli/src/cli.rs"
OUTPUT="$ROOT_DIR/crates/ail-cli/src/output.rs"
PROJECT="$ROOT_DIR/crates/ail-cli/src/project_commands.rs"
WORKFLOW="$ROOT_DIR/crates/ail-cli/src/workflow_commands.rs"
DIAGNOSTIC="$ROOT_DIR/crates/ail-cli/src/diagnostic_commands.rs"
LINK="$ROOT_DIR/crates/ail-cli/src/link_commands.rs"
LSP="$ROOT_DIR/crates/ail-cli/src/lsp_commands.rs"
TEST_COMMANDS="$ROOT_DIR/crates/ail-cli/src/test_commands.rs"
SOURCE_COMMANDS="$ROOT_DIR/crates/ail-cli/src/source_commands.rs"
PACKAGE_DISPATCH="$ROOT_DIR/crates/ail-cli/src/package_commands/dispatch.rs"
REMOTE="$ROOT_DIR/crates/ail-cli/src/remote_commands.rs"
CLI_G31="$ROOT_DIR/crates/ail-cli/tests/cli_g31r2.rs"
CLI_SUB="$ROOT_DIR/crates/ail-cli/tests/cli_subcommands.rs"
CLI_BASE="$ROOT_DIR/crates/ail-cli/tests/cli_baseline.rs"
REPAIR="$ROOT_DIR/crates/ail-cli/tests/repair_loop.rs"
LINK_ENSURE="$ROOT_DIR/crates/ail-cli/tests/link_ensure_cli.rs"
PKG_COMPAT="$ROOT_DIR/crates/ail-cli/tests/package_cli_compat.rs"
PKG_REGISTRY="$ROOT_DIR/crates/ail-cli/tests/package_cli_registry.rs"
REMOTE_TEST="$ROOT_DIR/crates/ail-cli/tests/remote_cli.rs"
MATURITY="$ROOT_DIR/docs/maturity-model.md"
COMPAT="$ROOT_DIR/docs/compatibility.md"
TROUBLESHOOTING="$ROOT_DIR/docs/troubleshooting.md"

require_literal() {
  local file="$1"
  local literal="$2"
  local label="$3"

  if ! grep -qF -- "$literal" "$file"; then
    printf 'missing %s in %s: %s\n' "$label" "$file" "$literal" >&2
    return 1
  fi
}

require_literal "$DOC" "<!-- Status: Implemented subset." "implemented-subset status"
require_literal "$DOC" "not a claim that AIL has Rust-level tooling" "production/tooling caveat"
require_literal "$DOC" "Every command accepts global --json" "json contract framing"
require_literal "$DOC" "ail fmt now covers ACL ChangeSet documents and the validation-stage \`.ail\` source subset" "formatter scope"
require_literal "$DOC" "\`.ail\` source support is intentionally narrow" "source language scope"
require_literal "$DOC" "LSP/editor support is validation-stage" "LSP scope"
require_literal "$DOC" "ail test is validation-stage" "test-runner scope"
require_literal "$DOC" "./scripts/docs-tooling-reference-smoke.sh" "verification command"

for command in Context Impact Callers Effects Proofs Change Verify Apply Compile Run Test Link Fmt Eval New Lsp Init Status Inspect Diff Rollback Rebase Merge Refactor Approve Reject Policy Package Remote Doctor Gc; do
  require_literal "$CLI" "${command}" "CLI command $command"
done

for symbol in "--json" "Available subcommands" "OutputMode::Json" "Commands::Fmt" "Commands::Test" "Commands::New" "Commands::Lsp" "Commands::Package" "Commands::Remote"; do
  require_literal "$CLI" "$symbol" "CLI dispatch evidence $symbol"
  require_literal "$DOC" "$symbol" "documented CLI evidence $symbol"
done

for symbol in JSON_OUTPUT_VERSION format_response format_error_response schema_version; do
  require_literal "$OUTPUT" "$symbol" "output contract $symbol"
  require_literal "$DOC" "$symbol" "documented output contract $symbol"
done

for symbol in cmd_init cmd_new cmd_status cmd_change; do
  require_literal "$PROJECT" "$symbol" "project command evidence $symbol"
done

for symbol in cmd_verify cmd_apply rebase_required_repair_option; do
  require_literal "$WORKFLOW" "$symbol" "workflow command evidence $symbol"
done

for symbol in cmd_doctor doctor_index_freshness runtime_profile_validity package_advisories; do
  require_literal "$DIAGNOSTIC" "$symbol" "doctor evidence $symbol"
done

for symbol in print_runtime_symbols emit_runtime_stub ensure_runtime_stub; do
  require_literal "$LINK" "$symbol" "link evidence $symbol"
  require_literal "$DOC" "$symbol" "documented link evidence $symbol"
done

for symbol in cmd_lsp run_stdio_lsp diagnostics_for_acl_text diagnostics_for_ail_source_text completion_items hover_for_token definition_for_token; do
  require_literal "$LSP" "$symbol" "LSP evidence $symbol"
  require_literal "$DOC" "$symbol" "documented LSP evidence $symbol"
done

for symbol in cmd_test discover_tests test_value_passed; do
  require_literal "$TEST_COMMANDS" "$symbol" "test command evidence $symbol"
  require_literal "$DOC" "$symbol" "documented test command evidence $symbol"
done

for symbol in source_commands.rs parse_ail_source load_source_graph; do
  require_literal "$DOC" "$symbol" "documented source evidence $symbol"
done

for symbol in parse_ail_source load_source_graph source_program_to_acl; do
  require_literal "$SOURCE_COMMANDS" "$symbol" "source command evidence $symbol"
done

for symbol in PackageCmd AdvisoryCmd "package install does not grant capabilities"; do
  require_literal "$PACKAGE_DISPATCH" "$symbol" "package CLI evidence $symbol"
  require_literal "$DOC" "$symbol" "documented package CLI evidence $symbol"
done

for symbol in RemoteCmd cmd_remote "local in-process"; do
  require_literal "$REMOTE" "$symbol" "remote CLI evidence $symbol"
done

for symbol in llm_agent_loop_e2e_with_schema_version doctor_json_has_overall_and_all_check_names run_native_target_exits_one_with_explicit_error; do
  require_literal "$CLI_G31" "$symbol" "CLI G31 evidence $symbol"
  require_literal "$DOC" "$symbol" "documented CLI G31 evidence $symbol"
done

for symbol in init_exits_zero_and_creates_ail_dir status_json_output_has_required_fields unknown_subcommand_lists_all_commands_including_new; do
  require_literal "$CLI_SUB" "$symbol" "CLI subcommand evidence $symbol"
done

require_literal "$CLI_BASE" "e2e_change_verify_apply_compile_run" "baseline e2e evidence"
require_literal "$CLI_SUB" "fmt_file_json_outputs_canonical_acl" "fmt CLI JSON evidence"
require_literal "$CLI_SUB" "fmt_file_json_outputs_canonical_ail_source" "fmt source JSON evidence"
require_literal "$CLI_SUB" "fmt_write_makes_check_pass" "fmt write/check evidence"
require_literal "$CLI_SUB" "fmt_ail_source_write_makes_check_pass" "fmt source write/check evidence"
require_literal "$CLI_SUB" "new_creates_project_scaffold_with_starter_source_and_acl" "new project scaffold evidence"
require_literal "$CLI_SUB" "run_file_executes_ail_source_main_without_acl_authoring" "run --file source evidence"
require_literal "$CLI_SUB" "run_file_executes_ail_source_function_with_typed_params" "run --file source params evidence"
require_literal "$CLI_SUB" "run_file_executes_ail_source_block_with_let_statement" "run --file source block evidence"
require_literal "$CLI_SUB" "run_file_executes_ail_source_if_else_expression" "run --file source if evidence"
require_literal "$CLI_SUB" "test_file_runs_ail_source_tests_without_acl_authoring" "test --file source evidence"
require_literal "$CLI_SUB" "lsp_diagnose_reports_acl_schema_errors" "lsp diagnose evidence"
require_literal "$CLI_SUB" "lsp_diagnose_reports_ail_source_parse_errors" "lsp source diagnose evidence"
require_literal "$CLI_SUB" "lsp_completion_and_hover_cover_acl_test_authoring" "lsp completion/hover evidence"
require_literal "$CLI_SUB" "lsp_definition_resolves_acl_target_to_id_location" "lsp definition evidence"
require_literal "$CLI_SUB" "test_command_runs_graph_test_nodes" "test command evidence"
require_literal "$REPAIR" "apply_blocked_without_prior_verify" "repair loop gate evidence"
require_literal "$LINK_ENSURE" "link_help_mentions_ensure_runtime_stub" "link ensure CLI evidence"
require_literal "$PKG_COMPAT" "package_audit_critical_advisory_blocks_and_fails" "package audit evidence"
require_literal "$PKG_REGISTRY" "package_tampered_signature_fails_verify_and_install" "package registry security evidence"
require_literal "$REMOTE_TEST" "remote_push_pull_json_use_local_file_bundle_store" "remote CLI evidence"
require_literal "$MATURITY" "Tooling UX" "maturity tooling gate"
require_literal "$COMPAT" "CLI JSON output" "compatibility CLI JSON surface"
require_literal "$TROUBLESHOOTING" "native linked execution not supported yet" "troubleshooting native gap"

printf 'docs tooling reference smoke passed\n'
