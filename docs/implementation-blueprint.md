# Implementation blueprint

<!-- Status: Living roadmap. Completed phases are validation milestones, not production-readiness claims. -->

This is the living implementation roadmap for AIL. It preserves the full product direction while separating completed validation evidence from the remaining production work.

Related: [Codebase guide](CODEBASE-GUIDE.md), [Architecture](architecture.md), [Decision log](decision-log.md), [Risks](risks.md), [Decisions register](open-questions.md).

## Status taxonomy

| Status | Meaning |
|--------|---------|
| Completed validation milestone | Code and tests prove an architecture slice works. This validates direction; it does not imply the full design is implemented. |
| Implemented subset | A real implementation exists, but it intentionally covers less than the target design. Scope limits must stay visible in docs and tests. |
| Production-ready | Hardened for production use with compatibility policy, security review, operational story, benchmark evidence, and failure-mode coverage. This repo does not currently claim production-ready status. |
| Historical notes | Preserved planning context. Useful for why the project sequenced work this way, not for current status. |

## Critical path

```txt
Storage / snapshots
  -> Semantic Graph
  -> ChangeSets
  -> type/effect/contract verification
  -> compiler pipeline
  -> WASM runtime host
  -> CLI end-to-end workflow
  -> context slices
  -> packages, remote exchange, and coordination
  -> production hardening
```

The product only becomes AI-native when graph, ChangeSet, verifier, compiler, runtime, storage, and context work together. A compiler alone would be traditional language infrastructure with AI-adjacent tooling.

## Current milestone map

| Area | Current status | Evidence | Remaining production gap |
|------|----------------|----------|--------------------------|
| Design baseline | Completed validation milestone | `docs/architecture.md`, `docs/decision-log.md`, `docs/open-questions.md` | Keep docs aligned as implementation narrows or expands scope. |
| Workspace and CLI | Implemented subset | `Cargo.toml`, `crates/ail-cli/src/cli.rs` | Durable workflows, UX polish, and command-depth parity with target tooling. |
| Storage and snapshots | Implemented subset | `crates/ail-storage/src/lib.rs`, storage tests | Production scale, operational backups, retention defaults, migration runbooks. |
| Semantic Graph / Core IR | Implemented subset | `crates/ail-core/src/semantic_graph.rs`, roundtrip tests | Full Core IR semantics and executable language coverage. |
| ChangeSets / ACL | Implemented subset | `crates/ail-change/src/parser.rs`, `canonical.rs`, `apply.rs` | Full operation surface, richer repair loop, compatibility/migration discipline. |
| Verification | Implemented subset | `crates/ail-verify/src/lib.rs`, pipeline/checker tests | Prod/critical profile rigor, solver limits, translation validation, policy UX. |
| Compiler | Implemented subset | `crates/ail-compiler/src/lib.rs`, lowering/WASM/native tests | Full executable surface, backend parity, large-project performance evidence. |
| Runtime | Implemented subset | `crates/ail-runtime/src/lib.rs`, runtime tests | Rich typed WASM ABI, memory/resource model, hardened isolation, operational limits. |
| Context Server | Implemented subset | `crates/ail-context/src/lib.rs` | Network transport, auth, distributed freshness, redaction operations. |
| Stdlib | Implemented subset | `crates/ail-stdlib/src/lib.rs`, module tests | Compatibility policy, official packages/adapters, verified contracts. |
| Packages | Implemented subset | `crates/ail-package/src/lib.rs`, package tests | Registry operations, federation, reproducible-build proof workflows. |
| Coordination / remote | Implemented subset | `crates/ail-coordinator/src/lib.rs`, `crates/ail-remote/src/lib.rs` | Durable remote sync service, multi-hop collaboration, key management. |
| Dogfooding | Completed validation milestone | `crates/ail-dogfood/src/lib.rs`, dogfood tests | Real project authoring loop using AIL itself, not only Rust examples. |
| Release hardening | Implemented subset | `docs/release-policy.md`, `docs/migration-guide.md`, `scripts/tag-release.sh` | Published compatibility guarantees and production release discipline. |

## Next recommended milestones

| Milestone | Goal | Success evidence |
|-----------|------|------------------|
| Executable language surface | Expand parsed/lowered/executed expressions toward the documented Core IR subset. | Parser/lowering/codegen/runtime tests for records, variants, `Result`/`Option`, pattern matching, resource/concurrency stubs or explicit rejections. |
| WASM ABI and memory model | Define and implement typed value layout across compiler and runtime. | ABI spec, memory access tests, schema/value roundtrips, host-call compatibility tests. |
| Production verification profile | Make `prod` acceptance meaningful and hard to misread. | Policy tests proving unverified/unsafe/assumed handling, translation-validation hooks, report fixtures. |
| Runtime hardening | Strengthen isolation, limits, audit, rollback, replay, and capability dispatch under failure. | Negative runtime tests, fuzz coverage, audit snapshots, limit/revocation tests. |
| AI-native tooling loop | Turn context -> ChangeSet -> verify -> apply -> repair into a durable workflow. | CLI integration tests with persisted `.ail/` state and machine-readable diagnostics. |
| Ecosystem path | Clarify package registry, official packages, signing, advisories, and compatibility. | Registry workflow tests, signed package fixtures, release/compatibility docs. |
| Performance validation | Prove graph, storage, context, compiler, and runtime behavior at realistic sizes. | Benchmarks, regression thresholds, large-graph fixtures, documented bottlenecks. |

## Validation rules

- A milestone needs executable evidence: tests, fixtures, benchmark output, verification reports, or runnable CLI behavior.
- Milestones do not reduce product scope; they validate whether the selected architecture survives implementation.
- Storage and compiler work must measure large-project behavior early.
- Multi-agent work must preserve the rule that agents propose ChangeSets against base snapshots and the coordinator serializes authoritative commits.
- Runtime work must preserve deny-by-default semantics; no external effect is available without a capability grant.
- Docs must state whether they describe target design, implemented subset, or historical context.

## Historical notes

The original implementation plan sequenced phases through SDD changes:

| SDD change | Covered |
|------------|---------|
| `toolchain-foundation` | Workspace, crate layout, CLI skeleton. |
| `storage-snapshots-foundation` | `GraphStore`, CAS objects, immutable snapshots, ChangeSet log shape. |
| `semantic-graph-core` | Initial Semantic Graph model. |
| `changeset-transaction-model` | ACL parser/canonicalizer/apply path. |
| `type-effect-verification` | Type/effect verification reports. |
| `contracts-refinements` | Contract/refinement obligations and solver API. |
| `wasm-compiler-pipeline` | Core IR, ANF, WASM artifacts, manifests. |
| `runtime-capability-host` | Wasmtime host, deny-by-default grants, audit/reporting. |
| `cli-end-to-end-workflow` | Local ChangeSet-to-runtime proof path. |
| `context-server` | In-process semantic context slices. |
| `stdlib-semantic-core` | Stdlib registry and semantic modules. |
| `packages-trust-model` | Package manifests, trust, signing, resolver/policy pieces. |
| `multi-agent-coordination` | Coordinator/rebase/conflict handling. |
| `dogfooding-milestone` | Self-model examples. |
| `performance-hardening` | Incremental compiler/cache/index structures. |
| `distributed-collaboration` | Remote bundles, signing, exchange primitives. |
| `native-backend-expansion` | Cranelift native object emission spike. |
| `release-hardening` | Release policy and migration docs. |

The first block was intentionally limited to foundation work:

```txt
toolchain-foundation
  -> storage-snapshots-foundation
  -> semantic-graph-core
```

That plan is complete as historical sequencing. Current work should use the roadmap above and the active validation registers in [Risks](risks.md) and [Decisions register](open-questions.md).
