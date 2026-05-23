# Verification model

<!-- Status: Implemented subset. Checker/report infrastructure exists across explicit checker modules; the full target pipeline remains broader than current validation coverage. -->

> Target design. Current implementation scope is called out in status notes and Implementation Notes. Related: [Type system](type-system.md), [AI Change Language](change-language.md), [Runtime](runtime.md), [Compiler](compiler.md).

## Verificación

No prometemos verificación absoluta para todo programa general-purpose. Eso no existe.

Sí prometemos que todo programa compile con un mapa explícito:

```txt
Verified:
- type safety
- declared effects only
- contracts proven
- refactor invariants preserved

Assumed:
- external API honors contract
- database isolation level behaves as declared

Unverified:
- native extension
- external AI model response quality
```

La revolución no es que la IA sea infalible. La revolución es que la IA no pueda esconder incertidumbre.

## Refactor safe

Un refactor sería una transformación que preserva comportamiento observable.

```txt
Refactor = preserva contratos observables
Feature = cambia contratos observables
Migration = cambia modelo/comportamiento declarado
```

El verificador compara antes y después:

- tipos públicos
- efectos
- contratos
- invariantes
- grafo de dependencias
- equivalencia semántica cuando sea posible
- snapshots/property tests cuando la prueba formal no alcance

Ejemplo de reporte:

```txt
Refactor report:
- Public API preserved: yes
- Effects preserved: yes
- Contracts preserved: yes
- Pure equivalence proven: 7/9 functions
- Behavioral snapshots passed: 42/42
- Unverified assumptions: PaymentProvider boundary unchanged
```

## Verification model: diseño propuesto

El verifier no intenta demostrar “todo” de forma mágica. Clasifica cada claim, obligación y riesgo en estados explícitos.

### Decisión base

```txt
Nada queda “quizás”.
Todo claim termina clasificado.
```

Estados:

```txt
proven
runtime_checked
assumed
unverified
unsafe
failed
```

### Política default

Decisión:

```txt
Strict by default.
Relaxed only by explicit policy/profile.
```

Comportamiento default:

```txt
proven           pasa
runtime_checked  pasa solo si el check runtime está materializado
assumed          pasa solo con boundary explícito + policy/approval
unverified       bloquea por default en public/prod boundaries
unsafe           bloquea salvo approval explícita fuerte
failed           bloquea siempre
```

Por contexto:

```txt
Public API / production:
  unverified = block
  unsafe = block por default; prod solo permite security exception explícita y fuerte
  assumed = approval required

Internal draft:
  unverified = allowed with warning if policy allows
  unsafe = block
  assumed = allowed if annotated/policy allows

Tests/prototypes:
  unverified = allowed by policy
  unsafe = still approval/block
```

Build profiles:

```txt
dev
test
staging
prod
```

Regla:

```txt
unverified no puede convertirse en el nuevo any.
```

### Estados de verificación

Cada claim/obligación se clasifica en un estado explícito.

#### proven

Significa:

```txt
El verifier pudo demostrar la propiedad sin depender de runtime
ni de una nueva confianza externa.
```

Fuentes válidas:

```txt
type checker
effect checker
SMT solver
proof rule
stdlib contract verified
structural analysis
```

Ejemplos:

```txt
type safety proven
effect declared proven
cart.total >= 0 proven by refinements
resource released proven
```

#### runtime_checked

Significa:

```txt
No se probó estáticamente,
pero existe un check real insertado/materializado en runtime.
```

Ejemplo:

```txt
external_payload.email -> Email
```

Vía decoder:

```txt
validate Email
```

Regla:

```txt
Si no hay check materializado, no cuenta como runtime_checked.
```

#### assumed

Significa:

```txt
El sistema acepta una verdad por boundary/contrato externo explícito.
```

Ejemplos:

```txt
Stripe honors idempotency key
Postgres transaction isolation behaves as declared
System clock is monotonic
```

Debe tener:

```txt
boundary
contract
trust level
approval/policy
```

No puede ser “supongo porque sí”.

#### unverified

Significa:

```txt
El sistema no pudo probar, runtime-checkear,
ni convertir en assumption aceptada.
```

Ejemplos:

```txt
custom native extension preserves memory safety
complex algorithm preserves fairness
external AI model returns truthful answer
```

Por default bloquea en public/prod boundaries.

#### unsafe

Significa:

```txt
Este cambio puede romper garantías del lenguaje/verifier.
```

Ejemplos:

```txt
raw pointer
unchecked FFI
disable runtime check
grant broad filesystem access
bypass capability system
```

Requiere approval fuerte.

#### failed

Significa:

```txt
Se encontró una contradicción o violación real.
```

Ejemplos:

```txt
type mismatch
undeclared effect
contract false
resource leak
use after release
ambiguous impl
```

Bloquea siempre.

#### Prioridad de estados

Si algo podría clasificarse en varios estados, gana el peor:

```txt
failed > unsafe > unverified > assumed > runtime_checked > proven
```

Ejemplo: si hay runtime check pero también usa unsafe boundary sin approval, el claim queda `unsafe`, no `runtime_checked`.

#### Forma de cada entry

Cada entrada del verification report debe incluir:

```txt
claim
state
evidence
scope
blocking
repair_options
```

Así el reporte es auditable y accionable, no una lista decorativa.

### Proof obligations

Decisión:

```txt
Proof obligations son entidades first-class.
Se generan, trackean, agrupan, prueban, degradan solo por policy y reportan.
Ninguna obligation desaparece silenciosamente.
```

Una proof obligation es una pregunta formal que el sistema necesita resolver para aceptar un claim.

Ejemplo:

```txt
fn charge(amount: PositiveMoney)
```

Si se llama con:

```txt
charge(total)
```

y `total: Money`, el verifier genera:

```txt
prove total > Money.zero
```

#### Fuentes de obligations

```txt
types/refinements
contracts
resource lifecycle
concurrency
interfaces
handlers
boundaries
policies
```

Ejemplos:

```txt
prove cart.total >= 0
prove handler StripePayment satisfies payment.charge contract
prove transaction is committed or rolled back
prove task is awaited or cancelled
prove no undeclared effect is used
```

#### Estados de una obligation

```txt
open
proven
runtime_checked
assumed
unverified
failed
```

#### Pipeline de resolución

```txt
generate obligations
  ↓
simplify
  ↓
try local proof rules
  ↓
try solver/SMT
  ↓
try contract composition
  ↓
try runtime check
  ↓
ask for assumption/approval if allowed
  ↓
fail or mark unverified
```

#### Agrupación

Obligations se agrupan por:

```txt
target
kind
source
blocking
```

Ejemplo:

```txt
target fn.checkout
kind refinement
source call.payment.charge
blocking true
```

#### Degradación controlada

Si no se puede probar, una obligation puede degradar solo si policy lo permite:

```txt
proven -> runtime_checked -> assumed -> unverified -> failed
```

No todas las obligations pueden degradar.

Ejemplo no degradable:

```txt
type mismatch -> failed
```

Ejemplo degradable:

```txt
external API honors contract -> assumed
```

#### Forma

```txt
proof_obligation po_123
target call.payment.charge
kind refinement
claim "total is PositiveMoney"
required_by "fn.charge amount: PositiveMoney"

attempts
  local_refinement failed
  solver failed
  contract_composition failed
end

repair_options
  option add_guard
    op insert_guard target=fn.checkout before=call.payment.charge condition="total > Money.zero"
  end

  option runtime_check
    op add_runtime_check target=call.payment.charge refinement=PositiveMoney
  end
end
```

Regla:

```txt
Cada obligation debe explicar qué hay que probar,
por qué existe,
qué evidencia se intentó,
y qué opciones de reparación/degradación existen.
```

### Verification profiles

<!-- Implementation Status: `ail-verify::PolicyEngine` currently implements these profiles as report gates over already-classified `VerificationState` entries. It does not itself prove that a `runtime_checked` entry has a materialized runtime check; upstream checker/report producers must classify that correctly. -->

Decisión:

```txt
Verification profiles son gates, no modificadores de verdad.
El estado de cada claim no cambia por profile.
El profile decide accepted/rejected/approval_required.
```

Profiles:

```txt
draft
dev
test
staging
prod
critical
```

#### draft

Uso: exploración y diseño.

Permite:

```txt
unverified con warnings
assumed anotado
runtime_checked
```

Bloquea:

```txt
failed
unsafe sin approval
```

#### dev

Uso: desarrollo normal.

Permite:

```txt
runtime_checked
assumed con boundary
unverified solo en nodos privados si está anotado
```

Bloquea:

```txt
failed
unsafe
unverified público
```

#### test

Uso: tests, simulación, replay.

Permite:

```txt
fake handlers
deterministic clock/random
runtime_checked
test-only assumptions
```

Bloquea:

```txt
failed
unsafe no aprobado
```

#### staging

Uso: pre-producción.

Permite:

```txt
assumed con approval
runtime_checked
```

Bloquea:

```txt
failed
unsafe
unverified
unapproved assumptions
```

#### prod

Uso: producción.

Permite:

```txt
proven
runtime_checked (si el productor del report materializó el runtime check)
assumed con strong approval
unsafe solo con strong security-exception approval
```

Bloquea:

```txt
failed
unverified
assumed sin strong approval
unsafe sin strong security-exception approval
```

#### critical

Uso: pagos, auth, safety, infraestructura crítica.

Permite:

```txt
proven
runtime_checked (el gate actual acepta el estado clasificado)
assumed solo con strong approval
```

Bloquea:

```txt
unverified
unsafe
failed
weak assumptions
```

Ejemplo:

```txt
claim X = unverified

draft -> accepted with warning
prod  -> rejected
```

El claim sigue siendo `unverified` en ambos casos. Solo cambia el gate.

### Verification report schema

Decisión:

```txt
Verification report es un artefacto estructurado, versionado, parseable y diffable.
No es un log textual.
Es el audit trail para aceptar/rechazar un ChangeSet.
```

Objetivos:

```txt
1. Decir si el cambio entra o no.
2. Mostrar por qué.
3. Enumerar claims por estado.
4. Mostrar proof obligations y repairs.
5. Mostrar policy/approval gates.
6. Ser parseable por LLM/tooling.
7. Ser estable para auditoría.
```

Schema propuesto:

```txt
verification_report <id>
schema verification/1.0
change <change_id>
profile <draft|dev|test|staging|prod|critical>
base <snapshot_id>
target <snapshot_id?>

status accepted | rejected | approval_required

summary
  verified_count <n>
  runtime_checked_count <n>
  assumed_count <n>
  unverified_count <n>
  unsafe_count <n>
  failed_count <n>
end

entries
  entry <id>
    claim "..."
    state proven | runtime_checked | assumed | unverified | unsafe | failed
    scope <node/ref>
    evidence ...
    blocking true|false
  end
end

proof_obligations
  ...
end

diagnostics
  ...
end

policy
  ...
end

approvals
  ...
end

structural_diff
  ...
end

artifacts
  ...
end

end
```

Top-level status:

```txt
accepted
rejected
approval_required
```

Warnings no son status principal; viven en entries/diagnostics.

#### Artifact consistency

El report debe referenciar hashes de los artefactos verificados:

```txt
artifacts
  submitted_change hash=...
  canonical_change hash=...
  semantic_graph_diff hash=...
  core_ir hash=...
  anf_ir hash=...
  wasm hash=...
  capabilities_manifest hash=...
  verification_report hash=...
end
```

Regla:

```txt
El report debe dejar claro exactamente qué fue verificado.
```

Si el canonical change, IR o manifest cambian, el report anterior ya no autoriza el nuevo artefacto.

### Capas de verificación

El pipeline verifica por capas:

```txt
1. Syntax / grammar
2. Op schema
3. Semantic references
4. Type checking
5. Effect checking
6. Refinement checking
7. Contract checking
8. Resource lifecycle
9. Concurrency safety
10. Boundary/FFI trust
11. Policy compliance
12. Artifact/codegen consistency
```

Cada capa puede producir diagnostics reparables.

### Técnicas de verificación por capa

Decisión:

```txt
El verifier es multi-engine.
Cada capa usa la técnica adecuada.
Todos los resultados se normalizan al mismo schema de estados/reporte.
```

No se usa “un solver para todo”.

#### 1. Syntax / grammar

Técnica:

```txt
deterministic parser
```

Resultado:

```txt
parsed | failed
```

No hay `assumed` en parsing.

#### 2. Op schema

Técnica:

```txt
schema validation
```

Verifica:

```txt
required keys
argument types
allowed ops
version compatibility
```

Si falla: `failed`.

#### 3. Semantic references

Técnica:

```txt
graph resolver
```

Verifica:

```txt
NodeIds existen
refs apuntan al tipo correcto
hash/base snapshot coincide
no stale context
```

#### 4. Type checking

Técnica:

```txt
deterministic type checker
```

Verifica:

```txt
types resolve
function calls match
generics instantiate
interfaces resolve coherently
no ambiguous impls
```

Fallos de tipo son `failed`, no assumptions.

#### 5. Effect checking

Técnica:

```txt
effect row checker
capability resolver
handler checker
```

Verifica:

```txt
effects declared
handlers satisfy capabilities
no undeclared external access
effect params propagate
```

#### 6. Refinement checking

Técnicas:

```txt
local rules
SMT solver
contract composition
runtime checks
assumptions if boundary
```

Puede terminar en:

```txt
proven
runtime_checked
assumed
unverified
failed
```

#### 7. Contract checking

Técnicas:

```txt
pre/post condition checking
symbolic execution limited
property testing
runtime checks
proof obligations
```

No todo contrato será `proven`; puede degradar según policy.

#### 8. Resource lifecycle

Técnica:

```txt
ownership/affine-linear analysis
```

Verifica:

```txt
no use after release
no double release
linear resources consumed
```

#### 9. Concurrency safety

Técnica:

```txt
structured concurrency analysis
task lifecycle analysis
shared state capability checks
```

Verifica:

```txt
no orphan tasks
await/cancel/transfer
no Cell<T> crossing task boundary
Shared requires safe capability
```

#### 10. Boundary/FFI trust

Técnica:

```txt
boundary contract validation
adapter schema validation
trust-level policy
```

Clasifica:

```txt
assumed
unverified
unsafe
failed
```

#### 11. Policy compliance

Técnica:

```txt
policy engine
```

Verifica:

```txt
profile gate
approval required
unsafe allowed?
public API changed?
capability grants allowed?
```

#### 12. Codegen consistency

Técnica:

```txt
hash mapping
IR provenance
manifest consistency
compiler validation
```

Verifica:

```txt
WASM corresponds to ANF/Core IR
capabilities manifest matches effects
report hashes match artifacts
```

Regla:

```txt
Cada engine produce resultados propios,
pero el verification report los normaliza en states comunes.
```

### Assumptions y boundaries

Decisión:

```txt
Assumptions son claims first-class, scoped, expiring, owned y approved,
siempre asociados a boundaries explícitos.
No existen assumptions flotantes.
```

Una assumption válida es una afirmación que el sistema no puede probar internamente, pero acepta porque está en una frontera de confianza explícita con contrato y responsable.

Ejemplo:

```txt
Stripe honors idempotency key
```

No se prueba internamente. Se modela como boundary:

```txt
boundary Stripe
contract payment.charge
trust assumed
```

#### Requisitos de una assumption válida

```txt
1. boundary explícito
2. claim formal
3. scope
4. owner/responsable
5. evidence/documentation
6. expiration/review policy
7. approval según profile
8. fallback/mitigation si aplica
```

Ejemplo:

```txt
assumption stripe_idempotency
boundary boundary.Stripe
claim "Stripe honors idempotency key for payment charge"
scope cap.payment.charge:PaymentProvider
owner team.payments
evidence doc.stripe_idempotency_contract
expires 2026-12-31
requires_approval security
mitigation "local idempotency table prevents duplicate order creation"
```

Inválido:

```txt
assume "checkout is safe"
```

Inválido salvo acotado como advisory boundary:

```txt
assume external_ai_returns_truth
```

Forma válida:

```txt
boundary OpenAI
claim "model output is advisory only, never authority"
```

#### Assumption lifecycle

```txt
proposed
approved
active
expired
revoked
failed_review
```

Si una assumption expira o se revoca:

```txt
prod/critical builds bloquean si dependen de ella
```

#### Boundaries

Un boundary representa una frontera de confianza:

```txt
external API
database engine
native extension
OS syscall
LLM provider
clock
random source
human approval
```

Cada boundary debe declarar:

```txt
trust level
capabilities exposed
contracts
handlers/adapters
failure modes
assumptions
```

Reglas:

```txt
1. No free-floating assumptions.
2. Toda assumption tiene owner y expiration/review policy.
3. Toda assumption pertenece a un boundary.
4. Expired/revoked assumptions bloquean profiles que las requieren.
5. Una assumption no puede ocultar unsafe.
6. Una assumption debe aparecer en verification report.
```

### Runtime checks

Decisión:

```txt
Runtime checks son nodos materializados en IR/graph.
Solo pueden justificar runtime_checked si existen en canonical graph/IR
y aparecen en el verification report con hash de artefacto.
```

Un runtime check verifica en ejecución algo que no pudo probarse estáticamente.

Ejemplo:

```txt
payload.email -> Email
```

Se valida en decoder:

```txt
validate_email(payload.email)
```

Si falla:

```txt
Err(DecodeError.InvalidEmail)
```

#### Cuándo se permite

Runtime checks son válidos para:

```txt
external input
FFI results
refinements dependientes de datos runtime
bounds/range checks
narrowing conversions
decoder validation
capability responses
```

No sirven para tapar:

```txt
type mismatch
undeclared effect
ambiguous impl
resource lifecycle violation provable
```

Eso es `failed`.

#### Requisitos de report

Para marcar algo como `runtime_checked`, debe registrarse:

```txt
check_id
target
condition
insertion_point
failure_behavior
artifact_hash
```

Ejemplo:

```txt
runtime_check rc_123
target field.email
condition Email(value)
insertion_point decoder.UserInput.email
failure_behavior Err(DecodeError.InvalidEmail)
artifact_hash core_ir:abc123
```

#### Failure behavior

Todo runtime check debe declarar qué pasa si falla:

```txt
return Err
abort
reject input
rollback transaction
deny capability response
```

Preferencia:

```txt
Result<T, E>
```

porque el lenguaje no tiene excepciones implícitas.

#### No auto-inserción silenciosa

El toolchain puede proponer:

```txt
op add_runtime_check target=call.payment.charge refinement=PositiveMoney
```

Pero insertar un check cambia comportamiento y debe aparecer como canonical diff.

Reglas:

```txt
1. Runtime check debe estar en canonical graph/IR.
2. Runtime check debe tener failure_behavior seguro.
3. Runtime check debe estar cubierto por artifact hash del report.
4. Runtime check no puede tapar errores que son failed por naturaleza.
5. Runtime check insertado por toolchain debe materializarse como diff.
```

### Contract checking

Decisión:

```txt
Contracts son ASTs tipados, no texto libre.
requires crea obligations para callers.
ensures crea obligations para callee/body.
invariants crean preservation obligations para cambios afectados.
```

Tipos principales:

```txt
requires
ensures
invariant
```

#### requires

Precondición que debe cumplirse antes de llamar.

```txt
fn charge(amount: Money)
  requires amount > Money.zero
```

El caller debe probarlo, runtime-checkearlo si policy permite, o fallar.

Si no puede:

```txt
E_PRECONDITION_NOT_PROVEN
```

#### ensures

Postcondición que la función promete al terminar.

```txt
fn cart_total(cart: Cart) -> Decimal<scale=2, precision=18>
  ensures result >= Decimal.zero
```

El verifier intenta probar que el body cumple la promesa.

#### invariant

Regla que debe preservarse para un tipo, módulo o sistema.

```txt
invariant Order:
  paid_order_has_payment
```

Toda operación que pueda afectar `Order` debe preservar esa invariant.

#### Pipeline

```txt
parse contract expr
  ↓
type-check contract
  ↓
generate proof obligations
  ↓
try local rules
  ↓
try SMT/solver
  ↓
try contract composition
  ↓
try runtime check if policy allows
  ↓
assumption only if boundary
  ↓
failed/unverified
```

#### Contract composition

Una función puede usar contracts de funciones llamadas.

Ejemplo:

```txt
fn line_total(item)
  ensures result >= Decimal.zero

fn cart_total(cart)
  uses line_total
  ensures result >= Decimal.zero
```

`cart_total` puede probarse usando el `ensures` de `line_total`.

#### Invariant impact analysis

Para invariants, el verifier necesita saber qué cambios pueden afectar la regla:

```txt
Order invariant
  affected by:
    - fn.create_order
    - fn.mark_paid
    - handler.payment_webhook
```

Si un ChangeSet toca algo que puede mutar `Order`, se regeneran preservation obligations.

#### Runtime contracts

Algunos contracts pueden ser `runtime_checked`, según policy:

```txt
requires external_payload.email is Email
```

Pero contracts críticos no deberían degradar libremente.

Ejemplo:

```txt
stock_never_negative
```

En `prod/critical` debería ser `proven` o enforced transaccionalmente, no solo “check después”.

#### Boundary contracts

Contracts de sistemas externos pueden quedar `assumed` solo si están en un boundary válido.

```txt
boundary Stripe
contract payment.charge
assumption stripe_idempotency
```

Reglas:

```txt
1. Contract expressions se parsean a ContractExprAST.
2. requires obliga al caller.
3. ensures obliga al callee/body.
4. invariants obligan a todos los cambios afectados.
5. Contract composition es fuente válida de evidencia.
6. Runtime checks solo si policy lo permite y se materializan.
7. Assumptions solo para boundaries externos válidos.
```

### Effect/capability verification

Decisión:

```txt
Effect verification compara effects declarados,
effects inferidos desde el body,
effects transformados por handlers,
capabilities otorgadas por profile,
y capabilities emitidas en manifest.
Ningún effect puede aparecer o desaparecer silenciosamente.
```

Objetivo:

```txt
effects declarados == effects realmente usados o propagados
```

Y todo effect externo debe estar autorizado por capability/handler/policy.

#### Uso interno

Si el body contiene:

```txt
EffectCall database.write:Order
```

la función debe declarar:

```txt
effects { database.write:Order }
```

Si no:

```txt
E_EFFECT_UNDECLARED
```

#### Effects de más

Si una función declara:

```txt
effects { file.write }
```

pero no lo usa ni lo propaga, el verifier emite:

```txt
E_EFFECT_UNUSED
```

Severity depende de policy. En perfiles estrictos, effects de más pueden bloquear porque amplían permisos sin necesidad.

#### Propagación

Si una función llama a otra con effects:

```txt
fn checkout effects { payment.charge:PaymentProvider }

fn api_checkout calls checkout
```

`api_checkout` debe:

```txt
declarar payment.charge:PaymentProvider
```

o manejarlo con un handler válido.

#### Effect handlers

Un handler que maneja:

```txt
payment.charge:PaymentProvider
```

debe declarar qué effects usa internamente:

```txt
effects { http.call:Stripe }
```

El effect no desaparece mágicamente; se transforma.

Ejemplo:

```txt
payment.charge:PaymentProvider
  handled_by handler.StripePayment
  transforms_to http.call:Stripe
```

#### Capability authorization

Declarar un effect no alcanza. El run profile debe otorgar la capability:

```txt
op grant target=module.checkout capability=database.write:Order profile=prod
```

Si no:

```txt
E_CAPABILITY_NOT_GRANTED
```

#### Manifest consistency

El manifest ejecutable debe coincidir con el IR verificado:

```txt
effects in IR == capabilities_manifest.requires
```

Si el WASM/imports/manifest piden algo extra:

```txt
E_MANIFEST_MISMATCH
```

#### Checks requeridos

```txt
1. Declared effects cover inferred effects.
2. Unused effects are reported.
3. Effect params propagate precisely through generics.
4. Handlers satisfy handled capability contracts.
5. Handler internal effects are declared.
6. Run profile grants required capabilities.
7. Capabilities manifest matches verified IR.
8. Runtime host denies ungranted capabilities.
```

#### Reporte

El verification report debe incluir:

```txt
declared_effects
inferred_effects
handler_transformations
required_capabilities
granted_capabilities
manifest_capabilities
missing_grants
unused_effects
```

### Técnicas por tipo de obligación

No todo se verifica igual:

```txt
type safety            deterministic type checker
effects                effect checker
refinements simples    SMT/solver o proof rules
contracts complejos    proof/test/runtime_check/assumption
resources              ownership/lifecycle analysis
concurrency            structured concurrency analysis
boundaries             trust contract + adapter checks
policies               rules engine
codegen consistency    artifact hash + IR mapping
```

### Verification report

Todo ChangeSet produce reporte obligatorio:

```txt
verification_report change.add_checkout

status accepted | rejected | approval_required

verified
  ...
end

runtime_checked
  ...
end

assumed
  ...
end

unverified
  ...
end

unsafe
  ...
end

failed
  ...
end

proof_obligations
  ...
end

policy
  ...
end

end
```

### Reglas de bloqueo

```txt
1. failed bloquea siempre.
2. unsafe bloquea salvo approval explícita fuerte.
3. unverified bloquea en public/prod boundaries.
4. assumed requiere boundary explícito y policy/approval.
5. runtime_checked pasa solo si el check está insertado/materializado.
6. proven pasa.
```

### Filosofía

```txt
La IA puede equivocarse.
El verifier no debe confiar en la IA.
El verifier tampoco promete omnisciencia.
Su trabajo es clasificar verdad, deuda y riesgo sin ocultarlos.
```

### Verification model completo: propuesta consolidada

Esta sección consolida el Verification model completo. El objetivo no es prometer verificación matemática total para cualquier programa general-purpose. El objetivo es que ninguna verdad, deuda o riesgo quede implícito.

#### Tesis

```txt
El verifier es la autoridad técnica.
La IA propone cambios.
El verifier clasifica claims.
La policy decide gates.
El report audita exactamente qué fue aceptado.
```

#### Inputs

El verifier recibe:

```txt
canonical_change
base_graph_snapshot
target_profile
project_policy
semantic_graph
core_ir
anf_ir
op_schemas
capability_registry
handler_bindings
boundary_registry
package_trust_metadata
approval_records
```

#### Outputs

El verifier produce:

```txt
verification_report
diagnostics
proof_obligations
policy_report
approval_requirements
artifact_hashes
```

#### Estados normalizados

Todo claim termina en uno de estos estados:

```txt
proven
runtime_checked
assumed
unverified
unsafe
failed
```

Prioridad:

```txt
failed > unsafe > unverified > assumed > runtime_checked > proven
```

#### Gate final

El status del report es:

```txt
accepted
rejected
approval_required
```

Regla:

```txt
Los estados describen verdad/riesgo.
El profile/policy decide si el cambio pasa.
```

#### Pipeline completo

<!-- Status: Target design with implemented subset. Current implementation covers the subset represented by `ail-verify` checker modules plus compiler/runtime hash checks; not every layer below is complete. -->

```txt
1. Parse ChangeSet
2. Canonicalize ChangeSet
3. Validate op schemas
4. Resolve graph references
5. Build semantic diff
6. Lower affected graph to Core IR
7. Type check
8. Effect/capability check
9. Generate proof obligations
10. Check refinements
11. Check contracts
12. Check invariants via impact analysis
13. Check resource lifecycle
14. Check concurrency safety
15. Check boundaries/FFI/trust
16. Check package trust/dependencies
17. Check policy gates
18. Check approval records
19. Lower to ANF
20. Check ANF effect/resource ordering
21. Generate/validate manifest
22. Codegen consistency check
23. Emit verification report
```

Implementation note: the codebase currently implements this as composable checker modules rather than one monolithic 23-step driver. Implemented areas include type, effect, contract, resource, concurrency, boundary, package, policy, proof, solver, report, and codegen checks under `crates/ail-verify/src/`. Graph canonicalization, full ANF/resource ordering validation, package/dependency policy depth, and critical-profile formal completeness remain validation work rather than completed proof.

#### Layer responsibilities

| Layer | Verifies | Failure type |
|---|---|---|
| Grammar | DSL parseability | `failed` |
| Op schema | required keys, arg types, version compatibility | `failed` |
| Graph resolver | NodeRefs, BlockRefs, snapshot/hash freshness | `failed` / `needs_rebase` |
| Type checker | calls, generics, interfaces, Dyn, coherence | `failed` |
| Effect checker | declared vs inferred effects, propagation | `failed` / diagnostics |
| Capability checker | grants, handler bindings, manifest requirements | `failed` / `approval_required` |
| Refinement checker | predicates over values | `proven/runtime_checked/assumed/unverified/failed` |
| Contract checker | requires/ensures/invariants | mixed states |
| Resource checker | affine/linear/shared lifecycle | `proven/failed/unsafe` |
| Concurrency checker | tasks/channels/shared state/cancellation | `proven/failed/unverified` |
| Boundary checker | FFI/external trust contracts | `assumed/unverified/unsafe/failed` |
| Policy engine | profile/project/security rules | `accepted/rejected/approval_required` |
| Codegen checker | artifact hash/provenance/manifest consistency | `proven/failed` |

#### Type verification

Verifica:

```txt
types resolve
nominal identity respected
explicit conversions used
generic params valid
effect/capability params valid
const params decidable
interface impls coherent
Dyn<Interface> contracts available
no ambiguous impls
```

Reglas:

```txt
type mismatch -> failed
ambiguous impl -> failed
missing interface impl -> failed
invalid narrowing conversion -> failed unless checked conversion returns Result
```

#### Refinement verification

Refinements generan proof obligations:

```txt
PositiveMoney requires amount > Money.zero
NonEmptyText requires length_graphemes(value) > 0
Email requires matches_email(value)
```

Evidencia permitida:

```txt
local proof
contract composition
SMT/solver
runtime check at boundary
explicit assumption only for external boundary
```

#### Contract verification

```txt
requires -> caller obligation
ensures -> callee/body obligation
invariant -> preservation obligation for affected changes
```

Evidencia de contracts:

```txt
body proof
called function ensures
stdlib verified contract
transactional enforcement
runtime check by policy
boundary assumption
```

Critical invariants deben ser `proven` o enforced transaccionalmente en `prod/critical`.

#### Effect/capability verification

Compara:

```txt
declared_effects
inferred_effects
handler_transformations
required_capabilities
granted_capabilities
manifest_capabilities
```

Reglas:

```txt
undeclared effect -> failed
ungranted capability -> rejected/approval_required depending policy
handler must satisfy handled capability contract
handler internal effects must be declared
unused broad effects may block strict profiles
manifest mismatch -> failed
runtime host must deny ungranted capabilities
```

#### Resource lifecycle verification

Resources se verifican mediante `Handle<Resource, Mode>`.

Checks:

```txt
no use after release
no double release
linear resources consumed exactly once
affine resources released/transferred/scope-cleaned
shared resources require concurrency-safe type/capability
transactions commit or rollback
locks release
streams close or transfer
tasks await/cancel/transfer
```

Failures:

```txt
use after release -> failed
double release -> failed
linear not consumed -> failed
shared without safe capability -> failed/unsafe
```

#### Concurrency verification

Verifica structured concurrency:

```txt
no orphan tasks
TaskGroup scope closes cleanly
tasks awaited/cancelled/transferred
channels closed or transferred
timeouts use clock.monotonic
Cell<T> does not cross task boundary
Shared handles require safe capability/type
cancellation behavior declared
```

Rules:

```txt
orphan task -> failed
Cell<T> across task boundary -> failed
shared mutable state without capability -> failed/unsafe
unbounded concurrency without policy -> unverified or rejected by profile
```

#### Boundary/FFI verification

Boundary debe declarar:

```txt
trust level
capabilities exposed
contracts
handlers/adapters
failure modes
assumptions
owner
review/expiration policy
```

FFI checks:

```txt
adapter schema valid
foreign types mapped explicitly
unsafe operations marked unsafe
assumptions attached to boundary
runtime checks inserted for decoded values
capabilities scoped least-privilege
```

Rules:

```txt
unchecked FFI -> unsafe
boundary without contract -> unverified/failed depending profile
expired assumption -> rejected in prod/critical
external AI output cannot be authority unless policy explicitly models it
```

#### Policy verification

Policy engine decide gates usando:

```txt
profile
structural_diff
verification states
capability grants
public API changes
unsafe/unverified/assumed entries
approval records
package trust metadata
```

Package assumptions are approved at the active assumption scope in strict
profiles, for example:

```txt
package:payments.stripe@1.2.0#assumption:stripe_idempotency
```

Puede devolver:

```txt
passed
failed
approval_required
```

#### Codegen/artifact verification

Garantiza que artefactos generados corresponden al IR verificado.

Checks:

```txt
canonical_change hash matches report
graph_diff hash matches report
Core IR hash matches report
ANF IR hash matches report
WASM/imports match capabilities manifest
capabilities manifest matches effect analysis
generated SDK/docs marked as derived artifacts
```

Rules:

```txt
artifact hash mismatch -> failed
manifest extra capability -> failed
WASM imports not in manifest -> failed
report cannot authorize changed artifacts
```

#### Refactor verification

Un refactor debe preservar comportamiento observable.

Checks:

```txt
public API preserved
effects preserved or explicitly justified
contracts preserved
invariants preserved
dependency graph updated
pure equivalence proven where possible
behavior snapshots/property tests used when proof not possible
```

Reglas:

```txt
behavior-changing change cannot be labeled refactor
if observable contract changes -> migrate required
```

#### Migration verification

Una migration cambia intencionalmente contrato/API/comportamiento.

Debe incluir:

```txt
old contract/signature
new contract/signature
compatibility plan
affected nodes/packages
deprecation plan if public
approval requirements
tests/proofs updated
```

Reglas:

```txt
public migration requires approval
silent API change -> failed
```

#### Package/dependency verification

Packages llevan trust metadata:

```txt
verified
assumed
unverified
unsafe
```

Checks:

```txt
imported package trust allowed by profile
exported capabilities documented
package contracts available
version constraints satisfied
deprecated APIs flagged
unsafe packages require approval
```

#### Generated tests and evidence

Tests pueden aportar evidencia, pero no son automáticamente proof.

Evidence states:

```txt
test_passed
property_test_passed
coverage_evidence
regression_snapshot_passed
```

Reglas:

```txt
tests can support runtime_checked/unverified mitigation
tests can support but not replace formal proof for critical invariants unless policy allows
test generation must be linked to contracts/invariants
```

#### Report requirements

Todo verification report debe incluir:

```txt
schema version
change id/hash
profile
base and target snapshot
top-level status
summary counts
entries by state
proof obligations
diagnostics
policy report
approval requirements/records
structural diff
artifact hashes
```

#### Final rules

```txt
1. The verifier never trusts the LLM.
2. Every claim gets a state.
3. Every obligation is tracked.
4. Every assumption belongs to a boundary.
5. Every runtime check is materialized.
6. Every unsafe requires strong approval.
7. Every public/prod unverified claim blocks by default.
8. Every report authorizes exact artifact hashes only.
9. Every policy exception is recorded.
10. Nothing disappears silently.
```
