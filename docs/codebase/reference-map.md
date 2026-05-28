# Reference map

Use this when you know the question and need the right document or crate quickly.

## Status lens

| Lens | Reality |
|------|---------|
| Target design | Full behavior is described across the canonical docs. |
| Implemented subset | Check the linked crate files and tests before treating a design section as implemented. |
| Historical context | Use `docs/history/ai-native-language-draft.md` only when you need original raw design background. |

## If you need...

| Need | Read docs | Inspect code |
|------|-----------|--------------|
| Big-picture architecture | [Architecture](../architecture.md), [Mental model](mental-model.md) | `crates/*/src/lib.rs` |
| First CLI walkthrough and CLI diagnostics | [Getting started](../getting-started.md), [Troubleshooting](../troubleshooting.md), [Tooling reference](../tooling-reference.md), [Tooling](../tooling.md) | `crates/ail-cli/tests/cli_subcommands.rs`, `crates/ail-cli/tests/cli_g31r2.rs`, `crates/ail-cli/src/workflow_commands.rs`, `scripts/docs-onboarding-smoke.sh`, `scripts/docs-troubleshooting-smoke.sh`, `scripts/docs-tooling-reference-smoke.sh`, `scripts/dogfood-conformance.sh` |
| Current implementation status and maturity gates | [Implementation blueprint](../implementation-blueprint.md), [Maturity model](../maturity-model.md), [Consistency review](../consistency-review.md) | Workspace tests under `crates/*/tests/` |
| Contributing or preparing a PR | [Contributing guide](../../CONTRIBUTING.md), [Maintainer playbook](maintainer-playbook.md), [Maturity model](../maturity-model.md) | `.github/PULL_REQUEST_TEMPLATE.md`, `.github/workflows/pr-validation.yml`, `scripts/pr-validation.py`, `scripts/pr-validation-smoke.sh`, `scripts/tag-release-gate-smoke.sh`, `CHANGELOG.md` |
| Semantic Graph and Core IR | [Core IR](../core-ir.md), [Type system](../type-system.md) | `crates/ail-core/src/semantic_graph.rs`, `crates/ail-compiler/src/core_ir.rs` |
| Language surface, ChangeSet syntax, and apply behavior | [Language reference](../language-reference.md), [AI Change Language](../change-language.md) | `crates/ail-change/src/parser.rs`, `parser_tests.rs`, `canonical.rs`, `canonical_ops.rs`, `apply.rs`, `op_schema.rs`, `crates/ail-compiler/src/expr_parser_tests.rs`, `scripts/docs-language-reference-smoke.sh` |
| Verification states, policy, and reports | [Verification](../verification.md), [Risks](../risks.md) | `crates/ail-verify/src/lib.rs`, `report.rs`, `pipeline.rs`, `policy.rs` |
| Compiler lowering and artifacts | [Compiler](../compiler.md) | `crates/ail-compiler/src/lower.rs`, `anf.rs`, `wasm.rs`, `native.rs`, `artifact_manifest.rs` |
| Runtime capabilities and host behavior | [Runtime](../runtime.md) | `crates/ail-runtime/src/host.rs`, `profile.rs`, `manifest.rs`, `handler.rs`, `schema.rs` |
| Storage, snapshots, migrations, retention | [Storage](../storage.md), [Migration guide](../migration-guide.md) | `crates/ail-storage/src/lib.rs`, `graph.rs`, `object.rs`, `migration.rs`, `backends/` |
| Context slices for LLMs | [Context Server](../context-server.md) | `crates/ail-context/src/lib.rs`, `builder.rs`, `server.rs`, `summary.rs` |
| Packages, trust, signing, advisories | [Package reference](../package-reference.md), [Package/trust model](../packages.md) | `crates/ail-package/src/lib.rs`, `manifest.rs`, `trust.rs`, `verification.rs`, `policy.rs`, `resolver.rs`, `lockfile.rs`, `signing.rs`, `registry.rs`, `remote_registry/types.rs`, `versioning.rs`, `scripts/docs-package-reference-smoke.sh` |
| Remote bundles and signed agent exchange | [Remote collaboration](../remote.md) | `crates/ail-remote/src/lib.rs`, `bundle.rs`, `identity.rs`, `policy.rs`, `signing.rs`, `exchange.rs` |
| Multi-agent serialization and rebase | [Coordinator](../coordinator.md) | `crates/ail-coordinator/src/coordinator.rs`, `rebase.rs` |
| Standard library semantic surface | [Stdlib reference](../stdlib-reference.md), [Standard library shape](../stdlib.md) | `crates/ail-stdlib/src/lib.rs`, `registry.rs`, `v1/module_entries.rs`, `v1/function_entries.rs`, `exec/registry.rs`, `capability.rs`, `scripts/docs-stdlib-reference-smoke.sh` |
| CLI workflows and local project layout | [Tooling reference](../tooling-reference.md), [Tooling](../tooling.md) | `crates/ail-cli/src/cli.rs`, `output.rs`, `project_commands.rs`, `workflow_commands.rs`, `diagnostic_commands.rs`, `package_commands/dispatch.rs`, `remote_commands.rs`, `scripts/docs-tooling-reference-smoke.sh` |
| Release, maturity claims, and compatibility policy | [Compatibility policy](../compatibility.md), [Release policy](../release-policy.md), [Maturity model](../maturity-model.md), [Migration guide](../migration-guide.md) | `Cargo.toml`, `CHANGELOG.md`, `crates/ail-change/src/acl_migrator.rs`, `scripts/docs-compatibility-smoke.sh`, `scripts/tag-release.sh`, `scripts/tag-release-gate-smoke.sh`, `scripts/release-preflight.sh`, `scripts/release-metadata-gate-smoke.sh` |
| Security and runtime hardening | [Security and runtime hardening](../security.md), [Runtime](../runtime.md), [Package reference](../package-reference.md), [Context Server](../context-server.md) | `crates/ail-runtime/src/host.rs`, `profile.rs`, `secret.rs`, `crates/ail-runtime/tests/preflight_tests.rs`, `resource_limits_tests.rs`, `handler_trust_tests.rs`, `secret_provider_audit_tests.rs`, `crates/ail-context/src/redaction.rs`, `crates/ail-package/src/signing.rs`, `advisory.rs`, `resolver.rs`, `scripts/docs-security-smoke.sh` |
| Performance validation | [Performance validation](../performance.md), [Implementation blueprint](../implementation-blueprint.md) | `crates/ail-compiler/tests/incremental_tests.rs`, `crates/ail-compiler/benches/large_graph.rs`, `crates/ail-storage/benches/storage_perf.rs`, `scripts/perf-preflight.sh`, `scripts/docs-performance-smoke.sh` |
| Known risks and validation gaps | [Risks](../risks.md), [Decisions register](../open-questions.md) | Fuzz targets and subsystem tests |
| Original design conversation | [Historical draft](../history/ai-native-language-draft.md) | Do not treat as implementation evidence |

## Rule of thumb

If a doc section sounds ambitious, check for an `Implementation Status` note and then inspect the crate. The docs intentionally preserve target design while marking implemented subsets.
