# Implementation blueprint

<!-- Status: Living roadmap. Completed phases are validation milestones, not production-readiness claims. -->

This is the living implementation roadmap for AIL. It preserves the full product direction while separating completed validation evidence from the remaining production work.

Related: [Codebase guide](CODEBASE-GUIDE.md), [Architecture](architecture.md), [Decision log](decision-log.md), [Risks](risks.md), [Decisions register](open-questions.md), [Wave operating model](wave-operating-model.md).

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
| Context Server | Implemented subset | `crates/ail-context/src/lib.rs`, `transport.rs` | HTTP transport, distributed auth, distributed freshness, redaction operations. |
| Stdlib | Implemented subset | `crates/ail-stdlib/src/lib.rs`, module tests | Compatibility policy, official packages/adapters, verified contracts. |
| Packages | Implemented subset | `crates/ail-package/src/lib.rs`, package tests | Registry operations, federation, reproducible-build proof workflows. |
| Coordination / remote | Implemented subset | `crates/ail-coordinator/src/lib.rs`, `crates/ail-remote/src/lib.rs` | Durable remote sync service, multi-hop collaboration, key management. |
| Dogfooding | Completed validation milestone | `crates/ail-dogfood/src/lib.rs`, dogfood tests | Real project authoring loop using AIL itself, not only Rust examples. |
| Release hardening | Implemented subset | `docs/release-policy.md`, `docs/migration-guide.md`, `scripts/tag-release.sh`, `scripts/release-preflight.sh` | Published compatibility guarantees and production release discipline. |

## Implementation coverage score (Wave 13D audit — 2026-05-24)

Waves 7–12 closed a meaningful set of gaps. This section records what changed and what remains. Estimates measure the fraction of the target design (as documented) that is functionally implemented with tests. They are deliberately conservative; the full target design remains larger than the current executable surface.

**Overall estimate: ~70% of documented target design implemented (up from ~65% before waves 7–12).**

No area has reached production-ready status.

### What waves 7–12 added

| Wave block | Key deliverables |
|------------|-----------------|
| W7–8 (compiler) | Closure env layout in linear memory; WASM resource primitives (`CellNew`/`Get`/`Set`, `MapNew`, `SetNew`, `IndexGet`); `ForEach` inline loop emission; `Fold` compile-time diagnostic gate. |
| W9 (storage + runtime + verify) | GC retention holds (branch/tag/audit); `collect_reachable_object_ids_for_snapshots` helper; handler trust level API + preflight enforcement; assumption-expiry preflight gate (stage 7); `verified_profile` embedding in persisted `VerificationReport`. |
| W10 (compiler + storage) | `Bytes` literal ABI for WASM (packed ptr/len `i64` + data section); generalized unsupported-construct diagnostic gate; Postgres report index; in-process memory report index. |
| W11 (compiler + runtime) | `Fold` via `call_indirect` + function table; native `Bytes` backend (packed `i64`); improved native-link usability (JSON errors, docs); in-memory secret vault + `secret.read` handler; secret WASM e2e tests. |
| W12 (compiler + runtime) | Lambda hoisting: capture-free 2-param Lambdas into WASM function table; `--runtime-lib` flag for `ail link`; `SecretProvider` trait (pluggable vault backend); typed `Bytes` ABI boundary tests; apply-gate profile matching enforcement. |
| Docs alignment (W9–W11) | Compiler, runtime security, context-server, and ABI-value-contract docs updated to match implemented subset. |

### Per-area coverage after waves 7–12

Scores are rough upper bounds on "what fraction of the documented target design is implemented and tested."

| Area | Estimate | Notable additions (W7–12) | Primary remaining gap |
|------|----------|--------------------------|----------------------|
| Design baseline / docs | ~85% | Docs alignment passes in four lanes | Ongoing drift as code evolves |
| Workspace / CLI | ~68% | Apply-gate profile matching, `--runtime-lib` flag | Durable workflows, `.ail/` persistence, UX polish |
| Storage | ~75% | GC retention holds, reachability helper, report indexes | Production scale, migration runbooks, operational backups |
| Semantic Graph / Core IR | ~58% | Minor: Bytes/Fold/ForEach expand executable surface slightly | Records, variants, `Result`/`Option`, pattern matching |
| ChangeSets / ACL | ~65% | Verification gate enforced before apply | Full operation surface, richer repair loop |
| Verification | ~72% | `verified_profile` embedded, assumption-expiry gate, report index | Translation validation, prod/critical policy rigor |
| Compiler | ~68% | `Bytes` ABI (WASM + native), `Fold`/`ForEach` execution, Lambda hoisting | Captured closure reducers, full language surface, native archive auto-build |
| Runtime | ~78% | Handler trust, secret vault + provider trait, schema enforcement, typed `Bytes` ABI, e2e | External secret providers (no real adapters), full async/channel runtime, fuzz coverage |
| Context Server | ~58% | Query-variant docs alignment (code was already present) | HTTP transport, distributed auth, freshness operations |
| Stdlib | ~52% | Minor | Compatibility policy, official adapters, verified contracts |
| Packages | ~46% | Minor | Registry network ops, federation, reproducible-build proof |
| Coordination / remote | ~46% | Minor | Durable remote sync, multi-hop collaboration, key management |
| Dogfooding | ~62% | Minor | Real project authoring loop (not only Rust examples) |
| Release hardening | ~73% | `--runtime-lib` workflow documented | Published compatibility guarantees, production release discipline |

### Remaining known gaps (not yet addressed)

The following gaps are confirmed absent or only stub-level. Do not claim coverage here without new implementation evidence:

- **Captured closure reducers**: Non-hoistable Lambdas (with captures, or param count ≠ 2) emit a closure env struct but `fn_idx` is a placeholder `0`; `call_indirect` dispatch for these is not implemented (`wasm_emit.rs` line 913–914 comment).
- **Full async/channel runtime**: `invoke_async` wraps synchronous WASM execution in a Tokio task; there is no channel-based runtime, actor model, or async-capable WASM primitive dispatch.
- **External secret providers**: `SecretProvider` trait is defined and `SecretVault` implements it; no real adapter for HashiCorp Vault, AWS Secrets Manager, or any external store exists.
- **Native runtime archive auto-build**: `--runtime-lib` accepts a pre-built `ail_runtime.a`; there is no `build.rs` or CI step that auto-builds and bundles the archive.
- **Schema-driven typed capability output**: `CapabilityOutputSchema` validates JSON field presence; it is not bridged to Core IR `ValueLayout` types — field types are free-form strings, not IR-typed.
- **Runnable native executable workflow**: `ail compile --native` + `ail link --runtime-lib` produces a native object and links it, but the full path to a standalone runnable binary (including runtime archive build) has no automated workflow or CI evidence.
- **Full language surface (records, variants, pattern matching)**: Many `Instruction::Unreachable` stubs remain in `wasm_emit.rs` for these constructs. Execution traps at runtime; there is no compile-time rejection for most of them.
- **Performance validation**: No benchmarks, regression thresholds, or large-graph fixtures exist for storage, compiler, or runtime.
- **Networked Context Server transport**: Context slices work in-process only; HTTP/stdio transport is not implemented.
- **Package registry network operations**: No HTTP registry client/server path; no Ed25519-verified package exchange.

## Next recommended milestones

| Milestone | Goal | Success evidence |
|-----------|------|------------------|
| Executable language surface | Expand parsed/lowered/executed expressions toward the documented Core IR subset. | Parser/lowering/codegen/runtime tests for records, variants, `Result`/`Option`, pattern matching, resource/concurrency stubs or explicit rejections. |
| WASM ABI and memory model | Define and implement typed value layout across compiler and runtime. | ABI spec, memory access tests, schema/value roundtrips, host-call compatibility tests. |
| Production verification profile | Make `prod` acceptance meaningful and hard to misread. | Policy tests proving unverified/unsafe/assumed handling, translation-validation hooks, report fixtures. |
| Runtime hardening | Strengthen isolation, limits, audit, rollback, replay, and capability dispatch under failure. | Negative runtime tests, fuzz coverage, audit snapshots, limit/revocation tests. |
| AI-native tooling loop | Turn context -> ChangeSet -> verify -> apply -> repair into a durable workflow. | CLI integration tests with persisted `.ail/` state and machine-readable diagnostics. |
| Ecosystem path | Clarify package registry, official packages, signing, advisories, and compatibility. | Registry workflow tests, signed package fixtures, release/compatibility docs. |
| Performance validation | Prove graph, storage, context, compiler, and runtime behavior at realistic sizes. | Benchmarks, regression thresholds, large-graph fixtures, documented bottlenecks; see [Performance validation](performance.md). |

## Parallel implementation wave

The next wave is split by reviewable, mostly non-conflicting work units. Each branch should keep tests with the behavior it implements and stay near the 400-line review budget when possible.

| Branch | Goal | Primary files/crates | Conflict risk | Verification |
|--------|------|----------------------|---------------|--------------|
| `feat/storage-perf` | Add storage/CAS/Postgres scale benchmarks and larger fixtures. | `crates/ail-storage`, optionally `crates/ail-testkit` | Very low | `cargo test -p ail-storage`; `cargo bench -p ail-storage` |
| `feat/expr-parser-expand` | Expand executable expression parsing toward records, variants, `Option`/`Result`, and pattern matching stubs or explicit rejections. | `crates/ail-compiler/src/expr_parser.rs`, Core IR/lowering tests | Low | `cargo test -p ail-compiler` |
| `feat/context-transport` | Add the first stdio/MCP-like Context Server transport over the existing in-process API. | `crates/ail-context` | Low | `cargo test -p ail-context` |
| `feat/package-registry-network` | Add a simple HTTP registry client/server path with Ed25519 verification fixtures. | `crates/ail-package` | Low | `cargo test -p ail-package` |
| `feat/translation-validation` | Make prod/critical profile validation materially stricter with provenance/shape and initial control-flow/effect obligations. | `crates/ail-verify` | Low-medium | `cargo test -p ail-verify` |
| `feat/wasm-abi-typed` | Implement the versioned typed-value WASM ABI and host decoding around RC/resource-handle semantics. | `crates/ail-compiler`, `crates/ail-runtime` | Medium-high | `cargo test -p ail-compiler`; `cargo test -p ail-runtime` |

Do not run `feat/closure-capture` in parallel with `feat/expr-parser-expand` or `feat/wasm-abi-typed`. Closure capture depends on the parser expansion, typed ABI, and memory model evidence and should be split into chained PRs after those land.

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
