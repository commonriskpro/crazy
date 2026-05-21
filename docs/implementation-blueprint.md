# Implementation blueprint

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

| Phase | Name | Objective | Primary output | Validation |
|------:|------|-----------|----------------|------------|
| 0 | Design baseline | Close architecture, risks, and technology choices. | Canonical docs, decision log, risk register. | Open questions are converted into decisions or validation items. |
| 1 | Project/toolchain skeleton | Create Rust workspace, crate layout, CLI entrypoint, and testing infrastructure. | `ail` runs basic commands and tests. | CI/local tests execute consistently. |
| 2 | Storage and snapshots foundation | Define async-native `GraphStore`, CAS abstraction, immutable snapshots, and ChangeSet log. | Snapshot read/write path and artifact-addressed storage. | Store/load snapshot benchmark and content-hash stability test. |
| 3 | Semantic Graph core | Model modules, symbols, nodes, edges, contracts, effects, and capabilities. | Program representation as graph objects. | Round-trip semantic graph fixtures without text as source of truth. |
| 4 | AI Change Language / ChangeSets | Parse, canonicalize, verify, and apply semantic ChangeSets transactionally. | ChangeSet parser and graph transaction application. | Valid ChangeSet applies cleanly; stale base snapshot requires rebase. |
| 5 | Type/effect checker | Implement nominal types, basic generics, effects, capabilities, and verification reports. | First real verification report. | Sample graph reaches explicit verification states. |
| 6 | Contracts and refinements | Add `requires`, `ensures`, runtime checks, and solver API. | Contract obligations and SMT/runtime-check outcomes. | Z3-backed proof succeeds on simple examples; unsupported predicates degrade explicitly. |
| 7 | Compiler IR pipeline | Lower verified graph snapshots to Core IR, ANF, and WASM layout. | Initial WASM artifact from graph input. | Golden lowering tests preserve semantic provenance. |
| 8 | Runtime host | Execute WASM with Wasmtime, host capability ABI, deny-by-default policy, and audit events. | Controlled runtime execution. | Program cannot access external effects without granted capabilities. |
| 9 | CLI workflow end-to-end | Connect `ail context`, `ail change`, `ail verify`, `ail apply`, `ail compile`, and `ail run`. | First complete local workflow. | ChangeSet-to-execution demo works from a clean project. |
| 10 | Context Server | Serve semantic queries, context slices, and deterministic summaries from structured facts. | Context API and line-oriented query surface. | Agent gets bounded, hash-tied context for a target change. |
| 11 | Stdlib semantic core | Implement core primitives: result/option/text/bytes/collections/time/testing/capability diagnostics. | Usable core library for real examples. | Example programs avoid ad-hoc builtins. |
| 12 | Packages and trust | Add package manifests, registry model, signing, trust levels, and reproducible-build metadata. | Verifiable package dependency model. | Package import does not grant runtime capabilities. |
| 13 | Multi-agent coordination | Add base snapshots, authoritative coordinator, semantic rebase, and conflict handling. | Multiple agents propose independent ChangeSets safely. | Concurrent ChangeSets serialize, rebase, or fail deterministically. |
| 14 | Dogfooding milestone | Use the toolchain/language model to describe parts of itself. | Self-hosting-adjacent semantic examples. | The language can represent its own graph/types/ChangeSet concepts. |
| 15 | Performance hardening | Add incremental compilation, artifact cache, graph indexes, and large-project benchmarks. | Scalable compilation path. | Large fixture compiles by dirty frontier, not whole-graph rebuild. |
| 16 | Distributed collaboration | Add object bundle exchange, signed context slices, and remote graph sync. | Team/remote-agent collaboration protocol. | Remote agent context and ChangeSet inputs are tamper-evident. |
| 17 | Native/backend expansion | Evaluate Cranelift/native or LLVM for native artifacts. | Optional native target path. | Native path preserves verification provenance and capability boundaries. |
| 18 | Release hardening | Finalize security, migrations, compatibility matrix, docs, and release policy. | First serious release candidate. | Compatibility and migration tests pass across sample projects. |

---

## First implementation block

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

Start the first SDD change:

```txt
toolchain-foundation
```

The goal is not to build the language yet. The goal is to create the project skeleton and prove the implementation can grow around the architecture instead of fighting it.
