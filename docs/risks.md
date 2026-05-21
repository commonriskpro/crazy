# Risks and validation register

> Related: [Decisions register](open-questions.md), [Decision log](decision-log.md), [Consistency review](consistency-review.md).

The primary risk is not building a language. It is building a system too complex to be reliable, usable, and verifiable.

Technology choices are closed — see [Decision log](decision-log.md). This register captures what can kill the project, what requires serious research during implementation, and what validation spikes must confirm the chosen decisions hold.

Each risk entry shape:

```txt
description
impact
likelihood
mitigation
validation strategy
owner/area
```

Cada riesgo debe tener:

```txt
description
impact
likelihood
mitigation
validation strategy
owner/area
```

### Riesgos técnicos

#### Complejidad sistémica

Riesgo:

```txt
El sistema combina graph store, compiler, verifier, runtime, package manager,
Context Server, stdlib, tooling y LLM protocol.
```

Mitigación:

```txt
architecture boundaries estrictos
schemas versionados
test suites por capa
integration tests end-to-end
dogfooding temprano
```

#### Trusted Computing Base demasiado grande

Riesgo:

```txt
Parser, canonicalizer, verifier, compiler, runtime host y storage podrían volverse demasiado grandes para confiar.
```

Mitigación:

```txt
mantener trusted core pequeño
IR validators por etapa
translation validation donde sea posible
reproducible builds
fuzzing de parser/canonicalizer/runtime ABI
```

#### Semantics drift

Riesgo:

```txt
La semántica real del compiler/runtime diverge de la semántica documentada del Core IR.
```

Mitigación:

```txt
formal-ish semantics para Core IR
golden tests
property tests
lowering tests Core IR -> ANF -> SSA -> WASM
semantic source maps obligatorios
```

### Riesgos de verificación

#### Solver limits

Riesgo:

```txt
SMT/solver no puede probar contratos/refinements no triviales,
generando demasiados runtime_checked/unverified.
```

Mitigación:

```txt
contracts diseñados para composability
proof obligations pequeñas
stdlib contracts verificados
runtime checks explícitos
profiles claros
diagnostics reparables
```

#### Falsa sensación de seguridad

Riesgo:

```txt
Usuarios creen que “accepted” significa matemáticamente perfecto.
```

Mitigación:

```txt
verification report con estados visibles
unverified/assumed/unsafe destacados
profile-bound artifacts
UX que no oculte deuda
```

#### Assumptions como puerta trasera

Riesgo:

```txt
assumed se convierte en una forma elegante de saltarse verification.
```

Mitigación:

```txt
no free-floating assumptions
owner/expiration/approval
prod/critical gates estrictos
assumption review workflow
```

### Riesgos de UX / LLM

#### Change Language demasiado verboso

Riesgo:

```txt
Los LLMs podrían producir ChangeSets enormes, difíciles de revisar.
```

Mitigación:

```txt
infer_boundary
canonicalization
structured summaries
Context Server slices
operation macros derivadas pero verificables
```

#### Contexto viejo

Riesgo:

```txt
El LLM genera cambios basados en snapshots obsoletos.
```

Mitigación:

```txt
assert_context
base snapshot obligatorio
E_CONTEXT_STALE
semantic rebase
```

#### Summaries engañosos

Riesgo:

```txt
Context Server summary en lenguaje natural contradice structured data.
```

Mitigación:

```txt
structured authoritative
summary non-authoritative
summary generated from structured data
summary consistency tests
```

### Riesgos de performance

#### Graph queries lentas

Riesgo:

```txt
Context Server, impact analysis y verification pueden volverse lentos en proyectos grandes.
```

Mitigación:

```txt
derived indexes
incremental verification
budgeted context queries
snapshot-aware caching
parallel checks
```

#### WASM/capability overhead

Riesgo:

```txt
Host capability calls y boundary encoding pueden ser lentos.
```

Mitigación:

```txt
typed binary ABI
batch capability calls
zero-copy where safe
profile-guided optimization
native backend future path
```

#### Storage growth

Riesgo:

```txt
Append-only graph history, reports, artifacts y audit logs crecen demasiado.
```

Mitigación:

```txt
retention policies
GC
compaction
artifact lifecycle
protected snapshots only where needed
external archival
```

### Riesgos de seguridad

#### Capability bypass

Riesgo:

```txt
Un módulo o handler evita el runtime capability protocol.
```

Mitigación:

```txt
WASM sandbox
deny-by-default runtime
host import validation
native/FFI marked unsafe
runtime audit
security fuzzing
```

#### Handler malicioso o vulnerable

Riesgo:

```txt
Handlers de paquetes externos filtran secretos o hacen efectos no declarados.
```

Mitigación:

```txt
handler trust levels
handler internal effects
least-privilege grants
secret capability isolation
package advisories
security approval for unsafe handlers
```

#### Supply chain

Riesgo:

```txt
Paquetes con trust metadata falsa, proofs inválidos o artifacts manipulados.
```

Mitigación:

```txt
package hashes
verification report hashes
signing model
registry advisories
reproducible builds
local verification option
```

### Riesgos de ecosistema/adopción

#### Sin ecosistema no hay utilidad

Riesgo:

```txt
Un lenguaje general-purpose necesita stdlib, packages, tooling, docs y adopción.
```

Mitigación:

```txt
stdlib fuerte
package/trust model claro
interop boundaries
generated SDKs
excellent tooling
```

#### Modelo mental demasiado nuevo

Riesgo:

```txt
Usuarios quieren archivos/código tradicional y no entienden graph/ChangeSet workflow.
```

Mitigación:

```txt
human-friendly views
great inspect/diff UX
interactive tutorials
examples end-to-end
docs enfocadas en conceptos
```

### Closed design questions (decisions made)

All previously open design questions are now closed. See [Decisions register](open-questions.md) for the full table. Summary:

| Area | Key decisions |
|------|--------------|
| Toolchain | Rust, chumsky/lalrpop (spike), Z3 + abstract solver API, Wasmtime, Cranelift v1 |
| Formats | BLAKE3 hashing, deterministic CBOR payloads, OpenTelemetry tracing |
| Crypto | AES-256-GCM, Ed25519, Argon2id, X25519 |
| Storage | Async GraphStore API; FoundationDB-compatible model; Postgres + CAS initial backend |
| Packages | Sigstore-style signing; reproducible builds for verified trust |
| Stdlib | Semantic core only; DB is official package; crypto defaults defined |
| Tooling | CLI `ail`; no interactive shell for v1 |

### Validation-required items

These are not open questions. Decisions are made; these spikes confirm the decisions hold under real conditions. Failure triggers a documented re-evaluation, not a scope cut.

#### V-01: Refinement predicate expressiveness and solver performance

```txt
area        verification
impact      high
likelihood  medium
mitigation  abstract solver API; design contracts for composability; bound proof obligations
validation  spike: run Z3 against representative stdlib contracts and real programs
```

#### V-02: Formal semantics sufficiency for critical profile

```txt
area        verification
impact      high
likelihood  medium
mitigation  formal-ish semantics doc; golden tests; property tests per IR stage
validation  spike: define critical profile requirements; verify formal model covers them
```

#### V-03: Cranelift source-map and capability-boundary preservation

```txt
area        compiler
impact      high
likelihood  medium
mitigation  ANF-level checks before backend; semantic source maps required per stage
validation  spike: compile representative programs via Cranelift; verify provenance and capability boundaries in output
```

#### V-04: Postgres + CAS graph store — scale and compile latency on large apps

```txt
area        storage / compiler
impact      high
likelihood  medium
mitigation  compiler consumes immutable snapshots, not repeated live queries; derived indexes; snapshot-aware caching
validation  spike: benchmark compile time on large graph snapshots; if latency is unacceptable, evaluate FoundationDB
```

#### V-05: Handler isolation latency

```txt
area        runtime
impact      medium
likelihood  medium
mitigation  typed binary ABI; batch capability calls; zero-copy where safe; profile-guided optimization
validation  spike: measure isolation overhead under representative handler load
```

#### V-06: Distributed graph collaboration protocol

```txt
area        storage / multi-agent
impact      high
likelihood  medium
mitigation  coordinator serializes commits; stale changes rebase/reverify; ChangeSet log provides ordering
validation  spike: simulate concurrent agent ChangeSets; measure rebase correctness and latency
```

#### V-07: Context-slice signing key management

```txt
area        context server / security
impact      medium
likelihood  medium
mitigation  Sigstore-style keyless signing; structured context is authoritative
validation  spike: define key lifecycle for distributed agents; confirm signing does not break latency budget
```

#### V-08: WASM memory management — RC vs GC

```txt
area        compiler / runtime
impact      high
likelihood  medium
mitigation  decision deferred to implementation spike; resource modes constrain ownership patterns
validation  spike: prototype RC and GC approaches; evaluate performance, correctness, and WASM ABI complexity
```

### Validation milestones (behavioral)

These confirm the system delivers on its thesis. They are not scope cuts — they are proofs the design works.

| # | Validation |
|---|-----------|
| B-01 | LLM generates valid ChangeSets reliably for representative programs |
| B-02 | Context Server slices reduce token/context needs versus raw file reading |
| B-03 | Canonicalization produces stable, deterministic diffs |
| B-04 | Type/effect checker catches real LLM-generated mistakes |
| B-05 | Refinement/contract obligations produce useful, actionable repairs |
| B-06 | Runtime host denies ungranted capabilities under adversarial input |
| B-07 | Profile-bound artifacts prevent prod misuse at the toolchain level |
| B-08 | Semantic rebase handles non-conflicting concurrent graph changes |
| B-09 | Storage GC/compaction keeps growth bounded under realistic project load |
| B-10 | Structural diff UX makes ChangeSet review understandable to humans |

#### Compiler

```txt
1. Exact ANF representation and serialization.
2. Custom SSA vs Cranelift/LLVM IR.
3. WASM ABI layout for records, variants, Result/Option, handles.
4. Memory management strategy for WASM.
5. Translation validation requirements for prod/critical.
```

#### Runtime

```txt
1. Binary encoding for host.call payloads: CBOR, MessagePack, canonical JSON, or custom.
2. Whether to use WASI underneath or hide it fully behind host ABI.
3. Handler execution isolation model.
4. Distributed tracing standard across capability calls.
5. Sync vs async capability call typing.
```

#### Storage

```txt
1. Concrete backend: embedded DB, CAS filesystem, object DB, or hybrid.
2. Hash algorithm and canonical serialization.
3. Distributed collaboration protocol for graph branches.
4. Default local retention policy.
5. Protected audit archive strategy.
```

#### Context Server

```txt
1. Exact query syntax: line-oriented DSL, RPC JSON, or both.
2. How summaries are generated and checked against structured data.
3. Whether context slices should be signed for distributed agents.
4. Default budgets by model/context size.
5. Safe exposure policy for runtime/audit context.
```

#### Packages

```txt
1. Registry protocol and package signing.
2. Whether verified packages require reproducible builds.
3. Federated trust across organizations.
4. Local proof checking vs trusted remote verification.
5. Package yanking while preserving old builds.
```

#### Stdlib

```txt
1. How large v1 stdlib should be.
2. Whether database capability is stdlib core or official package.
3. Exact crypto safe defaults.
4. Async runtime placement: std.concurrent vs std.runtime.
5. Stdlib versioning independent from language/Core IR.
```

#### Tooling

```txt
1. Final CLI name.
2. Whether interactive shell is required for first full product release.
3. How editor edits convert into ChangeSets.
4. Default human approval UX.
5. Whether local experiments can disable persistent graph storage.
```

### Validaciones necesarias

Estas validaciones no son “MVP” del producto; son pruebas de riesgo que deben hacerse durante la implementación completa.

```txt
1. LLM can generate valid ChangeSets reliably.
2. Context Server slices reduce token/context needs versus file reading.
3. Canonicalization produces stable diffs.
4. Type/effect checker catches real LLM mistakes.
5. Refinement/contract obligations produce useful repairs.
6. Runtime host denies ungranted capabilities.
7. Profile-bound artifacts prevent prod misuse.
8. Semantic rebase handles non-conflicting graph changes.
9. Storage GC/compaction controls growth.
10. Tooling UX makes structural diffs understandable.
```

### Risk register format

Each risk entry follows this shape:

```txt
risk <id>
  title "..."
  area compiler|runtime|verification|storage|ux|security|ecosystem
  impact low|medium|high|critical
  likelihood low|medium|high
  mitigation "..."
  validation "..."
  status open|mitigated|accepted|closed
end
```

### Rule

```txt
No serious unknown should remain hidden in prose.
Every unresolved concern becomes a tracked risk or validation spike.
Closed decisions live in the decision log and decisions register.
```
