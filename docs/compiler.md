# Compiler pipeline

<!-- Status: Implemented subset. Graph-to-Core-IR-to-ANF-to-WASM exists with hash chains and source maps for the current executable surface. Native body lowering covers arithmetic, control-flow (If/Loop/Match), data structures (records/variants/lists/tuples), text literals, and EffectCall; concurrency primitives, channels, dynamic dispatch, and resource acquire/release remain trap stubs or future work. Prod/critical backend profiles now require per-binding ChangeSet provenance in source maps, but full profile/report matching and translation validation remain future work. -->

> Target design. Current implementation scope is called out in the status note and Implementation Notes. Related: [Core IR](core-ir.md), [Verification](verification.md), [Runtime](runtime.md), [Storage](storage.md).

## Artefacto ejecutable

El target ejecutable principal sería:

```txt
program.wasm
program.capabilities.json
program.verification.json
```

Ejemplo:

```txt
checkout.wasm
checkout.capabilities.json
checkout.verification.json
```

El WASM corre en un runtime host que controla efectos externos.

## Compiler pipeline: target design

El compiler convierte snapshots verificados del Semantic Graph en artefactos ejecutables y derivados.

### Tesis

```txt
El compilador no compila texto.
Compila un snapshot del Semantic Graph aceptado para un verification profile.
```

Regla:

```txt
Compiler compiles accepted verification reports for the target profile.
The artifact is profile-bound.
A draft artifact cannot be promoted to prod without prod verification.
```

Esto permite compilar `unverified` en profiles relajados (`draft/dev/test`) si la policy lo acepta, pero impide fingir que ese artefacto es production-safe.

### Pipeline

```txt
Semantic Graph Snapshot
  ↓ select entrypoints/modules
Semantic Core IR
  ↓ normalize/check metadata
ANF IR
  ↓ optimize/lower
ANF IR (optimized)
  ↓ backend (Cranelift / wasm-encoder)
WASM / native
  + capabilities manifest
  + semantic source maps
  + artifact metadata
  + verification-linked hashes
```

Note: There is no compiler-produced SSA IR stage. SSA is managed internally by Cranelift during backend compilation and is not a named artifact in the pipeline.

### Inputs

```txt
graph_snapshot
entrypoints
target_profile
verification_report
runtime_profile
package_lock
compiler_config
backend_target
```

### Outputs

```txt
program.wasm or native binary
capabilities_manifest
semantic_source_map
artifact_manifest
compiler_report
runtime_profile_link
debug/profiling metadata
```

### Profile-bound artifacts

Every executable artifact records:

```txt
target_profile
verification_report_hash
graph_snapshot_hash
core_ir_hash
anf_ir_hash
capabilities_manifest_hash
compiler_version
```

Example:

```txt
artifact_manifest checkout.wasm
  profile draft
  verification_report ver_draft_123
  states
    proven_count 42
    runtime_checked_count 3
    assumed_count 1
    unverified_count 2
    unsafe_count 0
    failed_count 0
  end
end
```

Rules:

```txt
1. Artifact profile must match verification report profile.
2. Prod runtime rejects draft/dev/test artifacts.
3. Artifact cannot be promoted by relabeling; it must be reverified/recompiled for target profile.
```

### Stage 1: Graph selection

Selects what to compile:

```txt
entrypoints
public exports
reachable dependencies
required handlers
runtime profile bindings
```

Checks:

```txt
entrypoints exist
selected snapshot matches report
dependencies resolved
package trust accepted for profile
```

### Stage 2: Graph → Semantic Core IR

Converts graph objects into Core IR definitions:

```txt
TypeDef
FunctionDef
CapabilityDef
HandlerDef
ContractDef
InvariantDef
InterfaceDef
ImplDef
BoundaryDef
```

Preserves:

```txt
NodeId
stable identity
contracts
effects
refinements
resource modes
trust metadata
provenance
```

### Stage 3: Core IR validation

Runs or consumes verification for:

```txt
type checking
effect checking
contract checking
resource/concurrency checks
boundary checks
```

The compiler does not override verifier decisions. It requires:

```txt
verification_report.status = accepted for target_profile
```

### Stage 4: Core IR → ANF

ANF makes evaluation order explicit.

Example:

```txt
charge(cart_total(read_cart(cartId)))
```

becomes:

```txt
let cart = EffectCall database.read:Cart(cartId)
let total = Call fn.cart_total(cart)
let payment = EffectCall payment.charge:PaymentProvider(total)
return payment
```

ANF responsibilities:

```txt
effect ordering
runtime check insertion
resource acquire/release ordering
pattern match lowering
short-circuit lowering
Dyn dispatch lowering
handler call lowering
task/channel operation ordering
```

ANF is the main compiler IR because effect order is structural.

### Stage 5: ANF checks

Checks:

```txt
effect order matches verified semantics
runtime checks are present
resource lifecycle preserved
no hidden capability calls
control flow preserves contracts
debug metadata preserved
```

### Stage 6: ANF optimization

Allowed optimizations must preserve semantics and metadata.

Examples:

```txt
dead pure code elimination
constant folding
inlining pure functions
common subexpression elimination for pure expressions
contract-based simplification
```

Restricted optimizations:

```txt
cannot reorder EffectCall across observable boundaries
cannot remove runtime checks unless proven redundant
cannot widen capabilities
cannot drop audit/debug/provenance metadata required by profile
cannot change resource release order unsafely
```

### Stage 7: Backend lowering / emission

The optimized ANF IR is passed directly to the backend. There is no compiler-produced SSA IR; SSA is managed internally by Cranelift during backend compilation.

```txt
ANF IR (optimized)
  ↓ backend (Cranelift / wasm-encoder)
WASM / native object file
```

Backend responsibilities:

```txt
instruction selection
register/value allocation
lowering to WASM / native
source map and provenance annotation
capability call boundary preservation
```

Rules:

```txt
Backend must preserve semantic source maps.
Backend must preserve capability call boundaries.
Backend must preserve runtime checks.
Backend must preserve artifact hash provenance.
SSA is Cranelift-internal; it is not a named compiler stage or a produced artifact.
```

### Stage 8: Backend

Initial target:

```txt
WASM
```

Available targets (Phase 17+):

```txt
WASM (primary)
native object file via Cranelift (Phase 17)
```

The native Cranelift path is implemented in `crates/ail-compiler/src/native.rs`
via `emit_native(anf: &AnfIr) -> Result<NativeArtifact, CompileError>`.
It produces platform-native ELF/Mach-O/COFF object files with:
- Full provenance (`BTreeMap<NodeRef, u64>` — byte offsets in code section).
- Capability manifest (same schema as WASM backend).
- Sealed hash chain: `native_hash = blake3(anf_ir_hash || native_bytes)`.

Phase 8 expression lowering now covers this implemented subset. The following
ANF expression families produce real Cranelift IR instead of trap stubs:
- Extended arithmetic: `i64.div_s`, `i64.rem_s`, `i64.and`, `i64.or`, comparisons, `i64.neg`, `i64.eqz`
- Control flow: `If`, `ShortCircuitAnd`, `ShortCircuitOr`, `Seq`, `RuntimeCheck`
- Loops: `Loop`, `Break`, `Continue`, `WhileLoop`
- Pattern matching: `Match` (i64/bool arms + wildcard)
- Text literals: `Literal(Text)` via Cranelift data section + packed ptr/len
- Memory: `RecordNew`, `FieldGet`, `FieldUpdate` (stack-allocated, 8 bytes/field)
- Variants: `VariantNew` (16-byte stack slot, FNV tag discriminant, optional payload)
- Collections: `ListNew` (length header + elements), `TupleNew` (elements only)
- Effects: `EffectCall` (calls imported `host_call(I64×6)→I64`)

`Lambda` compiles without closure capture: params are bound as I64 arguments,
the body is lowered, and the function address is returned as I64. Closure
captures are deferred to Phase 9+.

Remaining as `ail_runtime_call` dispatch stubs (Phase 9+): `TaskSpawn`,
`TaskAwait`, `TaskCancel`, `TaskGroup`, `ChannelNew`, `ChannelSend`,
`ChannelReceive`, `Select`, `Timeout`, `Dispatch`, `ResourceAcquire`,
`ResourceRelease`. These emit a real Cranelift call to the imported
`ail_runtime_call` function; the runtime implementation is not yet provided.

Records/lists/variants are stack-allocated (not heap). Returned pointers are
invalid after function return. Full heap model deferred to Phase 9.

The WASM pipeline is unaffected by native backend changes.

#### Native-1 binary/object smoke: honest scope

**What `emit_native` produces:** a platform-native OBJECT FILE (ELF on Linux,
Mach-O on macOS, COFF on Windows). It is NOT a linked, runnable executable.

```txt
emit_native(anf) → NativeArtifact {
    native_bytes:            ELF / Mach-O / COFF object file bytes
    provenance:              BTreeMap<NodeRef, u64>  (code section byte offsets)
    source_map:              SourceMapEntry per binding, native_offset populated
    capabilities_manifest:   same schema as WASM backend
    hash_chain.native_hash:  blake3(anf_ir_hash || native_bytes)
    source_map_json:         JSON sidecar for program.source_map.json
    artifact_manifest_json:  JSON sidecar for program.artifact.json
}
```

**Current limitations (Native-1 slice):**

```txt
- No linker invocation. A system linker (cc, lld) is required to produce
  a runnable executable from the emitted object file.
- No runtime host: imported stubs (host_call, __ail_malloc, ail_runtime_call)
  must be supplied at link time.
- No self-hosting: ail-compiler itself is not compiled by ail-compiler.
- Lambda compiles: params bound, body lowered, address returned as I64.
  Closure captures are deferred to Phase 9+.
- Concurrency, dynamic dispatch, resource lifecycle, and channel primitives
  dispatch via imported `ail_runtime_call`; the runtime implementation is not
  yet provided (Phase 9+). Arithmetic, control-flow, loops, match, text
  literals, records/variants/lists/tuples, EffectCall, and Lambda produce
  real Cranelift IR.
- Records/lists/variants are stack-allocated. Returned pointers are invalid
  after function return. Full heap model deferred to Phase 9.
```

**Tests proving the current object smoke path (`tests/native_object_smoke_tests.rs`):**

```txt
- Magic bytes validation: emitted bytes start with the platform-native
  object file magic (ELF 7F 45 4C 46, Mach-O CF FA ED FE).
- Determinism: same AnfIr → byte-identical native_bytes and native_hash.
- Provenance: provenance map covers every binding with correct NodeRefs
  and monotonically non-decreasing offsets.
- Hash chain: native_hash = blake3(anf_ir_hash || native_bytes).
- Source map: native_offset populated for every binding after emit_native.
- Sidecars: source_map_json and artifact_manifest_json are valid JSON.
- Wave 6B gate: prod/critical profiles reject missing change_set; pass when populated.
- Arithmetic: i64.add/sub/mul emit real Cranelift IR, not trap stubs.
```

**WASM/native parity smoke (`tests/wasm_native_parity_smoke_tests.rs`):**

```txt
- Both backends accept the same AnfIr without error.
- Provenance maps cover the same NodeRefs (structural parity).
- Source maps have the same entry count and node_ids.
- Hash chains are independent: emit_wasm does not set native_hash;
  emit_native does not set wasm_hash.
- wasm_hash ≠ native_hash for the same input (different formulas/content).
- Each backend populates only its own offset field (wasm_offset or native_offset).
- Simple expressions (int, bool, i64.add, If) compile in both backends.
- i64.sub produces different output than Placeholder in both backends.
```

**Path to real binary / self-hosting:**

```txt
Phase 9:  Heap model — __ail_malloc supplied by runtime; records/variants/lists
          survive function return.
Phase 9:  Linker integration — emit_native output linked with cc/lld + ail_runtime.a
          to produce a runnable native binary.
Phase 10: ABI stabilization — ail_runtime_call, host_call signatures frozen.
Phase 11+: Full expression body lowering — Lambda, closures, concurrency.
Phase N:  Self-hosting — ail-compiler's own source compiled by ail-compiler.
          Requires: full language surface + runtime + linker + bootstrapping sequence.
```

WASM output:

```txt
program.wasm
program.capabilities.json
program.source_map.json
program.artifact.json
```

Backend rules:

```txt
WASM imports must be host capability calls.
No direct world access.
No import absent from capabilities manifest.
Memory/table exports follow runtime ABI.
```

### Capability manifest generation

Manifest is generated from verified effects + handler transformations.

Includes:

```txt
required capabilities
optional capabilities
handler requirements
runtime checks
profile grants expected
```

Manifest must match verification report.

### Semantic source maps

Source maps point back to semantic graph, not only text lines.

Implemented subset: `lower_to_anf_with_graph` enriches source map entries with
available `ChangeSet`, derived `BlockRef`, derived `ContractRef`, first
`EffectRef`, and first `RuntimeCheckRef`. `emit_wasm_with_profile` and
`emit_native_with_profile` clone those entries, add backend byte offsets, emit
JSON sidecars, and seal `source_map_hash` from deterministic CBOR. For `prod`,
`production`, and `critical` profiles, codegen rejects artifacts whose emitted
source map entries lack `ChangeSet` provenance. Other references remain optional
because not every graph node has a contract, effect, runtime check, or proof
obligation.

Fields:

```txt
wasm_offset / native_offset
NodeId
BlockRef
ChangeSet provenance
ContractRef
EffectRef
ProofObligationRef
RuntimeCheckRef
```

Purpose:

```txt
debugging
profiling
runtime error mapping
LLM repair context
```

### Diagnostics

Compiler diagnostics use the repairable diagnostics protocol.

Examples:

```txt
E_VERIFICATION_REPORT_PROFILE_MISMATCH
E_ARTIFACT_HASH_MISMATCH
E_RUNTIME_CHECK_MISSING
E_CAPABILITY_MANIFEST_MISMATCH
E_EFFECT_REORDERING_INVALID
E_SOURCE_MAP_LOST_METADATA
```

### Hash/provenance chain

Every stage produces hash-linked artifacts (`StageHashes`):

```txt
graph_snapshot_hash        pipeline input — SemanticGraph
verification_report_hash   pipeline input — VerificationReport
core_ir_hash               set by lower_to_core_ir
anf_ir_hash                set by lower_to_anf (optional until ANF stage)
wasm_hash                  set by emit_wasm (WASM backend)
native_hash                set by emit_native (native backend)
source_map_hash            set by backend after populating offsets
artifact_manifest_hash     set by artifact manifest emission
```

`capabilities_manifest_hash` is a field of `ArtifactManifest`, not `StageHashes`.
`canonical_change_hash` is a ChangeSet-level concept (stored in approval records), not a compiler stage hash.
There is no `ssa_ir_hash` — no SSA IR artifact is produced by the compiler.

Rule:

```txt
If any upstream hash changes, downstream artifacts must be regenerated/reverified.
```

### Incremental compilation

Compiler can compile affected slices.

Uses:

```txt
structural_diff
dependency graph
effect graph
contract/invariant impact index
package lock
```

Rules:

```txt
incremental artifacts must preserve same verification guarantees
stale dependency index invalidates incremental result
public API/effect changes widen affected set
```

### Debug/profile mode

Profiles affect compiler output.

```txt
draft/dev:
  keep rich debug metadata
  allow unverified if report accepted
  include repair context

prod:
  optimized
  keep audit/source map metadata required by policy
  reject draft-only checks/handlers

critical:
  preserve maximum auditability
  restrict aggressive optimizations if proof metadata would be lost
```

### Compiler trust

Compiler is part of trusted computing base.

Required:

```txt
versioned compiler
reproducible builds target
self-tests
golden lowering tests
IR verifier per stage
artifact hash validation
```

Long-term option:

```txt
translation validation
```

Meaning: after optimization/codegen, validate output preserves ANF/Core semantics for supported fragments.

### Final rules

```txt
1. Compiler input is graph snapshot + accepted verification report, not text.
2. Accepted means accepted for the target profile.
3. Artifacts are profile-bound.
4. Draft/dev/test artifacts cannot be promoted to prod by relabeling.
5. ANF is the main compiler IR.
6. SSA is Cranelift-internal; there is no compiler-produced SSA artifact.
7. Every lowering preserves provenance/source maps.
8. Runtime checks and capability calls cannot disappear.
9. Optimizations cannot reorder observable effects unsafely.
10. Every artifact is hash-linked to the verification report.
```

### Compiler technology decisions

| Area | Decision |
|------|----------|
| Toolchain language | Rust |
| Parser | Hand-written ACL parser and hand-written expression parser for the current subset; previous `chumsky`/`lalrpop` placeholder is reversed. |
| ANF serialization | Exact format decided during implementation; must be deterministic and schema-versioned |
| SSA / backend | Cranelift for WASM v1. LLVM/native added later if needed. SSA is managed internally by Cranelift — no compiler-produced SSA artifact exists. |
| WASM ABI layout | Implemented subset: records (i64 fields at 8-byte offsets), variants/Option/Result (i32 tag at offset 0, i64 payload at offset 8), lists (i64 count at offset 0, i64 elements). Descriptors in `WasmArtifact::export_types`. Structured EffectCall results via `host_call_write`. Rich ABI/value-layout parity remains validation work. |
| Memory management | Reference counting for normal heap values plus ownership/affine/linear rules for resource handles. No general GC in v1; cycles are rejected initially and revisited only if real programs justify tracing support. See [Decision log](decision-log.md#parallel-implementation-unblock-decisions). |
| Translation validation | Required for `prod`/`critical`; scope per profile. Cranelift source-map and capability-boundary preservation is a validation spike — see [Risks](risks.md) V-03 |
| Native backend | Cranelift implemented subset. `emit_native` produces ELF/Mach-O/COFF with provenance + capability manifest. Phase 8 lowering covers arithmetic, control-flow, loops, match, text literals, records/variants/lists/tuples, EffectCall, and Lambda (no closure capture). Remaining `ail_runtime_call` dispatch stubs: concurrency, dynamic dispatch, resource lifecycle (Phase 9+). |

### Implementation Notes

- `emit_wasm` emits executable bodies for integer/bool/control-flow expressions, `AnfExpr::EffectCall` (via `ail/host_call`), and all compound types: `RecordNew`, `VariantNew`, `ListNew`, `TupleNew`.
- A typed WASM/runtime boundary subset exists: `WasmArtifact::export_types` maps each exported binding name to its `WasmTypeDescriptor`, and the runtime uses `ValueDecoder::decode` via `RuntimeInstance::invoke_typed` to reconstruct `StructuredValue` from linear memory. This is not full rich ABI/value-layout parity.
- Structured `EffectCall` results (where the binding body is a Record/Variant/List type) use `ail/host_call_write` instead of `ail/host_call`; the host writes response bytes to `result_buffer_offset` in WASM memory. `WasmArtifact::result_buffer_offset` exposes this offset for callers.
- Memory layout: records store i64 fields at 8-byte offsets from the base pointer; variants store an i32 tag at offset 0 and an i64 payload at offset 8; lists store an i64 count at offset 0 followed by i64 elements.
- `emit_native` (Phase 8) now emits real Cranelift IR for arithmetic, control-flow (If/Loop/Match/ShortCircuit/Seq/RuntimeCheck), text literals, memory (records/variants/lists/tuples via stack slots), EffectCall (imported `host_call`), and Lambda (params bound, body lowered, address returned as I64; no closure capture). Concurrency and resource primitives (`TaskSpawn`, `ChannelSend`/`ChannelReceive`, `Dispatch`, `ResourceAcquire`/`ResourceRelease`, etc.) dispatch via imported `ail_runtime_call`; the runtime implementation is not yet provided (Phase 9+).
- Source-map hardening currently validates per-binding `change_set` only for `prod`/`production`/`critical` backend profiles. It does not yet prove semantic equivalence after optimization or enforce full verification-report/profile matching.

Code references: `crates/ail-compiler/src/expr_parser.rs`, `core_ir.rs`, `anf.rs`, `wasm.rs`, `native.rs`.
