# Implementation blueprint

<!-- Implementation Status: phase table updated to distinguish completed implementation milestones from remaining full-design scope. -->

This blueprint sequences implementation without reducing product scope. Each phase is a validation milestone: it proves a critical part of the full architecture works before the project moves deeper into implementation.

Related: [Architecture](architecture.md), [Decision log](decision-log.md), [Risks](risks.md), [Decisions and validation register](open-questions.md).

---

## Critical path

```txt
Storage / snapshots
  -> Semantic Graph
  -> ChangeSets
  -> Type/effect verification
  -> WASM compiler
  -> Runtime host
  -> CLI end-to-end workflow
  -> Context Server
  -> Packages and multi-agent collaboration
```

The product only becomes AI-native when the graph, ChangeSet, verifier, and runtime capability model work together. A compiler without those pieces would be a traditional compiler with AI-adjacent tooling, not this language.

---

## Phase map

| Phase | Name | Status | Objective | Primary output | Validation / code evidence |
|------:|------|--------|-----------|----------------|----------------------------|
| 0 | Design baseline | Completed | Close architecture, risks, and technology choices. | Canonical docs, decision log, risk register. | Open questions converted into decisions or validation items in `docs/open-questions.md`. |
| 1 | Project/toolchain skeleton | Completed | Create Rust workspace, crate layout, CLI entrypoint, and testing infrastructure. | `ail` crate and workspace test infrastructure. | `Cargo.toml`, `crates/ail-cli/src/main.rs`, `cargo test --workspace`. |
| 2 | Storage and snapshots foundation | Completed | Define async-native `GraphStore`, CAS abstraction, immutable snapshots, and ChangeSet log. | Snapshot read/write path and artifact-addressed storage. | `crates/ail-storage/src/object.rs`, `graph.rs`, `backends/*`. |
| 3 | Semantic Graph core | Completed | Model graph nodes, edges, contracts, effects, and capabilities. | Program representation as graph objects. | `crates/ail-core/tests/*roundtrip.rs`. |
| 4 | AI Change Language / ChangeSets | Completed | Parse, canonicalize, verify shape, and apply semantic ChangeSets transactionally. | ACL parser/canonicalizer/apply path. | `crates/ail-change/src/parser.rs`, `canonical.rs`, `apply.rs`. |
| 5 | Type/effect checker | Completed milestone | Implement type/effect verification reports. | Explicit checker/report crates. | `crates/ail-verify/src/type_checker.rs`, `effect_checker.rs`, `report.rs`. Scope remains simpler than full language design. |
| 6 | Contracts and refinements | Completed milestone | Add contract/refinement obligations and solver API. | Contract checker, proof pipeline, Z3 wrapper. | `crates/ail-verify/src/contract_checker.rs`, `proof.rs`, `z3_solver.rs`. |
| 7 | Compiler IR pipeline | Completed | Lower graph snapshots to Core IR, ANF, and WASM artifacts. | Deterministic hash-linked compiler pipeline. | `crates/ail-compiler/src/core_ir.rs`, `anf.rs`, `wasm.rs`; integration tests. |
| 8 | Runtime host | Completed milestone | Execute WASM with Wasmtime, capability ABI, deny-by-default policy, and audit events. | RuntimeHost with preflight, handlers, schema checks, reports. | `crates/ail-runtime/src/host.rs`; `effect_runtime_tests.rs`. Full rich ABI remains a gap. |
| 9 | CLI workflow end-to-end | Completed milestone | Connect local workflow enough to prove text-to-runtime execution. | End-to-end ChangeSet to graph to compiler to runtime test path. | `crates/ail-dogfood/tests/e2e_pipeline.rs`. CLI surface is not yet the full command set in `docs/tooling.md`. |
| 10 | Context Server | Completed milestone | Serve semantic context slices and deterministic summaries from structured facts. | In-process context API. | `crates/ail-context/src/lib.rs`, `builder.rs`, `summary.rs`. No transport server yet. |
| 11 | Stdlib semantic core | Completed milestone | Implement core primitives and capability diagnostics. | `ail-stdlib` modules. | `crates/ail-stdlib/src/*` and module tests. |
| 12 | Packages and trust | Completed milestone | Add package manifests, signing, trust levels, yanking, resolver/policy pieces. | Verifiable package model. | `crates/ail-package/src/*`; package tests. |
| 13 | Multi-agent coordination | Completed milestone | Add coordinator/rebase/conflict handling. | Coordinator crate and integration tests. | `crates/ail-coordinator/tests/coordinator_integration.rs`. |
| 14 | Dogfooding milestone | Completed milestone | Represent parts of AIL with its own graph/types/ChangeSet concepts. | Dogfood semantic examples. | `crates/ail-dogfood/src/*`, `tests/dogfood_program.rs`. |
| 15 | Performance hardening | Completed milestone | Add incremental compilation/cache/index structures. | Incremental compiler/cache path. | `crates/ail-compiler/src/incremental.rs`, `cache.rs`, incremental tests. Large-project benchmark validation remains open. |
| 16 | Distributed collaboration | Completed milestone | Add bundles, signing, and remote exchange primitives. | Remote bundle/signing crate. | `crates/ail-remote/src/*`. Full remote sync service remains future work. |
| 17 | Native/backend expansion | Completed spike | Evaluate native artifacts while preserving provenance and manifests. | Cranelift object emission with trap stubs. | `crates/ail-compiler/src/native.rs`; `native_backend_tests.rs`. Native expression lowering is intentionally not done. |
| 18 | Release hardening | Completed baseline | Add release, migration, compatibility, and docs policy. | Release policy and storage migrations. | `docs/release-policy.md`, `docs/migration-guide.md`, storage migration tests. |

---

## First implementation block

<!-- Implementation Status: historical section. The first block has been completed; later phases are represented in the phase map above. -->

The first SDD chain should cover phases 1-3:

```txt
toolchain-foundation
  -> storage-snapshots-foundation
  -> semantic-graph-core
```

This block proves that the project can persist a semantic program, address objects by hash, and expose the graph as the source of truth before parser, verifier, compiler, or runtime work begins.

### In scope

- Rust workspace and crate boundaries.
- `ail` CLI skeleton.
- Async-native `GraphStore` trait.
- CAS object abstraction.
- Snapshot identity and immutable snapshot reads.
- ChangeSet log shape, even before full ACL parsing.
- Initial semantic graph data model.
- Fixture-based tests and small storage benchmarks.

### Out of scope

- Full ACL parser.
- Full type/effect checker.
- SMT integration.
- WASM code generation.
- Runtime host.
- Distributed collaboration.
- Package registry.

---

## SDD chaining plan

| SDD change | Covers | Depends on |
|------------|--------|------------|
| `toolchain-foundation` | Phase 1 | Design baseline |
| `storage-snapshots-foundation` | Phase 2 | `toolchain-foundation` |
| `semantic-graph-core` | Phase 3 | `storage-snapshots-foundation` |
| `changeset-transaction-model` | Phase 4 | `semantic-graph-core` |
| `type-effect-verification` | Phase 5 | `changeset-transaction-model` |
| `contracts-refinements` | Phase 6 | `type-effect-verification` |
| `wasm-compiler-pipeline` | Phase 7 | `type-effect-verification` |
| `runtime-capability-host` | Phase 8 | `wasm-compiler-pipeline` |
| `cli-end-to-end-workflow` | Phase 9 | `runtime-capability-host` |
| `context-server` | Phase 10 | `cli-end-to-end-workflow` |
| `stdlib-semantic-core` | Phase 11 | `type-effect-verification` |
| `packages-trust-model` | Phase 12 | `stdlib-semantic-core` |
| `multi-agent-coordination` | Phase 13 | `context-server`, `storage-snapshots-foundation` |
| `dogfooding-milestone` | Phase 14 | `cli-end-to-end-workflow`, `stdlib-semantic-core` |
| `performance-hardening` | Phase 15 | `dogfooding-milestone` |
| `distributed-collaboration` | Phase 16 | `multi-agent-coordination`, `performance-hardening` |
| `native-backend-expansion` | Phase 17 | `wasm-compiler-pipeline`, `performance-hardening` |
| `release-hardening` | Phase 18 | All release-target phases |

---

## Validation rules

- A phase is not complete until it produces executable evidence: tests, fixtures, benchmark output, verification reports, or runnable CLI behavior.
- Validation milestones do not reduce product scope; they prove whether the selected architecture survives contact with implementation.
- Storage and compiler work must measure large-project behavior early. Compilation must operate from immutable snapshots and caches, not repeated live database queries.
- Multi-agent work must preserve the rule that agents propose ChangeSets against base snapshots; only the coordinator serializes authoritative commits.
- Runtime work must preserve deny-by-default semantics; no external effect is available without a capability grant.

---

## Next step

<!-- Implementation Status: historical section. The original next step is complete. Current next steps are tracked in the risk register and consistency review. -->

Start the first SDD change:

```txt
toolchain-foundation
```

The goal is not to build the language yet. The goal is to create the project skeleton and prove the implementation can grow around the architecture instead of fighting it.
