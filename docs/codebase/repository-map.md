# Repository map

This map ties directories and crates to responsibilities. It is based on the current workspace layout and crate module docs.

## Status lens

| Lens | Reality |
|------|---------|
| Target design | Separate subsystems for semantic graph, ChangeSets, verification, compiler, runtime, storage, context, packages, coordination, and tooling. |
| Implemented subset | The workspace contains 14 default Rust crates plus fuzz targets and docs. Some crates expose milestone APIs that are narrower than the full design docs. |
| Historical context | Earlier design material was consolidated into `docs/`; root-level draft material now lives under `docs/history/`. |

## Root directories

| Path | Responsibility |
|------|----------------|
| `crates/` | Rust workspace crates for the AIL toolchain. |
| `docs/` | Canonical design, implementation, status, and maintainer documentation. |
| `fuzz/` | `cargo-fuzz` targets for parser, CBOR, and runtime fuzzing. |
| `scripts/` | Release helper scripts, including tag creation and metadata preflight. |
| `.github/workflows/` | CI workflow configuration. |
| `.atl/` | Local skill-registry cache/docs, not language source of truth. |

## Crate map

| Crate | Responsibility | Read first |
|-------|----------------|------------|
| `ail-core` | Semantic Graph primitives, graph index, node/edge/effect/contract/reference types. | `crates/ail-core/src/semantic_graph.rs` |
| `ail-change` | AI Change Language model, parser, canonicalizer, op schema checks, ACL migrations, and ChangeSet apply path. | `crates/ail-change/src/lib.rs` |
| `ail-verify` | Type/effect/contract/resource/concurrency/boundary/codegen/package checkers and verification reports. | `crates/ail-verify/src/lib.rs` |
| `ail-compiler` | Deterministic graph-to-Core-IR-to-ANF-to-WASM/native pipeline, hash chain, manifests, source maps, incremental cache. | `crates/ail-compiler/src/lib.rs` |
| `ail-runtime` | Wasmtime-backed deny-by-default capability host, profiles, handlers, schemas, audit, replay, reports, rollback helpers. | `crates/ail-runtime/src/lib.rs` |
| `ail-storage` | Content-addressed object storage, graph snapshots/logs, memory/file/Postgres stores, retention, migrations, branches, tags, approvals, exports, integrity. | `crates/ail-storage/src/lib.rs` |
| `ail-context` | Read-only hash-stable context queries, DTOs, source adapters, response builder, deterministic summaries, in-process server-shaped API. | `crates/ail-context/src/lib.rs` |
| `ail-package` | Package manifests, trust, signing, resolver, registry, lockfile, advisories, yanking, compatibility, capability policy. | `crates/ail-package/src/lib.rs` |
| `ail-remote` | Remote collaboration primitives: identities, signer policy, object bundles, signed context slices, remote ChangeSets, exchange DTOs, optional crypto feature. | `crates/ail-remote/src/lib.rs` |
| `ail-coordinator` | Authoritative multi-agent ChangeSet serialization, semantic rebase, conflict classification, remote submission verification. | `crates/ail-coordinator/src/lib.rs` |
| `ail-stdlib` | Canonical v1 standard-library registry, semantic modules, capability names, executable pure stdlib descriptors. | `crates/ail-stdlib/src/lib.rs` |
| `ail-cli` | `ail` binary, command dispatch, project `.ail/` layout, store selection, JSON/human output modes, local workflows. | `crates/ail-cli/src/cli.rs` |
| `ail-dogfood` | Self-referential examples that model AIL graph, ChangeSet, and stdlib concepts with AIL types. | `crates/ail-dogfood/src/lib.rs` |
| `ail-testkit` | Shared deterministic graph, snapshot, store, and fixture helpers for workspace tests. | `crates/ail-testkit/src/lib.rs` |

## Important implementation caveats

- `ail-context` is currently in-process, not a deployed network server.
- The executable language surface is narrower than the target Core IR/type-system docs.
- WASM effect dispatch exists, but rich typed ABI/value layout remains validation work.
- Native backend work proves provenance/object emission and lowers a subset; do not assume native execution parity with WASM.
- CLI command surface exists in code, but docs may describe target workflow depth beyond the current durable behavior of every command.
