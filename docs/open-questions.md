# Decisions and validation register

<!-- Status: Validation register. No unresolved design questions remain here; implementation gaps are tracked as validation-required items. -->

This register records closed decisions and validation-required items. It replaces the open-questions format; nothing here is unresolved.

Related: [Decision log](decision-log.md), [Risks](risks.md).

---

## Closed decisions

### Core IR / type system

| Topic | Decision |
|-------|----------|
| Formal semantics of Core IR | Defined in `docs/core-ir.md`. Operational semantics documented; formal proof of completeness is a validation milestone, not a blocker. |
| Refinement predicate power | Use SMT (Z3 first, abstract solver API). Power is bounded by solver performance. Expressiveness limits are a tracked validation risk — see [Risks](risks.md). |
| Effect handler semantics | Simpler handler model adopted; algebraic effect semantics only if the handler model proves insufficient (tracked spike). |
| Resource ownership | Practical resource modes (unique/shared/borrowed) adopted; full linear/affine type theory deferred as an optional future extension. |
| `Dyn<Interface>` model | Explicit dynamic dispatch with contracts and effect constraints; full formal model documented in `docs/type-system.md`. |

### Compiler

| Topic | Decision |
|-------|----------|
| Toolchain language | **Rust** |
| Parser | **Hand-written parsers** for the current ACL and expression subsets. Earlier `chumsky`/`lalrpop` direction was reversed because the implemented grammar is deliberately small and line-oriented. Code: `crates/ail-change/src/parser.rs`, `crates/ail-compiler/src/expr_parser.rs`. |
| ANF representation | ANF makes effect order structural; exact serialization format decided during implementation. |
| SSA and backend | **Cranelift** for WASM v1. LLVM/native added later if needed. Custom SSA not required. |
| WASM ABI layout | Current implementation uses an `ail/host_call` ABI with pointer/length fields and an `i64` result for effect dispatch. The next implementation target is an explicit, versioned typed-value layout for scalars, text, lists, records, variants, `Result`, `Option`, and opaque resource handles. Deterministic CBOR remains valid for manifests/storage/debug and compatibility paths, not the primary rich runtime ABI. Code: `crates/ail-compiler/src/wasm.rs`, `crates/ail-runtime/src/host.rs`. |
| Memory management | Use reference counting for normal heap values plus ownership/affine/linear rules for resource handles. Do not introduce a general GC in v1. Reject cycles initially and revisit only if real programs justify tracing support. |
| Translation validation | Required for `prod`/`critical`; scope defined per profile in `docs/verification.md`. |

### Runtime

| Topic | Decision |
|-------|----------|
| WASM runtime | **Wasmtime** |
| Host payload encoding | **Deterministic CBOR** — stable, self-describing, no custom format needed. A later binary format spike may supersede if benchmarks justify it. |
| WASI usage | Hidden behind host ABI; WASI used as a WASM execution host substrate, not exposed to programs directly. |
| Handler isolation | Isolation model defined in `docs/runtime.md`; latency is a tracked validation risk. |
| Distributed tracing | **OpenTelemetry** |
| Capability call typing | Async-native from the start; sync capability calls exposed only where required by the host ABI contract. |

### Storage

| Topic | Decision |
|-------|----------|
| Storage API | **Async-native GraphStore API**. Compiler consumes immutable snapshots and an in-memory/mmap compilation database — not repeated live DB queries. |
| Storage model | **FoundationDB-compatible**: ordered keys, immutable snapshots, ChangeSet log, CAS blobs, transactionally updated indexes. |
| Initial backend | **Postgres** (metadata + indexes) + **CAS object store / filesystem** (blobs). SQLite/libSQL is an optional simple/local backend only — not the primary architecture. FoundationDB is the aspirational production backend; a spike will determine whether operational cost justifies adoption over Postgres. |
| Implemented file layout | Filesystem object store uses flat BLAKE3-hex filenames; Postgres uses `cas_objects` and `snapshots_index`. This is an implementation simplification of the conceptual graph-store directory layout. |
| Hash algorithm | **BLAKE3** |
| Canonical serialization | **Deterministic CBOR** for runtime payloads and storage objects. |
| Distributed collaboration | Agents submit ChangeSets against base snapshots; a coordinator serializes authoritative commits; stale changes rebase and reverify. Distributed graph collaboration protocol is a tracked validation item. |
| Local retention policy | Default policies defined in `docs/storage.md`; exact defaults tunable by project config. |
| Protected audit archive | Described in `docs/storage.md`; external archival via export bundles. |

### Context Server

| Topic | Decision |
|-------|----------|
| Query syntax | Line-oriented DSL primary; RPC JSON for machine consumers. Both documented in `docs/context-server.md`. |
| Summary generation | **Deterministic / template-based** from structured facts. Structured data is authoritative; natural-language summaries are non-authoritative helpers. |
| Context slice signing | Signing is desirable for distributed agents; key management is a tracked validation item. The first transport target is stdio/MCP-like to validate AI tooling integration before HTTP/distributed auth. |
| Context budgets | Default budgets defined per model tier in `docs/context-server.md`. |
| Audit context exposure | Safe exposure policy documented in `docs/context-server.md`. |
| Transport shape | Current implementation is an in-process `ail-context` API, not a network transport server. The protocol shape remains server-compatible. Code: `crates/ail-context/src/lib.rs`. |

### Packages

| Topic | Decision |
|-------|----------|
| Registry protocol and signing | Start with a deployable HTTP registry and Ed25519 verification. **Sigstore-style / keyless signing** remains the target where possible after the basic registry workflow is validated. Signed artifacts and registry metadata are required. |
| Reproducible builds | Required for `verified` trust level. |
| Federated trust | Supported via trust metadata; cross-org federation details finalized during implementation. |
| Proof verification | Local proof checking available; trusted remote verification allowed at lower trust levels. |
| Package yanking | Yanked packages unavailable for new installs; existing resolved builds continue to work via CAS content addresses. |

### Standard library

| Topic | Decision |
|-------|----------|
| Stdlib v1 scope | **Semantic core** — types, effects, contracts, core primitives. Service capabilities and adapters ship as **official packages**, not stdlib core. |
| Database capability | **Official package**, not stdlib core. |
| Crypto defaults | **BLAKE3** (hashing), **AES-256-GCM** (symmetric), **Ed25519** (signing), **Argon2id** (key derivation), **X25519** (key exchange). |
| Async runtime placement | `std.concurrent` for concurrency primitives; runtime bindings via capability system. |
| Stdlib versioning | Versioned independently from language/Core IR; compatibility rules documented in `docs/stdlib.md`. |

### Tooling

| Topic | Decision |
|-------|----------|
| CLI name | **`ail`** |
| Interactive shell | **Not required** for first full product release. Shell/REPL is a future extension. |
| Editor → ChangeSet | Editor changes produce ChangeSets via language server protocol; details in `docs/tooling.md`. |
| Human approval UX | Structured diff + approval prompt; exact UX iterates with dogfooding. |
| Local experiments | Projects can configure lightweight local graph storage; full storage is the default path. |

### Product / implementation framing

| Topic | Decision |
|-------|----------|
| Implementation sequencing | Full product scope; implementation is sequenced by risk, not by feature reduction. |
| Validation vs MVP | Implementation phases are **validation milestones** — they prove high-risk subsystems work, not reduce product scope. |
| Dogfooding readiness | Defined as the point where the language can describe its own types and ChangeSets; tracked in [Risks](risks.md). |

---

## Validation-required items

These are not open questions. Decisions are made; these items require implementation spikes or benchmarks to confirm the decisions hold.

See [Risks](risks.md) for the full risk register with mitigation and validation strategies.

| Item | Risk if invalid |
|------|----------------|
| Refinement predicate expressiveness / solver performance | Too many `runtime_checked`/`unverified` outcomes; solver too slow on real programs |
| Formal semantics sufficiency for `critical` profile | `critical` profile cannot make meaningful guarantees |
| Cranelift source-map and capability-boundary preservation | Partially validated: compiler tests now cover enriched source-map propagation, capability `source_ref` preservation, deterministic source-map/manifest hashes, and prod/critical rejection when per-binding ChangeSet provenance is missing. Remaining risk is full translation validation and complete profile/report policy enforcement. |
| Postgres + CAS graph store: scale and compile latency on large apps | Compilation becomes slow at scale; may require FDB or architectural change |
| Handler isolation latency | Capability isolation overhead makes runtime unusable at scale |
| Distributed graph collaboration protocol | Multi-agent coordination breaks under concurrent ChangeSets |
| Context-slice signing key management | Distributed agent trust cannot be established or maintained |
| WASM memory model implementation | RC plus ownership/affine/linear resource handles must be validated against performance, closure capture, rich typed ABI, and replay/audit behavior. |
| Expression parser scope | Current parser only accepts the executable subset (`int`, `bool`, vars, calls, `if`, arithmetic/comparison helpers). Full grammar support is future implementation work. |
| Native backend execution parity | Cranelift native backend lowers an implemented subset with provenance/manifests, while concurrency, dynamic dispatch, resource lifecycle, and full WASM parity remain validation work. |
| In-WASM host dispatch completeness | Effect calls execute through `ail/host_call`, but host-side dispatch still has intentionally simple payload/value handling. Rich typed boundary layout remains a validation item. |
