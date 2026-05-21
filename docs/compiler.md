# Compiler pipeline

> Full extracted design. Related: [Core IR](core-ir.md), [Verification](verification.md), [Runtime](runtime.md), [Storage](storage.md).

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

## Compiler pipeline: propuesta completa

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
SSA IR
  ↓ backend
WASM / native
  + capabilities manifest
  + semantic source maps
  + artifact metadata
  + verification-linked hashes
```

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

### Stage 7: ANF → SSA

SSA is a backend artifact.

```txt
ANF IR
  ↓ mechanical lowering
SSA IR
```

SSA responsibilities:

```txt
control/dataflow
backend optimization
register/value allocation
lowering to WASM/native
```

Rules:

```txt
SSA must preserve semantic source maps.
SSA must preserve capability call boundaries.
SSA must preserve runtime checks.
SSA must preserve artifact hash provenance.
```

### Stage 8: Backend

Initial target:

```txt
WASM
```

Future target:

```txt
native via LLVM/Cranelift/custom backend
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

Every stage produces hash-linked artifacts:

```txt
graph_snapshot_hash
canonical_change_hash
verification_report_hash
core_ir_hash
anf_ir_hash
ssa_ir_hash
wasm_hash
capabilities_manifest_hash
source_map_hash
artifact_manifest_hash
```

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
6. SSA is backend artifact.
7. Every lowering preserves provenance/source maps.
8. Runtime checks and capability calls cannot disappear.
9. Optimizations cannot reorder observable effects unsafely.
10. Every artifact is hash-linked to the verification report.
```

### Open design questions

```txt
1. Exact ANF representation and serialization.
2. Whether SSA is custom or delegated to Cranelift/LLVM IR.
3. Exact WASM ABI layout for values, records, variants, Result/Option.
4. GC/memory management strategy for WASM target.
5. How much translation validation is required for prod/critical.
6. Native backend priority and sandboxing model.
```
