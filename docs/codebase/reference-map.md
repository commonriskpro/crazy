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
| Current implementation status | [Implementation blueprint](../implementation-blueprint.md), [Consistency review](../consistency-review.md) | Workspace tests under `crates/*/tests/` |
| Semantic Graph and Core IR | [Core IR](../core-ir.md), [Type system](../type-system.md) | `crates/ail-core/src/semantic_graph.rs`, `crates/ail-compiler/src/core_ir.rs` |
| ChangeSet syntax and apply behavior | [AI Change Language](../change-language.md) | `crates/ail-change/src/parser.rs`, `canonical.rs`, `apply.rs`, `op_schema.rs` |
| Verification states, policy, and reports | [Verification](../verification.md), [Risks](../risks.md) | `crates/ail-verify/src/lib.rs`, `report.rs`, `pipeline.rs`, `policy.rs` |
| Compiler lowering and artifacts | [Compiler](../compiler.md) | `crates/ail-compiler/src/lower.rs`, `anf.rs`, `wasm.rs`, `native.rs`, `artifact_manifest.rs` |
| Runtime capabilities and host behavior | [Runtime](../runtime.md) | `crates/ail-runtime/src/host.rs`, `profile.rs`, `manifest.rs`, `handler.rs`, `schema.rs` |
| Storage, snapshots, migrations, retention | [Storage](../storage.md), [Migration guide](../migration-guide.md) | `crates/ail-storage/src/lib.rs`, `graph.rs`, `object.rs`, `migration.rs`, `backends/` |
| Context slices for LLMs | [Context Server](../context-server.md) | `crates/ail-context/src/lib.rs`, `builder.rs`, `server.rs`, `summary.rs` |
| Packages, trust, signing, advisories | [Packages](../packages.md) | `crates/ail-package/src/lib.rs`, `manifest.rs`, `resolver.rs`, `policy.rs`, `signing.rs` |
| Remote bundles and signed agent exchange | [Remote collaboration](../remote.md) | `crates/ail-remote/src/lib.rs`, `bundle.rs`, `identity.rs`, `policy.rs`, `signing.rs`, `exchange.rs` |
| Multi-agent serialization and rebase | [Coordinator](../coordinator.md) | `crates/ail-coordinator/src/coordinator.rs`, `rebase.rs` |
| Standard library semantic surface | [Standard library](../stdlib.md) | `crates/ail-stdlib/src/lib.rs`, `registry.rs`, `v1.rs`, `exec.rs` |
| CLI workflows and local project layout | [Tooling](../tooling.md) | `crates/ail-cli/src/cli.rs`, `project.rs`, `store.rs` |
| Release and compatibility policy | [Release policy](../release-policy.md), [Migration guide](../migration-guide.md) | `Cargo.toml`, `CHANGELOG.md`, `scripts/tag-release.sh`, `scripts/release-preflight.sh` |
| Known risks and validation gaps | [Risks](../risks.md), [Decisions register](../open-questions.md) | Fuzz targets and subsystem tests |
| Original design conversation | [Historical draft](../history/ai-native-language-draft.md) | Do not treat as implementation evidence |

## Rule of thumb

If a doc section sounds ambitious, check for an `Implementation Status` note and then inspect the crate. The docs intentionally preserve target design while marking implemented subsets.
