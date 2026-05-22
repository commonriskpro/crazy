# Architecture overview

<!-- Implementation Status: core workspace implements the major architecture slices as milestones; some sections remain full-design intent rather than complete feature parity. -->

> Full extracted design. Start here, then use [README](../README.md) for topic navigation.

## Original preface

# Borrador: lenguaje de programación AI-native

> Estado: archivo histórico/raw. La versión organizada vive en `README.md` y `docs/`. Este archivo conserva el desarrollo completo de la conversación para auditoría y detalle.

Este documento resume la idea conversada: diseñar un lenguaje general-purpose creado para que lo escriban LLMs, no humanos. La interacción humana sería conversacional; la fuente real del programa sería una representación semántica verificable, no archivos de texto tradicionales.

## Tesis principal

No queremos “otro Python” ni “un DSL legible por humanos”. Queremos cambiar qué significa programar:

```txt
Humano expresa intención
  ↓
IA propone cambios semánticos
  ↓
Toolchain verifica
  ↓
Programa vive como grafo/IR
  ↓
Compila a WASM + runtime de capabilities
```

La IA no debería editar archivos como hoy. Debería emitir operaciones verificables sobre un sistema semántico.

## Principios de diseño

| Principio | Decisión |
|---|---|
| Source of truth | El programa vive como Semantic Graph / Core IR, no como texto humano. |
| Escritura por LLM | El LLM emite ChangeSets: operaciones pequeñas, canónicas y reparables. |
| Verificación | La IA propone; el verificador acepta o rechaza. |
| General-purpose | El lenguaje debe soportar funciones, tipos, módulos, efectos, errores, concurrencia, FFI y paquetes. |
| Efectos | Todo acceso al mundo externo debe ser explícito mediante un sistema extensible de capabilities. |
| Hardcoded mínimo | El lenguaje hardcodea mecanismos fundamentales, no servicios concretos como DB, HTTP o Stripe. |
| Compilación | Primero a Core IR verificable; luego a WASM + manifest + reporte de verificación. |
| Contexto | El LLM consulta slices semánticos, no lee 40 archivos. |

## Componentes principales

```txt
Conversation Layer
  ↓
Intent Compiler
  ↓
AI Change Language
  ↓
Semantic Program Graph
  ↓
Verified Core IR
  ↓
Verifier / Type Checker / Effect Checker
  ↓
WASM + Runtime Host
```

## Qué se hardcodea y qué no

Todo lenguaje general-purpose hardcodea una física básica. La pregunta correcta no es si hardcodear, sino qué merece ser parte del lenguaje.

Hardcoded en el lenguaje:

```txt
- valores
- funciones
- tipos
- módulos
- contratos
- referencias estables
- cambios transaccionales
- effects/capabilities como mecanismo
- verificación/reportes
```

No hardcoded:

```txt
- database
- HTTP
- Stripe
- filesystem concreto
- cloud provider
- framework web
- proveedor LLM
```

Analogía:

```txt
Hardcodear gravedad: sí.
Hardcodear una silla específica: no.
```

El lenguaje define leyes. Las librerías y runtimes definen cosas del mundo.

## Relación con lenguajes existentes

Candidatos/inspiraciones:

| Proyecto | Qué aporta |
|---|---|
| Unison | Programa como codebase semántica, referencias content-addressed, refactors más seguros. |
| Koka | Sistema de efectos explícitos. |
| Lean / F* / Idris | Verificación, pruebas, invariantes. |
| MLIR | Infraestructura para IRs y compiladores. |
| WASM | Target ejecutable portable y sandboxeado. |

La opción más cercana filosóficamente para investigar/forkear sería Unison, pero habría que mutarlo fuerte.

## Estrategia de diseño

Decisión de proceso:

```txt
Diseño completo desde el inicio.
Implementación por fases después.
```

No queremos dejar decisiones fundamentales “para más adelante”, porque modificar la arquitectura profunda después sería caro. Lo que sí puede hacerse por etapas es la implementación.

Regla:

```txt
Nada fundamental queda sin diseñar.
Solo puede quedar sin implementar temporalmente.
```

## Matriz de diseño completo

| Área | Decisión necesaria | Estado actual | Implementación sugerida |
|---|---|---|---|
| Source of truth | Programa como Semantic Graph / Core IR, no como source files clásicos. | Decidido | Producto completo |
| AI Change Language | Formato exacto de ChangeSets que escriben los LLMs. | Decidido | Producto completo |
| Semantic Core IR | Primitivas completas del lenguaje interno verificable. | Decidido | Producto completo |
| Compiler IR | ANF como compiler IR principal; SSA como backend artifact. | Decidido | Producto completo |
| Type system | Primitives, records, variants, generics, refinements, interfaces, resources. | Decidido | Producto completo |
| Error model | `Result`, `Option`, `PatchField`, sin excepciones implícitas. | Decidido | Producto completo |
| Effects/capabilities | Efectos extensibles, handlers, runtime grants. | Decidido | Producto completo |
| Contracts | `requires`, `ensures`, `invariant`, proof obligations. | Decidido | Producto completo |
| Verification model | Estados explícitos y profiles. | Decidido | Producto completo |
| Refactor model | Refactors como operaciones semánticas verificadas. | Decidido | Producto completo |
| Package system | Trust, imports/exports, capabilities, assumptions, unsafe surface. | Decidido | Producto completo |
| Storage/versioning | Graph store, snapshots, ChangeSet history, hashes, GC/retention. | Decidido | Producto completo |
| Context Server | Semantic slices hash-bound para LLMs. | Decidido | Producto completo |
| Runtime host | WASM host deny-by-default con capabilities, handlers, limits, audit. | Decidido | Producto completo |
| Executable target | WASM primero; native posible después. | Decidido | Producto completo |
| Concurrency | `can_suspend` effect + task/channel primitives. | Decidido | Producto completo |
| Resource lifecycle | `Handle<Resource, Mode>` con `Affine`, `Linear`, `Shared`. | Decidido | Producto completo |
| FFI/boundaries | Boundaries con trust, contracts, assumptions, approvals. | Decidido | Producto completo |
| Standard library | Semántica común + capabilities definitions. | Decidido | Producto completo |
| Tooling | CLI/workflows sobre graph snapshots y ChangeSets. | Decidido | Producto completo |
| Security model | Least privilege, deny-by-default, audit, package/runtime trust. | Decidido | Producto completo |
| LLM repair loop | Diagnósticos estructurados con repair options. | Decidido | Producto completo |

## Implementación

La implementación debe seguir el diseño de producto completo. Puede secuenciarse internamente por subsistemas, pero no debe presentarse como un MVP que recorta la visión o cambia la arquitectura.

### Implementation Notes

Current implementation preserves the architectural thesis: ChangeSets, Semantic Graph, verification reports, compiler pipeline, WASM runtime host, package trust, context slices, stdlib, remote bundles, and dogfood examples exist as Rust crates.

Known deliberate deviations from the original shape:

- Context Server is currently an in-process API, not a network server.
- WASM ABI is intentionally narrow and i64-oriented for executable milestones.
- Native backend proves Cranelift/provenance but emits trap stubs.
- CLI workflow exists as crate/skeleton plus tests, not every command listed in tooling design.

These deviations preserve the design direction while keeping implementation risk bounded.

## Crate map

| Crate | Role | Key dependencies |
|-------|------|-----------------|
| `ail-core` | Semantic Graph IR, type system primitives, node/edge/effect/contract types | (no workspace deps) |
| `ail-change` | AI Change Language parser, canonicalizer, apply engine, ACL format | `ail-core` |
| `ail-compiler` | Core IR → ANF lowering, WASM emit, native emit, source maps | `ail-core`, `ail-change`, `ail-verify` |
| `ail-verify` | Type checker, effect checker, proof obligations, verification reports | `ail-core`, `ail-change` |
| `ail-storage` | Object store (memory, file, Postgres), snapshot envelopes, graph store, CBOR codec | `ail-core` |
| `ail-context` | Context Server, semantic query engine, hash-bound context slices, derived index cache | `ail-core`, `ail-storage`, `ail-change` |
| `ail-runtime` | WASM host (Wasmtime), capability manifest, runtime profiles, resource limits, audit log | `ail-compiler` |
| `ail-stdlib` | Standard library registry, capability definitions, built-in module metadata | `ail-core` |
| `ail-package` | Package manifest, lockfile, trust levels, capability policy enforcer, registry client | `ail-core`, `ail-storage` |
| `ail-remote` | Agent identity (Ed25519), ObjectBundle, SignedContextSlice, RemoteChangeSet; optional AES-256-GCM/Argon2id/X25519 primitives under `feature = "crypto"` | `ail-storage`, `ail-change`, `ail-context` |
| `ail-coordinator` | Multi-agent ChangeSet serialization via `tokio::sync::Mutex`; semantic rebase; conflict classification; `verify_remote_submission` | `ail-core`, `ail-change`, `ail-remote` |
| `ail-dogfood` | Self-referential validation: builds `SemanticGraph` and `ChangeSet` that model the toolchain's own types; projects stdlib registry to graph | `ail-core`, `ail-change`, `ail-stdlib` |
| `ail-testkit` | Shared test fixtures: `make_semantic_graph()`, `make_large_graph(n)`, `make_snapshot_envelope()`, `fixture!()` macro, re-exports of in-memory store types | `ail-core`, `ail-storage` |
| `ail-cli` | `ail` binary: full CLI surface, `StoreHandle` abstraction (memory/file/Postgres), command dispatch | all crates |
