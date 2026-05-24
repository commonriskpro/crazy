# Decision log

<!-- Status: Decision log. Updated during docs consistency pass against implementation through `feature/docs-consistency`. -->

## Product scope

- Design full product upfront; implementation is sequenced by risk, not framed as a scope-reducing MVP.
- Implementation phases are validation milestones — they prove high-risk subsystems work.
- General-purpose AI-native language, not a DSL.
- Source of truth is Semantic Graph, not files.

## Core architecture

- Semantic Core IR is ML-like with effect rows, contracts, capabilities, resource handles.
- ANF is main compiler IR; SSA is backend artifact.
- WASM is first executable target; native can come later.
- Runtime is deny-by-default capability host.

### Maturity direction

- **Reliability target**: Rust-level mature, usable reliability — memory/resource safety and zero-cost abstractions. This is the bar; prototypes and "mostly works" are not acceptable end states.
- **Not a Rust clone**: AIL does not adopt Rust's borrow-checker model, lifetime annotations, or surface syntax. The mechanism is different.
- **The mechanism**: memory/resource safety and zero-cost abstractions are achieved via Semantic Graph-visible ownership (Handle<Resource, Mode> with Copy/Affine/Linear/Shared modes), resource lifecycle enforcement, effects, and capabilities — all represented as first-class nodes/edges in the Semantic Graph, verified before lowering, and lowered through Core IR → ANF → SSA → executable target without runtime overhead.

## Type system

- Nominal by default; structural only via explicit constraints.
- No general implicit subtyping.
- Inference can propose; canonical graph stores explicit signatures.
- Generics include type/effect/capability/limited const params.
- Dynamic dispatch explicit with `Dyn<Interface>`.
- No null/nil/undefined in Core IR.

## Change/verification/runtime

- AI Change Language is line-oriented DSL and versioned protocol.
- ChangeSets are atomic graph transactions.
- Requires/expect are AI claims, not authority.
- Verification states are explicit and profile-gated.
- Assumptions must be boundary-scoped, owned, expiring, approved.
- Runtime checks must be materialized and hash-covered.
- Packages import symbols but do not grant capabilities.

## Context/tooling/storage

- Context Server is semantic query layer, not RAG over files.
- Structured context is authoritative; summaries are non-authoritative helpers generated from structured facts.
- Storage is append-only semantically, GC/compacted physically by policy.
- Tooling operates on graph snapshots and ChangeSets.

## Technology choices

### Toolchain

| Area | Decision |
|------|----------|
| Implementation language | **Rust** |
| Parser | **Hand-written line parser for ACL** plus a small hand-written expression parser. This reverses the earlier `chumsky`/`lalrpop` spike placeholder because the implemented grammar subset is intentionally simple and line-oriented. See `crates/ail-change/src/parser.rs` and `crates/ail-compiler/src/expr_parser.rs`. |
| SMT solver | **Z3** first, behind an abstract solver API; cvc5-compatible later |
| WASM runtime | **Wasmtime** |
| Compiler backend v1 | **Cranelift** (WASM); LLVM / native added later if needed |

### Data formats and crypto

| Area | Decision |
|------|----------|
| Hashing | **BLAKE3** |
| Canonical serialization / runtime payloads | **Deterministic CBOR** — a later binary format spike may supersede if benchmarks justify |
| Distributed tracing | **OpenTelemetry** |
| Package signing | **Sigstore-style / keyless** where possible; signed artifacts and registry metadata required |
| Symmetric encryption | **AES-256-GCM** |
| Asymmetric signing | **Ed25519** |
| Key derivation | **Argon2id** |
| Key exchange | **X25519** |

### Storage

| Area | Decision |
|------|----------|
| Storage API | **Async-native GraphStore**. Compiler consumes immutable snapshots and an in-memory/mmap compilation database. |
| Storage model | **FoundationDB-compatible**: ordered keys, immutable snapshots, ChangeSet log, CAS blobs, transactionally updated indexes. |
| Initial backend | **Postgres** (metadata + indexes) + **CAS object store / filesystem** (blobs). FDB is the aspirational production backend (spike required to confirm operational cost). SQLite/libSQL is an optional simple/local backend — not the primary architecture. |
| Multi-agent coordination | Agents submit ChangeSets against base snapshots; a coordinator serializes authoritative commits; stale changes rebase and reverify. |

### Standard library and packages

| Area | Decision |
|------|----------|
| Stdlib v1 scope | **Semantic core** only (types, effects, contracts, primitives). Service capabilities and adapters are official packages. |
| Database capability | **Official package**, not stdlib core. |
| CLI name (v1) | **`ail`** |
| Interactive shell | **Not required** for first full product release. |
| Context Server summaries | **Deterministic / template-based** from structured facts; natural-language summaries are non-authoritative. |

## Implementation decisions and deviations

| Area | Decision | Rationale / deviation |
|------|----------|-----------------------|
| Expression parser | Implement a small recursive-descent parser for the current executable expression subset (`int`, `bool`, vars, calls, `if`, arithmetic/comparison helpers). | This deliberately avoids a parser-generator dependency until the expression language grows enough to justify one. It is not the full surface grammar in `docs/core-ir.md`; it is the executable subset used by current lowering/codegen. See `crates/ail-compiler/src/expr_parser.rs`. |
| WASM ABI | Use `ail/host_call` for scalar effect dispatch, `ail/host_call_write` for structured handler responses, module memory export, and descriptor-backed decoding for the current executable subset. | The original docs describe a richer production ABI. The implemented subset proves effect execution and structured boundary decoding while leaving full value layout, ownership, and handle semantics as explicit validation work. See `crates/ail-compiler/src/wasm.rs` and `crates/ail-runtime/src/host.rs`. |
| File store layout | Store filesystem CAS objects as flat files named by lower-hex BLAKE3 object id; Postgres stores objects in `cas_objects` and indexes snapshots through `snapshots_index`. | This is simpler than the conceptual `graph_store/nodes/edges/...` directory layout and keeps schema meaning in CBOR objects rather than paths. See `crates/ail-storage/src/backends/tempfile.rs` and `crates/ail-storage/src/backends/postgres.rs`. |
| Effect dispatch protocol | Compile ANF `EffectCall` through WASM host dispatch and route host-side calls through registered `Handler`s after grant/schema checks. | Earlier runtime notes called ABI wiring deferred; current implementation now dispatches compiled effect calls and audits them. See `crates/ail-runtime/tests/effect_runtime_tests.rs`, `crates/ail-runtime/src/host.rs`, and `crates/ail-runtime/src/handler.rs`. |
| Context Server deployment | Implement context as an in-process crate (`ail-context`) rather than a network transport server. | The protocol remains server-shaped, but the first implementation proves bounded, hash-stable semantic responses without adding transport/auth surface area. See `crates/ail-context/src/lib.rs`. |
| Remote service boundary | Add transport-agnostic exchange DTOs before adding a durable remote sync server. | This gives context/remote/coordinator a service-shaped contract while avoiding premature network, auth, and persistence choices. Remote bundle exchange stays on deterministic CBOR because bundle maps are keyed by `ObjectId`, not JSON strings. See `crates/ail-context/src/server.rs`, `crates/ail-remote/src/exchange.rs`, and `crates/ail-coordinator/src/coordinator.rs`. |
| Native backend | Implement Cranelift native object emission with provenance/manifests and native lowering for the current Phase 8 expression subset. | This advances beyond the original trap-stub spike without claiming native execution parity with WASM; concurrency, dynamic dispatch, and resource lifecycle still trap or remain future work. See `crates/ail-compiler/src/native.rs`. |

## Reversed decisions

| Previous decision | Current decision | Why |
|-------------------|------------------|-----|
| Parser crate to be `chumsky` or `lalrpop`. | Hand-written parsers for ACL and current expression subset. | Current grammars are small enough that parser generators would add more dependency and abstraction cost than value. Re-evaluate if grammar complexity grows. |
