# Decision log

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
| Parser | **chumsky** or **lalrpop** — exact crate deferred to implementation spike |
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
