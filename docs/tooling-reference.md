# Tooling reference

<!-- Status: Implemented subset. This reference documents the current validation-stage CLI and machine-output contracts. It is not a claim that AIL has Rust-level tooling, formatter, LSP, package ecosystem, or production editor workflow yet. -->

AIL's tooling goal is serious-language ergonomics: humans should be able to create, inspect, verify, run, package, and repair projects without reverse-engineering the Semantic Graph. The current CLI proves important slices of that goal, but everyday project tooling is still incomplete.

## Quick path

1. Use [Getting started](getting-started.md) for the smallest validated CLI walkthrough.
2. Use [Troubleshooting](troubleshooting.md) when a command rejects the workflow.
3. Use this reference to understand the implemented command families and machine-output contract.
4. Run `./scripts/docs-tooling-reference-smoke.sh` after changing CLI command shape, JSON output, diagnostics, package commands, remote commands, or tooling docs.

## Current command families

| Family | Commands | Current role | Evidence |
|--------|----------|--------------|----------|
| Project lifecycle | `new`, `init`, `status`, `doctor`, `gc` | Create a project scaffold, initialize `.ail/`, inspect state, run health checks, and clean unreachable file-store objects. | `crates/ail-cli/src/cli.rs`, `project_commands.rs`, `new_creates_project_scaffold_with_starter_source_and_acl`, `diagnostic_commands.rs`, `crates/ail-cli/tests/cli_subcommands.rs`, `crates/ail-cli/tests/cli_g31r2.rs` |
| Formatting | `fmt` | Format current ACL ChangeSet documents through the parser/canonicalizer and validation-stage `.ail` source files through the source frontend; supports stdout, `--check`, `--write`, and `--json`. | `crates/ail-cli/src/fmt_commands.rs`, `fmt_file_json_outputs_canonical_acl`, `fmt_file_json_outputs_canonical_ail_source`, `fmt_write_makes_check_pass`, `fmt_ail_source_write_makes_check_pass` |
| AI-native change loop | `context`, `impact`, `callers`, `effects`, `proofs`, `change`, `verify`, `apply`, `diff` | Support the context → ChangeSet → verification → apply/repair loop. | `workflow_commands.rs`, `context_commands.rs`, `graph_query_commands.rs`, `llm_agent_loop_e2e_with_schema_version` |
| Compile/run | `compile`, `run`, `eval`, `link` | Compile current graph slices to WASM/native artifacts, run WASM profile-gated paths, run minimal `.ail` source files with `run --file`, including typed scalar parameters and runtime args, evaluate inline expressions, and link native objects when runtime symbols are supplied. | `compile_commands.rs`, `run_commands.rs`, `source_commands.rs`, `run_file_executes_ail_source_main_without_acl_authoring`, `run_file_executes_ail_source_function_with_typed_params`, `eval_commands.rs`, `link_commands.rs`, `link_ensure_cli.rs` |
| Testing | `test` | Discover graph test targets or minimal `.ail` source tests and execute them through the same graph → Core IR → ANF → WASM → runtime path as executable code. | `crates/ail-cli/src/test_commands.rs`, `test_command_runs_graph_test_nodes`, `test_file_runs_ail_source_tests_without_acl_authoring`, `create_test` |
| Editor support | `lsp --stdio`, `lsp --diagnose`, `lsp --complete`, `lsp --hover-token`, `lsp --definition-token`, `lsp --references-token` | Provide validation-stage LSP initialization, ACL parser/schema diagnostics, `.ail` source parser diagnostics, completion snippets, hover text, same-file ACL id go-to-definition, and same-file references for editors and smoke checks. | `crates/ail-cli/src/lsp_commands.rs`, `lsp_diagnose_reports_acl_schema_errors`, `lsp_diagnose_reports_ail_source_parse_errors`, `lsp_completion_and_hover_cover_acl_test_authoring`, `lsp_definition_resolves_acl_target_to_id_location`, `lsp_references_find_same_file_acl_identifier_uses`, `diagnostics_for_acl_text`, `diagnostics_for_ail_source_text`, `completion_items`, `hover_for_token`, `definition_for_token`, `references_for_token` |
| Review/governance | `approve`, `reject`, `policy`, `rollback`, `rebase`, `merge`, `refactor` | Keep human approval, policy gates, semantic branch operations, and refactor metadata visible. | `approval_commands.rs`, `policy_commands.rs`, `branch_commands.rs`, `crates/ail-cli/tests/cli_g31r2.rs` |
| Package workflow | `package init/add/install/search/verify/publish/audit/advisory/yank/yanked/explain` | Validate local package trust, compatibility, advisories, yanking, registry records, and the import != grant invariant. | `crates/ail-cli/src/package_commands/dispatch.rs`, `package_cli_baseline.rs`, `package_cli_compat.rs`, `package_cli_registry.rs` |
| Remote/local exchange | `remote submit/push/pull` | Exercise local in-process remote exchange and file-backed bundle movement; not a networked collaboration service yet. | `remote_commands.rs`, `remote_cli.rs` |

## Machine-output contract

Every command accepts global --json (`--json`). Successful JSON output is wrapped by `format_response` as:

```json
{ "status": "ok", "data": { "schema_version": "1" } }
```

Structured JSON errors use `format_error_response` with:

```json
{ "status": "error", "data": { "schema_version": "1" } }
```

This contract is validation-stage but compatibility-sensitive. If a field is used by tests, docs, or automation, classify changes through [Compatibility policy](compatibility.md).

## Current diagnostics and repair surface

| Surface | Implemented behavior | Evidence |
|---------|----------------------|----------|
| Apply gate | `apply` checks prior verification state, profile matching, stale-base state, and repair options before mutating snapshots. | `workflow_commands.rs`, `repair_loop.rs` |
| Native run gap | `run --target native` returns an explicit error instead of pretending native linked execution exists. | `run_native_target_exits_one_with_explicit_error`, [Troubleshooting](troubleshooting.md) |
| Linker help | `link` exposes `--print-runtime-symbols`, `--emit-runtime-stub`, `--ensure-runtime-stub`, and structured error output for missing artifacts. | `link_commands.rs`, `link_ensure_cli.rs` |
| Doctor checks | `doctor` reports graph/index/schema/artifact/runtime-profile/package-advisory/assumption health. | `diagnostic_commands.rs`, `doctor_json_has_overall_and_all_check_names` |
| Package repair data | package install/verify/audit surface compatibility issues, advisories, yanks, signature state, and reproducible-evidence state. | `package_cli_compat.rs`, `package_cli_registry.rs` |

## Current gaps

These are still not serious-language-complete:

- ail fmt now covers ACL ChangeSet documents and the validation-stage `.ail` source subset. It is not yet a complete source formatter for future modules/imports/pattern syntax.
- `.ail` source support is intentionally narrow: `run --file` and `test --file` accept `fn` declarations with typed scalar parameters, block bodies with `let` statements, single-line `if/else` expressions, and simple `test` declarations, then lower them into the graph pipeline. This is a dogfooding slice, not the full language parser yet.
- LSP/editor support is validation-stage: `ail lsp` can initialize, publish ACL parser/schema diagnostics, publish `.ail` source parser diagnostics, return ACL completion snippets, return ACL hover text, resolve same-file ACL id definitions, and find same-file ACL references, but source completions/hover/navigation, workspace indexing, semantic rename, cross-file navigation, and full CLI/LSP diagnostic parity are not complete yet.
- ail test is validation-stage: it runs graph test targets, but it is not yet a full project test ecosystem with fixtures, watch mode, coverage, property-test UX, or package-level test orchestration.
- Package workflows are local/validation-stage, not a deployed registry ecosystem.
- Native execution is not a full runnable end-user path; native linking exists, but `run --target native` is intentionally blocked.
- JSON output is schema-versioned, but not every command family has a published long-term JSON schema.

## Evidence anchors

These exact source symbols are intentionally named so static docs drift checks can catch accidental overclaims:

- CLI dispatch: `--json`, `Available subcommands`, `OutputMode::Json`, `Commands::Fmt`, `Commands::Test`, `Commands::New`, `Commands::Lsp`, `Commands::Package`, `Commands::Remote`.
- JSON output: `JSON_OUTPUT_VERSION`, `format_response`, `format_error_response`, `schema_version`.
- Linker UX: `print_runtime_symbols`, `emit_runtime_stub`, `ensure_runtime_stub`.
- Package UX: `PackageCmd`, `AdvisoryCmd`, `package install does not grant capabilities`.
- Source/tooling: `source_commands.rs`, `parse_ail_source`, `load_source_graph`, `run_file_executes_ail_source_main_without_acl_authoring`, `run_file_executes_ail_source_function_with_typed_params`, `run_file_executes_ail_source_block_with_let_statement`, `run_file_executes_ail_source_if_else_expression`, `test_file_runs_ail_source_tests_without_acl_authoring`.
- Formatter/tests: `fmt_file_json_outputs_canonical_acl`, `fmt_file_json_outputs_canonical_ail_source`, `fmt_write_makes_check_pass`, `fmt_ail_source_write_makes_check_pass`, `cmd_new`, `new_creates_project_scaffold_with_starter_source_and_acl`, `cmd_lsp`, `run_stdio_lsp`, `diagnostics_for_acl_text`, `diagnostics_for_ail_source_text`, `completion_items`, `hover_for_token`, `definition_for_token`, `references_for_token`, `lsp_diagnose_reports_acl_schema_errors`, `lsp_diagnose_reports_ail_source_parse_errors`, `lsp_completion_and_hover_cover_acl_test_authoring`, `lsp_definition_resolves_acl_target_to_id_location`, `lsp_references_find_same_file_acl_identifier_uses`, `cmd_test`, `discover_tests`, `test_value_passed`, `test_command_runs_graph_test_nodes`, `create_test`.
- End-to-end tests: `llm_agent_loop_e2e_with_schema_version`, `doctor_json_has_overall_and_all_check_names`, `run_native_target_exits_one_with_explicit_error`.

## Review checklist

Before claiming tooling maturity, verify:

- [ ] The command has human-readable output and `--json` output where automation needs it.
- [ ] JSON output includes `schema_version` and avoids leaking Rust `Debug` formatting in user-facing contracts.
- [ ] Diagnostics explain the next action instead of only naming an internal failure.
- [ ] Compatibility-sensitive output changes update [Compatibility policy](compatibility.md), [Troubleshooting](troubleshooting.md), and release notes.
- [ ] The claim does not imply general-source formatting, full LSP/editor maturity, full test ecosystem maturity, or production package workflow maturity unless evidence exists.

## Verification

```sh
./scripts/docs-tooling-reference-smoke.sh
```

The smoke is static. It keeps this reference tied to current CLI source and tests without running build-heavy commands.
