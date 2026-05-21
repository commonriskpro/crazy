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

## AI Change Language

Este sería el lenguaje que escribe el LLM. No está optimizado para belleza humana, sino para baja ambigüedad.

Decisión:

```txt
AI Change Language = DSL textual line-oriented.
No JSON/YAML como formato principal para LLMs.
Canonicalizer convierte el DSL a AST.
Semantic Graph guarda el resultado, no el texto como source of truth.
```

Razón:

- los LLMs generan bien patrones `verb target key=value`
- es fácil de diffear
- es fácil de reparar con errores estructurados
- evita nesting, comas y escaping excesivo de JSON
- evita ambigüedad/indentación peligrosa de YAML

JSON puede existir como formato interno/API. YAML no debería ser formato principal del lenguaje.

Forma base:

```txt
change <id>
intent "human-readable intent"

op <verb> target=<id> key=value key=value
op <verb> id=<id> key=value key=value

verify target=<id>
end
```

Ejemplo:

```txt
change create_cart_total

op create_function id=fn.cart_total
op add_param target=fn.cart_total name=cart type=Cart
op set_return target=fn.cart_total type=Money
op set_effect target=fn.cart_total effect=pure
op set_totality target=fn.cart_total total=true
op add_contract target=fn.cart_total kind=ensures rule="result >= Money.zero"
op set_body target=fn.cart_total expr="fold(cart.items, Money.zero, fn.line_total)"

op verify
end
```

Para expresiones grandes, se usan bloques referenciados:

```txt
change add_checkout
intent "Add checkout flow"

op create_function id=fn.checkout
op add_param target=fn.checkout name=cartId type=CartId
op infer_boundary target=fn.checkout
op set_body target=fn.checkout body=@expr.checkout_body

block @expr.checkout_body
  let cart = EffectCall database.read:Cart(cartId)
  let total = Call fn.cart_total(cart)
  let payment = EffectCall payment.charge:PaymentProvider(total)
  match payment:
    Ok(receipt) -> ...
    Err(error) -> ...
end

verify target=fn.checkout
end
```

### Categorías de operaciones

Decisión:

```txt
El Change Language usa pocas categorías de operación,
estables y combinables.
No debe tener un verbo distinto para cada feature de producto.
```

Categorías base:

```txt
Core graph ops:
create
set
add
remove
connect
disconnect
rename
move
replace
delete

Semantic workflow ops:
infer
verify
assert
lock
refactor
migrate
derive
generate
approve
reject
deprecate
annotate

Runtime/security ops:
grant
revoke
bind
expose
hide
```

#### create

Crea un nodo nuevo en el Semantic Graph.

```txt
op create_function id=fn.checkout
op create_type id=type.Cart kind=record
op create_capability id=cap.payment.charge
```

#### set

Setea una propiedad única.

```txt
op set_return target=fn.checkout type=Result<OrderId, CheckoutError>
op set_body target=fn.checkout body=@expr.checkout
```

#### add / remove

Agrega o remueve elementos de una colección del nodo.

```txt
op add_param target=fn.checkout name=cartId type=CartId
op add_effect target=fn.checkout effect=database.read:Cart
op add_contract target=fn.checkout kind=ensures rule=order_created_after_payment

op remove_effect target=fn.checkout effect=event.emit:OrderPaid
```

#### connect / disconnect

Crea o quita relaciones semánticas entre nodos.

```txt
op connect source=fn.checkout relation=uses target=cap.payment.charge
op connect source=fn.checkout relation=emits target=event.OrderPaid

op disconnect source=fn.checkout relation=uses target=cap.payment.charge
```

#### rename

Cambia nombre visible, no identidad estable.

```txt
op rename target=fn.cart_total name=calculate_cart_total
```

Regla: `id` no cambia.

#### move

Mueve un nodo entre módulos/paquetes preservando identidad.

```txt
op move target=fn.cart_total to=module.pricing
```

#### replace

Reemplaza una definición o bloque completo.

```txt
op replace target=fn.checkout.body with=@expr.checkout_v2
```

#### delete

Borra un nodo completo. Siempre requiere análisis de impacto.

```txt
op delete target=fn.old_checkout
verify impact target=fn.old_checkout
```

#### infer

Pide inferencia/materialización explícita.

```txt
op infer_boundary target=fn.checkout
op infer_effects target=fn.checkout
op infer_return target=fn.checkout
```

#### assert

Declara expectativas sobre el estado actual antes de aplicar el cambio. Sirve para evitar cambios basados en contexto viejo.

```txt
op assert_exists target=fn.checkout
op assert_signature target=fn.checkout hash=sig_123
op assert_hash target=fn.checkout.body hash=body_456
```

#### lock

Protege API, comportamiento o contracts durante refactors/migrations.

```txt
op lock_behavior target=fn.checkout
op lock_public_api target=module.payment
op lock_contracts target=type.Order
```

#### verify

Valida el cambio, scope o nodo.

```txt
verify target=fn.checkout
verify scope=module.checkout
verify change=current
```

#### refactor

Operaciones semánticas conocidas que deben preservar comportamiento observable.

```txt
op refactor_extract_function from=fn.checkout range=@range.payment to=fn.charge_payment
op refactor_inline target=fn.old_helper
```

Regla: si cambia contrato observable, no es refactor; es migration.

#### migrate

Cambio intencional de API, contrato o comportamiento.

```txt
op migrate_api target=fn.checkout from=sig.v1 to=sig.v2
```

#### derive

Genera implementaciones derivadas desde tipos/schemas bajo reglas verificables.

```txt
op derive_eq target=type.Address mode=structural
op derive_decoder target=type.UserInput format=Json
op derive_encoder target=type.UserOutput format=Json
```

#### generate

Pide generación controlada de artefactos derivados.

```txt
op generate_tests target=fn.checkout from=contracts
op generate_sdk target=module.api language=typescript
op generate_docs target=module.payment audience=reviewer
```

Regla: generated artifacts no son source of truth salvo que se materialicen explícitamente en el grafo.

#### approve / reject

Acepta o rechaza inferencias, assumptions, migrations o propuestas.

```txt
op approve_inferred_boundary target=fn.checkout version=sig_123
op approve_assumption target=boundary.stripe reason="External provider contract"
op reject_inferred_boundary target=fn.checkout version=sig_124
```

#### deprecate

Marca un nodo como reemplazado sin borrarlo.

```txt
op deprecate target=fn.old_checkout replacement=fn.checkout_v2
```

Útil para APIs, paquetes y migraciones graduales.

#### annotate

Agrega metadata, rationale, trust, assumptions o notas de revisión.

```txt
op annotate target=fn.checkout key=rationale value="Checkout must be idempotent"
op annotate target=boundary.stripe key=trust value=assumed
```

#### grant / revoke

Declara permisos/capabilities otorgadas o removidas a módulos, paquetes o run profiles.

```txt
op grant target=module.checkout capability=database.read:Cart profile=prod
op revoke target=module.checkout capability=file.write profile=prod
```

Esto pertenece a runtime/security, no al body de una función.

#### bind

Asocia handlers con capabilities en un environment/run profile.

```txt
op bind_handler capability=payment.charge:PaymentProvider handler=handler.StripePayment profile=prod
op bind_handler capability=payment.charge:PaymentProvider handler=handler.FakePayment profile=test
```

#### expose / hide

Controla exports públicos.

```txt
op expose target=fn.checkout as=api.checkout
op hide target=fn.internal_helper
```

### Cobertura de operaciones

“Cubrir todo” significa que cualquier cambio de software pueda expresarse como combinación de operaciones genéricas sobre el grafo:

```txt
crear
actualizar
relacionar
mover
renombrar
borrar
inferir
verificar
asegurar precondiciones
bloquear contratos
migrar
derivar
generar artefactos
aprobar/rechazar propuestas
deprecar
otorgar/revocar capabilities
asociar handlers
exponer/ocultar APIs
anotar
```

No queremos operaciones de producto ambiguas:

```txt
op add_checkout       // demasiado alto nivel
op add_stripe         // demasiado específico
op fix_bug            // ambiguo
op make_it_better     // inválido
```

La intención humana puede ser “agregá checkout”, pero el LLM debe traducirla a operaciones atómicas verificables.

Reglas:

```txt
1. create/delete cambian nodos.
2. set/add/remove cambian propiedades.
3. connect/disconnect cambian relaciones.
4. rename/move preservan identidad.
5. refactor preserva comportamiento observable.
6. migrate declara cambio intencional.
7. infer materializa diff; nunca cambia silenciosamente.
8. verify decide si el ChangeSet entra.
9. assert/lock protegen contra contexto viejo y cambios accidentales.
10. grant/revoke/bind afectan ejecución y seguridad.
11. expose/hide afectan API pública.
```

### Estructura formal de un ChangeSet

Decisión:

```txt
Un ChangeSet puede escribirse en forma corta,
pero el canonicalizer lo normaliza a una forma completa con secciones explícitas.
```

Secciones:

```txt
metadata
requires
expect
ops
blocks
verify
approval
```

#### Forma completa

```txt
change add_checkout
intent "Add checkout flow with payment and order creation"

metadata
  author agent
  reason "User requested checkout"
end

requires
  assert_exists type.Cart
  assert_exists cap.payment.charge
  assert_hash fn.cart_total sig=sig_123
end

expect
  creates fn.checkout
  effects database.read:Cart
  effects payment.charge:PaymentProvider
  no_new_public_api_except fn.checkout
  no_unsafe
end

ops
  op create_function id=fn.checkout
  op add_param target=fn.checkout name=cartId type=CartId
  op infer_boundary target=fn.checkout
  op set_body target=fn.checkout body=@expr.checkout
end

block @expr.checkout
  ...
end

verify
  target fn.checkout
  impact module.checkout
  contracts required
  effects required
end

approval
  require_if public_api_changed
  require_if unsafe_added
end

end
```

#### Forma corta

```txt
change add_checkout
intent "Add checkout flow"

op create_function id=fn.checkout
op add_param target=fn.checkout name=cartId type=CartId
op infer_boundary target=fn.checkout
op set_body target=fn.checkout body=@expr.checkout

block @expr.checkout
  ...
end

verify target=fn.checkout
end
```

### Claims de IA vs políticas externas

`requires` y `expect` pueden ser escritos por la IA, por lo tanto no son autoridad. Son claims verificables.

```txt
requires = claims sobre el estado previo esperado
expect   = claims sobre el diff esperado
```

El toolchain los contrasta contra la realidad.

Ejemplo de contexto viejo:

```txt
requires
  assert_hash fn.cart_total sig=sig_123
end
```

Si el grafo real tiene otro hash:

```txt
Rejected: stale context
```

Ejemplo de diff inesperado:

```txt
expect
  no_new_public_api_except fn.checkout
end
```

Si el cambio expone accidentalmente `fn.debug_payment`:

```txt
Rejected: unexpected public API fn.debug_payment
```

Regla:

```txt
requires/expect escritos por IA son claims verificables.
policy/approval no los controla la IA.
```

Políticas externas aplicadas por el toolchain/proyecto:

```txt
project_policy
security_policy
review_policy
human_approval_required
```

Ejemplos:

```txt
no_unsafe
no_unverified_public_api
max_new_capabilities 2
require_human_approval for public_api_changed
```

La IA puede declarar intención y expectativas. No puede autoaprobar cambios críticos.

### Diagnósticos reparables

Decisión:

```txt
Diagnostics son parte del protocolo del lenguaje.
Los errores deben ser estructurados, parseables y reparables por ChangeSets.
No hay auto-repair silencioso salvo que una policy lo permita explícitamente.
```

Formato base:

```txt
error <ERROR_ID>
severity <info|warning|error|critical>
target <NodeId|BlockRef|RangeRef>
message "..."

evidence
  ...
end

expected
  ...
end

actual
  ...
end

repair_options
  option <id>
    ... ops ...
  end
end

blocking <true|false>
```

Reglas:

```txt
1. Todo diagnóstico tiene código estable.
2. Todo diagnóstico apunta a NodeId, block o range.
3. Si hay reparación segura, ofrece ops exactas.
4. Si hay múltiples reparaciones, no elige mágicamente.
5. Si implica API/security/unsafe, requiere approval/policy.
6. Diagnostics son parseables por máquina.
7. El LLM repara enviando un nuevo ChangeSet o una opción elegida.
```

Tipos de repair:

```txt
direct_op      aplicar una op exacta
choice         elegir entre varias opciones
migration      requiere migrate
approval       requiere humano/policy
explanation    requiere más información/intención
```

#### Ejemplo: effect no declarado

```txt
error E_EFFECT_UNDECLARED
severity error
target fn.checkout
message "Function uses undeclared effect database.write:Order"

evidence
  used_effect database.write:Order at @expr.checkout:line8
end

expected
  effects { database.read:Cart, payment.charge:PaymentProvider }
end

actual
  effects { database.read:Cart, payment.charge:PaymentProvider, database.write:Order }
end

repair_options
  option add_effect
    op add_effect target=fn.checkout effect=database.write:Order
  end

  option remove_write
    op replace target=@expr.checkout:line8 with=@expr.no_write
  end
end

blocking true
```

#### Ejemplo: refinement no probado

```txt
error E_REFINEMENT_NOT_PROVEN
severity error
target call.payment.charge
message "Money cannot be used as PositiveMoney"

expected
  PositiveMoney
end

actual
  Money
end

proof_obligation
  prove total > Money.zero
end

repair_options
  option guard
    op insert_guard target=fn.checkout before=call.payment.charge condition="total > Money.zero"
  end

  option runtime_check
    op add_runtime_check target=call.payment.charge refinement=PositiveMoney
  end

  option change_contract
    op migrate_api target=fn.charge from=PositiveMoney to=Money
    requires approval public_api_changed
  end
end

blocking true
```

Regla final:

```txt
El verifier no solo dice “falló”.
Debe explicar qué falló, dónde, por qué, qué esperaba, qué encontró y cómo puede repararse.
```

### Canonicalization

Decisión:

```txt
Todo ChangeSet tiene submitted form y canonical form.
Submitted form es lo que escribió el LLM.
Canonical form es normalizada, estable, diffable y verificable.
```

El commit del Semantic Graph guarda:

```txt
submitted_change
canonical_change
structural_diff
verification_report
```

#### Qué normaliza

Orden de operaciones:

```txt
create
set/add/remove
connect/disconnect
infer/materialized inference
verify
```

Orden de keys:

```txt
op add_param target=fn.x name=a type=Int
```

IDs:

```txt
Fn.CartTotal
fn.cart-total
fn.cart_total
```

se normalizan a:

```txt
fn.cart_total
```

Literals:

```txt
true/false
Decimal<scale=2, precision=18>
NormalizedText<NFC>
```

Blocks:

```txt
block @expr.checkout_body hash=expr_abc123
```

#### Expandir inferencias aceptadas

Si el submitted form dice:

```txt
op infer_boundary target=fn.checkout
```

el toolchain puede inferir:

```txt
return: Result<OrderId, CheckoutError>
effects:
  - database.read:Cart
  - database.write:Order
```

Una vez aceptado, canonical form materializa la inferencia:

```txt
op set_return target=fn.checkout type=Result<OrderId, CheckoutError>
op add_effect target=fn.checkout effect=database.read:Cart
op add_effect target=fn.checkout effect=database.write:Order
```

Regla: no queda como “inferilo cada vez”. La inferencia ayuda a escribir menos, pero el grafo canónico queda explícito.

#### Materializar defaults explícitos

Si el submitted form omite defaults seguros:

```txt
op create_function id=fn.cart_total
```

canonical form los materializa:

```txt
op create_function id=fn.cart_total visibility=private
```

Otro ejemplo:

```txt
op create_type id=type.Address kind=record
```

puede canonicalizar a:

```txt
op create_type id=type.Address kind=record visibility=private derive=none
```

Reglas para defaults:

```txt
1. Deben ser seguros.
2. Deben ser mecánicos.
3. Deben ser no ambiguos.
4. Deben estar documentados.
5. Nunca pueden otorgar permisos, exponer APIs o asumir trust peligroso.
```

Defaults prohibidos:

```txt
public
unsafe
assumed
grant all capabilities
autoapprove migration
```

#### Qué no puede hacer canonicalization

```txt
No puede inventar semántica.
No puede autoaprobar assumptions.
No puede elegir entre repairs ambiguos.
No puede cambiar behavior.
No puede ocultar warnings.
No puede convertir un cambio inseguro en seguro por formato.
```

Regla final:

```txt
Submitted form puede ser abreviada.
Canonical form debe ser explícita.
```

### Transaction model

Decisión:

```txt
ChangeSets son transacciones atómicas sobre el Semantic Graph.
Un ChangeSet entra completo o no entra.
No hay partial apply.
```

Estados:

```txt
draft
parsed
canonicalized
verified
rejected
applied
rolled_back
needs_rebase
```

Pipeline:

```txt
submitted_change
  ↓ parse
parsed_change
  ↓ canonicalize
canonical_change
  ↓ verify
verification_report
  ↓ apply transaction
new graph snapshot
```

#### Atomicidad

Si una operación falla:

```txt
op 1 ok
op 2 ok
op 3 fails
```

resultado:

```txt
op 1 y op 2 no quedan aplicadas
```

#### Base snapshot

Todo ChangeSet aplica contra un snapshot base:

```txt
change add_checkout
base snapshot_123
```

Si el grafo actual cambió:

```txt
error E_STALE_BASE
expected snapshot_123
actual snapshot_124

repair_options
  option rebase
    op rebase_change target=change.add_checkout onto=snapshot_124
  end
end
```

#### Rollback

Rollback no deshace texto. Vuelve a un snapshot anterior del grafo:

```txt
snapshot_123 -> snapshot_124
rollback snapshot_124 -> snapshot_123
```

#### Concurrencia

Si dos cambios nacen del mismo snapshot:

```txt
Change A base=snapshot_123
Change B base=snapshot_123
```

y A entra primero:

```txt
current=snapshot_124
```

B debe rebasear y reverificar contra `snapshot_124`.

#### Merge/rebase semántico

Los conflictos son de grafo, no de texto.

Ejemplo mergeable:

```txt
A agrega field User.email
B agrega field User.name
```

Ejemplo conflictivo:

```txt
A renombra User.email
B modifica User.email
```

El toolchain puede hacer semantic rebase cuando las operaciones no colisionan. Si colisionan, requiere decisión explícita.

#### Locks

Cambios críticos pueden bloquear nodos, APIs o contracts:

```txt
op lock_public_api target=module.payment
op lock_behavior target=fn.checkout
op lock_contracts target=type.Order
```

Reglas:

```txt
1. Apply only against base snapshot.
2. No partial apply.
3. Rollback by graph snapshot.
4. Concurrent changes require semantic rebase.
5. Conflicts are graph-level, not text-level.
6. Locks protect critical nodes/contracts/public APIs.
```

### Versioning y schema evolution

Decisión:

```txt
AI Change Language es un protocolo versionado.
No es texto suelto.
Canonicalization es version-aware.
```

Todo ChangeSet declara versión y base snapshot:

```txt
change add_checkout
language acl/1.0
base snapshot_123
```

Forma abreviada permitida:

```txt
change add_checkout acl=1.0 base=snapshot_123
```

#### Qué se versiona

Los schemas evolucionan por separado:

```txt
acl_version       sintaxis del Change Language
op_schema         schema de operaciones
graph_schema      schema del Semantic Graph
core_ir_schema    schema del Core IR
diagnostics       formato de diagnostics
verification      formato de verification report
```

Ejemplo metadata:

```txt
metadata
  acl_version acl/1.0
  graph_schema 3
  core_ir_schema 2
  diagnostics_schema 1
  verification_schema 1
end
```

#### Compatibilidad

Reglas:

```txt
patch versions no rompen
minor versions agregan ops/campos compatibles
major versions pueden romper y requieren migrator
```

Ejemplo:

```txt
acl/1.2 puede leer acl/1.0
acl/2.0 requiere migrator si hay breaking changes
```

#### Migrators

Si cambia el schema, debe existir migración explícita:

```txt
migrator acl_1_to_2
```

Operación:

```txt
op migrate_changeset from=acl/1.0 to=acl/2.0 target=change.add_checkout
```

#### Deprecated ops

Las ops no se borran abruptamente:

```txt
op deprecate_operation name=set_effect replacement=add_effect since=acl/1.3 remove_in=acl/2.0
```

El parser puede aceptar formato viejo y canonicalizar al nuevo mientras esté soportado.

#### Stable semantics

Regla:

```txt
La misma canonical_change bajo la misma versión debe producir el mismo structural_diff.
```

Si cambia semántica, debe cambiar versión mayor o existir migrator explícito.

#### Reglas finales

```txt
1. Cada ChangeSet declara acl_version y base snapshot.
2. Schemas versionan por separado.
3. ChangeSets viejos se leen por compatibilidad o migrators.
4. Deprecated ops siguen parseables hasta major version.
5. Canonicalization depende de versión.
6. El historial debe seguir siendo legible y verificable en el futuro.
```

### Ejemplo end-to-end: agregar `cart_total`

Este ejemplo debe seguir las reglas actuales del draft:

- ChangeSet line-oriented.
- `acl_version` y `base snapshot` explícitos.
- `requires`/`expect` son claims verificables.
- `infer_boundary` puede abreviar submitted form.
- canonical form materializa inferencias/defaults seguros.
- `Money` no es Core IR; se usa `Decimal<scale=2, precision=18>` como ejemplo numérico.
- No `null`.
- `Eq`/`Ord` no aparecen porque no se comparan valores.
- Effects explícitos: función pura.
- Verification report separa verified/inferred/proof obligations/unverified/unsafe.

#### Contexto previo asumido

```txt
type CartItem = {
  price: NonNegativeDecimal<scale=2, precision=18>
  quantity: NonNegativeInt
}

type Cart = {
  items: List<CartItem>
}
```

#### Submitted ChangeSet

```txt
change add_cart_total acl=1.0 base=snapshot_001
intent "Add pure cart total calculation"

requires
  assert_exists type.Cart
  assert_exists type.CartItem
  assert_exists type.NonNegativeDecimal
  assert_exists type.NonNegativeInt
  assert_exists module.cart
end

expect
  creates fn.cart_total
  modifies module.cart
  no_new_public_api_except fn.cart_total
  no_unsafe
  no_unverified
end

ops
  op create_function id=fn.cart_total
  op add_param target=fn.cart_total name=cart type=Cart
  op infer_boundary target=fn.cart_total
  op add_contract target=fn.cart_total kind=ensures rule="result >= Decimal.zero"
  op set_body target=fn.cart_total body=@expr.cart_total
  op expose target=fn.cart_total as=api.cart_total
end

block @expr.cart_total
  let total = List.fold(
    cart.items,
    Decimal.zero,
    lambda acc item:
      Decimal.add(acc, Decimal.multiply(item.price, item.quantity))
  )
  return total
end

verify
  target fn.cart_total
  contracts required
  effects required
  refinements required
end

end
```

#### Canonical ChangeSet

```txt
change add_cart_total
language acl/1.0
base snapshot_001

metadata
  intent "Add pure cart total calculation"
end

requires
  assert_exists type.Cart
  assert_exists type.CartItem
  assert_exists type.NonNegativeDecimal
  assert_exists type.NonNegativeInt
  assert_exists module.cart
end

expect
  creates fn.cart_total
  modifies module.cart
  no_new_public_api_except fn.cart_total
  no_unsafe
  no_unverified
end

ops
  op create_function id=fn.cart_total visibility=private
  op add_param target=fn.cart_total name=cart type=Cart
  op set_return target=fn.cart_total type=Decimal<scale=2, precision=18>
  op add_effect target=fn.cart_total effect=pure
  op add_contract target=fn.cart_total kind=ensures rule="result >= Decimal.zero"
  op set_body target=fn.cart_total body=@expr.cart_total hash=expr_abc123
  op expose target=fn.cart_total as=api.cart_total
end

block @expr.cart_total hash=expr_abc123
  let total = List.fold(
    cart.items,
    Decimal.zero,
    lambda acc item:
      Decimal.add(acc, Decimal.multiply(item.price, item.quantity))
  )
  return total
end

verify
  target fn.cart_total
  contracts required
  effects required
  refinements required
end

end
```

#### Verification report

```txt
verification_report add_cart_total

status accepted

verified
  type_safety
  effects_declared
  function_is_pure
  contract_result_non_negative
  expected_diff_matches_actual_diff
end

inferred
  return Decimal<scale=2, precision=18>
  effects pure
end

proof_obligations
  item.price >= Decimal.zero proven by CartItem.price: NonNegativeDecimal<scale=2, precision=18>
  item.quantity >= 0 proven by CartItem.quantity: NonNegativeInt
  Decimal.add preserves non_negative when operands non_negative: proven by stdlib contract
  Decimal.multiply preserves non_negative when operands non_negative: proven by stdlib contract
end

structural_diff
  creates fn.cart_total
  modifies module.cart
  exposes api.cart_total
end

unverified none
unsafe none

end
```

#### Qué demuestra

```txt
1. El LLM escribe una forma abreviada.
2. El toolchain infiere return/effects.
3. Canonical form materializa signature y defaults seguros.
4. El verifier prueba types/effects/contracts/refinements.
5. El expected diff se compara contra el actual diff.
6. Si todo pasa, el graph aplica el ChangeSet como transacción atómica.
```

### Grammar formal del Change Language

Decisión:

```txt
La grammar del Change Language es mínima en sintaxis,
pero completa en semántica mediante op schemas y subgrammars.
```

Principio:

```txt
parser simple
validator inteligente
canonicalizer estricto
```

#### Case conventions

IDs estables usan namespaces explícitos:

```txt
fn.cart_total
type.CartItem
module.checkout
cap.payment.charge
handler.StripePayment
boundary.Stripe
api.checkout
```

Convención:

```txt
namespace.lower_snake
type.PascalCase
fn.lower_snake
module.lower_snake
cap.lower_snake.lower_snake
handler.PascalCase
```

#### Comments

Solo comentarios de línea:

```txt
# comment
```

#### Strings

Strings siempre con comillas dobles:

```txt
"Add checkout flow"
```

Escapes mínimos:

```txt
\" \\ \n \t
```

#### Free text

Free text existe solo como metadata/documentación, nunca como semántica ejecutable o verificable.

Permitido:

```txt
intent
reason
rationale
doc
comment
review_note
human_approval_note
```

Prohibido como free text:

```txt
types
effects
contracts
policies
capabilities
permissions
verification conditions
security rules
migration semantics
```

Regla:

```txt
Free text puede explicar.
Free text no puede decidir.
```

#### Complex values

Valores complejos van como string y luego se parsean con subgrammar específica.

```txt
op set_return target=fn.checkout type="Result<OrderId, CheckoutError>"
op add_contract target=fn.checkout kind=ensures rule="result >= Decimal.zero"
```

El string no queda opaco: canonical form lo convierte a AST tipado:

```txt
TypeExprAST
ContractExprAST
ExprAST
PolicyExprAST
```

Regla:

```txt
Nada semántico queda como string libre en el grafo canónico.
```

#### Refs

Referencias a bloques o rangos:

```txt
@expr.checkout_body
@schema.user_input
@doc.checkout_notes
@range.payment_logic
```

#### Key/value

```txt
key=value
```

Valores permitidos en grammar principal:

```txt
id
ref
string
number
boolean
set/list simple
```

Ejemplos:

```txt
target=fn.checkout
body=@expr.checkout_body
type="CartId"
public=true
effects={database.read:Cart,payment.charge:PaymentProvider}
```

#### Typed blocks

Los blocks declaran tipo explícito:

```txt
block expr @expr.checkout_body
  ...
end

block schema @schema.user_input
  ...
end

block doc @doc.checkout_notes
  ...
end
```

Cada tipo de block tiene subgrammar propia.

#### EBNF aproximado

```txt
change       = "change" ws id attrs? nl change_body "end" nl? ;

attrs        = (ws key "=" value)* ;

change_body  = (intent | section | op | block | verify | comment | blank)* ;

intent       = "intent" ws string nl ;

section      = section_name nl section_body "end" nl ;
section_name = "metadata" | "requires" | "expect" | "ops" | "approval" ;

section_body = (op | assertion | expectation | metadata_entry | approval_entry | comment | blank)* ;

op           = "op" ws verb (ws kv)* nl ;

verify       = "verify" (ws kv | ws bare_word)* nl
             | "verify" nl verify_body "end" nl ;

block        = "block" ws block_kind ws ref attrs? nl block_content "end" nl ;
block_kind   = "expr" | "schema" | "doc" | "range" | "policy" | "test" ;

kv           = key "=" value ;

value        = string | number | boolean | id | ref | set | list ;

set          = "{" value ("," value)* "}" ;
list         = "[" value ("," value)* "]" ;
```

#### Reglas de grammar

```txt
1. Indentation no tiene semántica.
2. `end` cierra sections, blocks y change.
3. Comments solo con `#`.
4. Strings siempre double-quoted.
5. No bare strings con espacios.
6. Ops solo una por línea.
7. Blocks son tipados.
8. Valores complejos se parsean por subgrammar.
9. Parser acepta submitted form; formatter emite canonical form.
```

#### Verificabilidad de grammar

La verificabilidad ocurre por capas:

```txt
1. DSL principal parsea.
2. Subgrammars parsean valores complejos.
3. Op schemas validan keys requeridas y tipos de argumentos.
4. Semantic validator resuelve referencias.
5. Verifier clasifica proof obligations.
```

Regla final:

```txt
La grammar transporta cambios.
La semántica vive en ASTs, schemas, graph y verifier.
```

### AI Change Language completo: propuesta consolidada

Esta sección consolida el diseño completo del AI Change Language como protocolo para que LLMs modifiquen programas sin editar source files tradicionales.

#### Propósito

```txt
AI Change Language no expresa “programas finales”.
Expresa transacciones verificables sobre el Semantic Graph.
```

El humano comunica intención en lenguaje natural. El LLM traduce esa intención a ChangeSets. El toolchain parsea, canonicaliza, verifica y aplica transacciones atómicas sobre el grafo.

#### Non-goals

```txt
- No es un lenguaje general de implementación.
- No es el source of truth.
- No es YAML/JSON para humanos.
- No permite free text como semántica.
- No permite cambios parciales.
- No permite autoaprobación de cambios críticos.
```

#### Pipeline completo

```txt
Human intent
  ↓
LLM submitted ChangeSet
  ↓ parse
Parsed ChangeSet
  ↓ canonicalize
Canonical ChangeSet
  ↓ op schema validation
Validated ChangeSet
  ↓ semantic validation
Graph-aware ChangeSet
  ↓ verification
Verification Report
  ↓ policy/approval gate
Accepted ChangeSet
  ↓ atomic apply
New Semantic Graph Snapshot
```

#### Formato base

```txt
change <id> acl=<version> base=<snapshot>
intent "..."

requires
  ... claims sobre estado previo ...
end

expect
  ... claims sobre diff esperado ...
end

ops
  op <verb> key=value key=value
end

block <kind> @ref
  ... content parsed by subgrammar ...
end

verify
  ... verification requests ...
end

approval
  ... approval requirements ...
end

end
```

Forma corta permitida: el LLM puede escribir ops directo bajo `change`; canonicalizer lo normaliza a la forma completa.

#### Artefactos producidos

Cada ChangeSet procesado produce:

```txt
submitted_change       texto original del LLM
parsed_change          AST del DSL
canonical_change       forma normalizada/version-aware
structural_diff        diff sobre Semantic Graph
verification_report    resultado de verificación
policy_report          resultado de policies externas
approval_record        aprobaciones requeridas/otorgadas
graph_snapshot         nuevo snapshot si se aplica
```

#### Autoridad y confianza

```txt
LLM submitted ChangeSet      propuesta
requires/expect              claims verificables
canonicalizer                normalización mecánica
verifier                     autoridad técnica sobre pruebas
policy engine                autoridad de reglas del proyecto
human/maintainer/security    autoridad de aprobación
Semantic Graph               source of truth
```

Regla:

```txt
La IA propone y declara claims.
El toolchain verifica.
Las policies gobiernan.
Los humanos aprueban lo crítico.
```

#### Policy model

Policies son reglas externas al ChangeSet. La IA puede verlas y debe cumplirlas, pero no puede modificarlas ni autoaprobar excepciones.

Formato conceptual:

```txt
policy project.default {
  deny unsafe
  deny unverified_public_api
  require_approval public_api_changed by=maintainer
  require_approval unsafe_requested by=security
  max_new_capabilities 2
  require_tests_for public_function_added
  require_verification contracts,effects,types
}
```

Policies comunes:

```txt
no_unsafe
no_unverified_public_api
no_new_public_api_without_approval
max_new_capabilities <n>
no_capability <capability>
require_tests_for <condition>
require_docs_for <condition>
require_human_approval <condition>
require_security_approval <condition>
deny_external_boundary_without_contract
deny_handler_without_trust_level
```

Policy result:

```txt
policy_report
  status passed | failed | approval_required
  violations [...]
  approvals_required [...]
end
```

#### Approval model

Approvals son registros externos, no free text autoritativo dentro del ChangeSet.

Approval types:

```txt
human_approval
maintainer_approval
security_approval
architecture_approval
runtime_approval
policy_exception
```

Ejemplo:

```txt
approval
  require_if public_api_changed by=maintainer
  require_if unsafe_added by=security
  require_if assumption_added by=architecture
end
```

Approval record:

```txt
approval_record approval_123
  subject change.add_checkout
  approver role:maintainer
  approves public_api_changed
  timestamp 2026-05-21T00:00:00Z
  note "API reviewed"
end
```

Reglas:

```txt
1. La IA no puede aprobar su propio cambio crítico.
2. Approval records se guardan junto al graph history.
3. Approval puede expirar si cambia el canonical diff.
4. Approval debe referenciar hash del canonical_change.
```

#### Reference model

El lenguaje necesita referencias estables para nodos, bloques, rangos, snapshots y diagnostics.

```txt
NodeRef       fn.checkout, type.Cart, module.payment
BlockRef      @expr.checkout_body, @schema.user_input
RangeRef      @range.payment_logic
SnapshotRef   snapshot_123
ChangeRef     change.add_checkout
DiagRef       diag.E_EFFECT_UNDECLARED#1
HashRef       hash:abc123
```

Range refs deben ser estructurales, no depender solo de números de línea.

Ejemplo:

```txt
range @range.payment_logic
  in @expr.checkout_body
  starts_at node=call.payment.charge
  ends_at node=match.payment_result
end
```

Regla:

```txt
Line/column puede existir para display,
pero refactors y diagnostics deben usar referencias estructurales cuando sea posible.
```

#### ChangeSet composition

ChangeSets pueden relacionarse entre sí.

```txt
metadata
  depends_on change.add_cart_types
  supersedes change.old_checkout_attempt
  conflicts_with change.rewrite_checkout
  part_of change.checkout_epic
end
```

Relaciones:

```txt
depends_on      este cambio requiere otro aplicado antes
supersedes      reemplaza una propuesta anterior
conflicts_with  no puede aplicarse junto a otro sin resolución
part_of         pertenece a un cambio mayor/epic
blocks          impide aplicar otro cambio hasta resolverse
```

Reglas:

```txt
1. dependencies deben estar aplicadas y verificadas.
2. superseded changes no se aplican salvo override explícito.
3. conflicts requieren resolución antes de apply.
4. composition nunca permite saltar verification.
```

#### Op schema model

La grammar no conoce la semántica de cada op. Cada operación tiene schema.

Formato conceptual:

```txt
op_schema add_param {
  required target: NodeRef<FunctionDef>
  required name: Identifier
  required type: TypeExpr
  optional default: Expr

  effects_on_graph modifies target.params
  validates type_resolves
  rejects duplicate_param_name
}
```

Ejemplos:

```txt
op_schema create_function {
  required id: NodeId<FunctionDef>
  optional visibility: Visibility default=private

  effects_on_graph creates id
  rejects id_already_exists
}

op_schema set_return {
  required target: NodeRef<FunctionDef>
  required type: TypeExpr

  effects_on_graph modifies target.signature.return
  validates type_resolves
}

op_schema add_effect {
  required target: NodeRef<FunctionDef|HandlerDef>
  required effect: EffectExpr

  effects_on_graph modifies target.effects
  validates capability_resolves_or_effect_is_pure
}

op_schema expose {
  required target: NodeRef
  required as: PublicApiRef

  effects_on_graph modifies public_exports
  may_trigger public_api_changed
}
```

Regla:

```txt
Parser valida forma.
Op schema valida argumentos.
Semantic validator valida referencias.
Verifier valida consecuencias.
Policy engine valida permisos.
```

#### Operation groups

Core graph ops:

```txt
create, set, add, remove, connect, disconnect,
rename, move, replace, delete
```

Semantic workflow ops:

```txt
infer, verify, assert, lock, refactor, migrate,
derive, generate, approve, reject, deprecate, annotate
```

Runtime/security ops:

```txt
grant, revoke, bind, expose, hide
```

Regla:

```txt
Si una operación tiene semántica especial de verificación,
seguridad, runtime, API pública o workflow,
merece verbo propio.
```

#### Inference in ChangeSets

Inference es permitida para reducir tokens, pero siempre se materializa.

```txt
op infer_boundary target=fn.checkout
```

produce canonical ops:

```txt
op set_return target=fn.checkout type="Result<OrderId, CheckoutError>"
op add_effect target=fn.checkout effect=database.read:Cart
op add_effect target=fn.checkout effect=payment.charge:PaymentProvider
```

Regla:

```txt
Inference may propose.
Verifier must check.
Canonical graph must store explicit result.
```

#### Diagnostics and repair loop

Diagnostics devuelven repairs como ops exactas o choices.

```txt
error E_EFFECT_UNDECLARED
target fn.checkout

repair_options
  option add_effect
    op add_effect target=fn.checkout effect=database.write:Order
  end
end
```

El LLM repara con un nuevo ChangeSet o elige una opción si policy lo permite.

#### Transaction semantics

```txt
ChangeSet applies atomically.
No partial apply.
Apply requires base snapshot.
Rollback uses graph snapshot.
Concurrent changes require semantic rebase.
Conflicts are graph-level.
Locks protect critical nodes/contracts/APIs.
```

#### Versioning

```txt
acl_version
op_schema
graph_schema
core_ir_schema
diagnostics_schema
verification_schema
```

Canonicalization is version-aware. Old ChangeSets remain readable through compatibility or migrators.

#### Complete minimal example

```txt
change add_cart_total acl=1.0 base=snapshot_001
intent "Add pure cart total calculation"

requires
  assert_exists type.Cart
  assert_exists module.cart
end

expect
  creates fn.cart_total
  no_unsafe
  no_unverified
end

ops
  op create_function id=fn.cart_total
  op add_param target=fn.cart_total name=cart type="Cart"
  op infer_boundary target=fn.cart_total
  op add_contract target=fn.cart_total kind=ensures rule="result >= Decimal.zero"
  op set_body target=fn.cart_total body=@expr.cart_total
end

block expr @expr.cart_total
  let total = List.fold(cart.items, Decimal.zero, fn.add_line_total)
  return total
end

verify
  target fn.cart_total
  contracts required
  effects required
  refinements required
end

end
```

#### What makes it AI-native

```txt
1. LLM writes transformations, not files.
2. Syntax is repetitive and repairable.
3. Claims are checked, not trusted.
4. Inference reduces verbosity but canonical graph stays explicit.
5. Diagnostics return machine-actionable repairs.
6. Transactions prevent partial changes.
7. Policies/approvals stop self-approval.
8. Versioning preserves long-term history.
9. Structural refs enable semantic refactor/repair.
10. Source of truth remains Semantic Graph.
```

Reglas del formato:

- Una sola forma canónica de expresar cada operación.
- IDs estables en vez de nombres ambiguos.
- Cambios transaccionales: si algo falla, no entra nada.
- Errores reparables por máquina.
- Formato textual verbose solo como protocolo de escritura, no como almacenamiento principal.

## Semantic Program Graph

El programa real se guarda como nodos y relaciones:

```txt
Nodes:
- Function
- Type
- Module
- Effect
- Capability
- Contract
- Invariant
- Test
- Boundary

Edges:
- calls
- reads
- writes
- emits
- depends_on
- proves
- breaks_if_changed
```

Esto permite refactors seguros, análisis de impacto y contexto semántico para la IA.

## Core IR verificable

El Core IR es la capa pequeña y formal que el toolchain entiende de verdad.

No es el lenguaje que escribe el humano ni necesariamente el LLM. Es la representación canónica que se type-checkea, se verifica y se compila.

### Objetivo del Core IR

```txt
Ser lo suficientemente chico para tener semántica formal,
pero lo suficientemente expresivo para compilar un lenguaje general-purpose.
```

### Borrador preliminar de primitivas posibles

> Estado: pendiente de decisión. Esta sección no representa una decisión final; lista candidatos para discutir.

#### Definiciones de programa

```txt
ModuleDef       módulo con imports/exports semánticos
TypeDef         definición de tipo
FunctionDef     función pura o effectful
CapabilityDef   efecto externo provisto por runtime/paquete
ContractDef     requires/ensures/invariant asociado a tipos o funciones
```

#### Tipos

```txt
Primitive       Int, Bool, Text, Unit, Bytes
Record          producto nominal: { field: Type }
Variant         suma nominal: CaseA | CaseB(payload)
List<T>         colección finita
Option<T>       Some<T> | None
Result<T, E>    Ok<T> | Err<E>
Function        (A, B) -> C with effects
Refinement      Type where predicate
```

`Option` y `Result` pueden implementarse como variants, pero conviene tratarlos como tipos estándar porque son centrales para errores y ausencia de valores.

#### Expresiones

```txt
Literal         valor constante
Var             referencia local
Let             binding inmutable
If              branch booleano
Match           pattern matching sobre variants
RecordNew       construir record
FieldGet        leer campo
FieldSet        update inmutable de record
VariantNew      construir caso de variant
Call            llamada a función declarada
EffectCall      llamada a capability externa
FunctionRef     referencia a función nombrada
Lambda          función anónima pura/effectful
```

#### Contratos

```txt
Requires        precondición
Ensures         postcondición
Invariant       regla que debe preservarse
Assume          frontera explícita no demostrada localmente
Assert          obligación verificable dentro del programa
```

#### Efectos

```txt
Pure            sin efectos externos
CapabilityUse   uso de capability declarada
EffectSet       conjunto de capabilities requeridas por una función
Boundary        integración externa asumida/no verificada completamente
```

### Decisiones importantes por discutir

#### Opción: valores inmutables por defecto

Una posibilidad es que el Core IR inicial no tenga mutación local arbitraria. Un update produciría un nuevo valor:

```txt
FieldSet(order, status, Paid) -> new_order
```

La mutación real del mundo vive en effects/capabilities:

```txt
EffectCall database.write:Order(new_order)
```

Tradeoff: simplifica verificación, refactors y razonamiento de LLMs, pero puede alejarse de modelos imperativos tradicionales.

#### Opción: sin excepciones implícitas

Una posibilidad es modelar errores con `Result<T, E>` o capabilities que declaren fallos posibles.

```txt
PaymentProvider.charge(...) -> Result<PaymentReceipt, PaymentError>
```

Tradeoff: el error es parte del tipo y se vuelve verificable, pero puede volver más verboso el IR.

#### Opción: limitar loops arbitrarios al principio

Para mantener verificabilidad inicial, una opción sería preferir:

```txt
List.map
List.fold
recursión con métrica de terminación
```

Tradeoff: mejora verificación de terminación, pero restringe estilo imperativo. Un `while` general podría existir si declara variante/medida de terminación o queda marcado como menos verificable.

#### Opción: dejar concurrencia fuera del primer núcleo

La concurrencia es general-purpose, pero podríamos dejarla fuera del primer Core IR para no mezclar demasiados problemas. Más adelante podría agregarse como capabilities/constructos controlados:

```txt
task.spawn
task.await
channel.send
channel.receive
```

### Ejemplo IR: función pura

```txt
FunctionDef fn.cart_total
  params:
    cart: Cart
  returns: Money
  effects: Pure
  ensures:
    result >= Money.zero
  body:
    Call List.fold(
      FieldGet(Var cart, items),
      Money.zero,
      FunctionRef fn.add_line_total
    )
```

### Ejemplo IR: función con efectos

```txt
FunctionDef fn.checkout
  params:
    cartId: CartId
  returns: Result<OrderId, CheckoutError>
  effects:
    database.read:Cart
    database.write:Order
    payment.charge:PaymentProvider
    event.emit:OrderPaid
  body:
    Let cart = EffectCall database.read:Cart(cartId)
    Let total = Call fn.cart_total(cart)
    Let payment = EffectCall payment.charge:PaymentProvider(total)
    Match payment:
      Ok(receipt) ->
        Let order = Call fn.create_paid_order(cart, receipt)
        EffectCall database.write:Order(order)
        EffectCall event.emit:OrderPaid(order.id)
        Ok(order.id)
      Err(error) ->
        Err(CheckoutPaymentFailed(error))
```

### Candidatos a quedar fuera del primer Core IR

```txt
- clases/objetos con herencia
- reflection
- eval dinámico
- macros arbitrarias
- threads nativos
- punteros crudos
- excepciones implícitas
- FFI sin boundary declarado
```

No significa que el lenguaje nunca pueda tener estas cosas. Significa que probablemente no pertenezcan al núcleo verificable inicial, pero debe decidirse.

La regla importante:

```txt
No verificamos texto. Verificamos IR con semántica formal.
```

## Type system completo: diseño propuesto

Estado: en diseño. Las decisiones acordadas se marcan explícitamente.

### Nominal vs structural typing

Decisión:

```txt
Los tipos son nominales por defecto.
El structural typing existe solo mediante constraints explícitas.
```

Razón:

- protege identidad de dominio
- evita mezclar tipos que “se parecen” pero significan cosas distintas
- mejora refactor safe
- encaja con Semantic Graph e IDs estables
- reduce errores de LLM por similitud superficial

Ejemplo:

```txt
type UserId = Id
type OrderId = Id
```

Aunque ambos bajen a `Id`, no son compatibles automáticamente:

```txt
UserId != OrderId
```

Structural constraints explícitas:

```txt
fn get_email<T>(value: T) -> Email
  where T has field email: Email
```

Regla:

```txt
La compatibilidad por forma nunca es implícita.
Debe declararse como constraint.
```

### Subtyping y conversions

Decisión:

```txt
No hay subtyping general implícito.
Usamos conversions explícitas + interface constraints.
Permitimos refinement subtyping/erasure limitado y trackeado por verifier.
```

Reglas:

```txt
1. Un tipo nominal no es subtipo automático de otro tipo nominal.
2. Las relaciones de dominio se expresan con interfaces/constraints, no herencia implícita.
3. Las conversions entre tipos nominales son explícitas.
4. Los refinement types pueden degradarse a su base mediante erasure controlado.
5. El verifier debe trackear cuándo se pierde información de refinement.
```

Ejemplo:

```txt
type UserId = Id
type OrderId = Id

fn load_user(id: UserId) -> User
```

Esto es inválido:

```txt
orderId: OrderId
load_user(orderId)
```

Debe existir conversión explícita si el dominio la permite:

```txt
load_user(OrderId.toUserId(orderId))
```

Para refinements:

```txt
email: Email
text: Text = erase_refinement(email)
```

El reporte puede indicar:

```txt
Refinement erasure:
- Email -> Text at fn.send_raw_text
```

Motivo: reduce magia, mejora verificación y evita que el LLM mezcle tipos por similitud superficial.

### Inferencia de tipos

Decisión:

```txt
Inferencia local sí.
Inferencia global permitida como análisis/propuesta.
El ChangeSet puede omitir tipos/effects si pide inferencia explícita.
El Semantic Graph canónico siempre guarda signatures resueltas.
Los cambios en boundaries públicas requieren diff/aceptación explícita.
```

Boundaries explícitas:

```txt
- FunctionDef params/return/effects
- Interface methods
- CapabilityDef signatures
- Handler signatures
- Public exports
- FFI boundaries
- Package APIs
- Contract/refinement declarations
```

Ejemplo:

```txt
fn checkout(cartId: CartId)
  -> Result<OrderId, CheckoutError>
  effects {
    database.read:Cart,
    payment.charge:PaymentProvider,
    database.write:Order,
    event.emit:OrderPaid
  }
{
  let cart = database.read<Cart>(cartId)
  let total = cart_total(cart)
  let payment = payment.charge(total)

  match payment:
    Ok(receipt) -> ...
    Err(error) -> ...
}
```

Explícito:

```txt
cartId: CartId
-> Result<OrderId, CheckoutError>
effects { ... }
```

Inferido localmente:

```txt
cart: Cart
total: Money
payment: Result<PaymentReceipt, PaymentError>
```

Regla mental:

```txt
El LLM puede escribir menos.
El grafo canónico no puede ser ambiguo.
```

Ejemplo ChangeSet con boundary inferida:

```txt
change add_checkout

op create_function id=fn.checkout
op add_param target=fn.checkout name=cartId type=CartId
op set_body target=fn.checkout body=...
op infer_boundary target=fn.checkout
op verify

end
```

El toolchain infiere y materializa:

```txt
Inferred boundary:
- return: Result<OrderId, CheckoutError>
- effects:
  - database.read:Cart
  - payment.charge:PaymentProvider
  - database.write:Order
  - event.emit:OrderPaid

Canonicalized into graph: yes
```

Si la API ya existía, la inferencia se verifica contra el contrato público:

```txt
Expected:
fn.checkout(cartId: CartId)
  -> Result<OrderId, CheckoutError>

Inferred:
fn.checkout(cartId: CartId)
  -> Result<PaymentReceipt, PaymentError>

Result:
Boundary contract violation
```

Opciones:

```txt
- adaptar implementación a la signature existente
- proponer API migration explícita
```

Con effects pasa igual:

```txt
Expected effects:
- database.read:Cart

Inferred effects:
- database.read:Cart
- database.write:Order

Result:
Undeclared effect database.write:Order
```

Con refinements:

```txt
Required:
PositiveMoney

Inferred:
Money

Proof obligation:
prove value > Money.zero, validate at runtime, or reject
```

Comparado con TypeScript: TS permite inferir muchos retornos públicos y deja que el contrato quede definido por la implementación. Este lenguaje puede inferir, pero después materializa y congela la signature en el Semantic Graph. Si cambia, aparece como diff transaccional.

Regla final:

```txt
Inference proposes.
Verifier checks.
Canonical graph stores.
Boundary changes require explicit diff.
```

### Generics

Decisión:

```txt
El type system soporta cuatro clases de parámetros genéricos:
- TypeParam
- EffectParam
- CapabilityParam
- ConstParam limitado
```

#### TypeParam

Parámetros de tipo clásicos:

```txt
fn identity<T>(value: T) -> T

List<T>
Map<K, V>
Result<T, E>
Option<T>
```

#### EffectParam

Permite preservar precisión de efectos en funciones genéricas.

```txt
fn map<T, U, e>(
  items: List<T>,
  f: T -> U effects e
) -> List<U>
  effects e
```

Si `f` es pura, `map` es pura. Si `f` usa HTTP, `map` propaga ese efecto.

#### CapabilityParam

Permite abstraer sobre capabilities concretas.

```txt
fn with_retry<T, E, cap>(
  action: () -> Result<T, E> effects { cap }
) -> Result<T, E>
  effects { cap, clock.sleep }
```

Esto sirve para wrappers genéricos de retry, audit, logging, caching o policies sin hardcodear una capability concreta.

#### ConstParam

Permite valores constantes a nivel de tipo, pero de forma limitada/decidible.

```txt
Vector<T, N>
FixedText<MaxLength>
Password<MinLength=12>
CurrencyAmount<Scale>
```

Regla:

```txt
ConstParam no debe convertirse en programación arbitraria a nivel de tipos.
Solo valores simples, decidibles y verificables.
```

Ejemplo combinado:

```txt
fn traverse<T, U, E, e>(
  items: List<T>,
  f: T -> Result<U, E> effects e
) -> Result<List<U>, E>
  effects e
```

Objetivo:

```txt
La abstracción genérica no debe destruir precisión de tipos, effects ni capabilities.
```

### Variance

Decisión:

```txt
Los generics son invariantes por defecto.
No hay user-defined variance en el Core IR inicial.
Function types pueden aplicar reglas seguras internamente.
```

Contexto:

Variance responde preguntas como:

```txt
Si Dog <: Animal,
¿List<Dog> <: List<Animal>?
```

Pero el lenguaje ya decidió:

```txt
No general implicit subtyping.
Nominal typing por defecto.
Conversions/constraints explícitas.
```

Por eso, user-defined variance (`out T`, `in T`) agregaría complejidad sin aportar suficiente valor al modelo base.

Casos reales donde otros lenguajes usan variance:

```txt
ReadOnlyList<out T>
Handler<in T>
Comparator<in T>
Stream<out T>
Sink<in T>
Producer<out T>
Consumer<in T>
```

En este lenguaje se modelan preferentemente con interfaces/constraints explícitas:

```txt
fn render_all<T>(items: ReadOnlyList<T>)
  where T implements AnimalLike
```

o con adapters explícitos:

```txt
dogs.map(dog_to_animal_view)
```

Regla:

```txt
Si necesitamos compatibilidad entre tipos genéricos,
se expresa con constraints/adapters,
no con subtyping genérico implícito.
```

### Interfaces, constraints y coherence

Decisión:

```txt
El lenguaje soporta interfaces/typeclasses estáticas con:
- associated types explícitos y limitados
- default methods con effects/contracts visibles
- blanket impls con reglas estrictas
- orphan rules
- coherence obligatoria
```

#### Associated types

Permiten que una interface declare tipos relacionados.

```txt
interface Repository<T> {
  type Error

  fn get(id: Id<T>) -> Result<T, Error>
}
```

Cada implementación fija esos tipos:

```txt
impl Repository<User> for PostgresUserRepo {
  type Error = DbError
}
```

Regla: associated types deben ser explícitos en el IR y aparecer en el contexto semántico.

#### Default methods

Una interface puede incluir implementación por defecto.

```txt
interface Serializable<T> {
  fn encode(value: T) -> Bytes

  fn encode_text(value: T) -> Text
    effects { pure }
  {
    bytes_to_text(encode(value))
  }
}
```

Regla: todo default method declara effects/contracts igual que cualquier función. No puede esconder effects.

#### Blanket impls

Permiten implementar una interface para familias de tipos.

```txt
impl<T> Serializable<List<T>>
  where Serializable<T>
{
  fn encode(items: List<T>) -> Bytes
}
```

Regla: permitidos, pero deben pasar coherence check y no pueden introducir ambigüedad.

#### Orphan rules

Para evitar conflictos globales:

```txt
Un paquete solo puede implementar una interface para un tipo
si controla la interface o controla el tipo.
```

Ejemplo:

```txt
impl Serializable<User>
```

Permitido si el paquete define `Serializable` o define `User`.

#### Coherence

Coherence significa:

```txt
Para una combinación concreta de interface + tipo,
el compilador siempre debe resolver una única implementación.
```

Si hay dos impls posibles:

```txt
impl Serializable<User> from packageA
impl Serializable<User> from packageB
```

el compilador no elige automáticamente. Falla:

```txt
Error: ambiguous implementation for Serializable<User>

Resolution required:
- choose explicit impl
- remove conflicting import
- create adapter/newtype
```

Regla filosófica:

```txt
La abstracción está permitida.
La ambigüedad no.
```

### Null, ausencia y campos opcionales

Decisión:

```txt
No existen null/nil/undefined en el Core IR.
La ausencia, el fallo y los updates parciales se modelan con tipos explícitos.
```

Tipos estándar:

```txt
Option<T>       valor opcional del dominio
Result<T, E>    operación que puede fallar
PatchField<T>   campo de update/patch con tres estados
```

#### Valor opcional del dominio

```txt
middleName: Option<NonEmptyText>
```

Estados:

```txt
None
Some("Carlos")
```

`Some("")` no es válido si el tipo interno es `NonEmptyText`.

#### Formularios y entradas externas

Los inputs externos pueden traer strings vacíos, `null`, campos ausentes o formatos ambiguos. Eso se resuelve en Boundary/serialization, antes de entrar al dominio.

Ejemplo:

```txt
decode UserProfileForm {
  field middleName:
    input Text
    normalize trim
    empty_as None
    output Option<NonEmptyText>
}
```

#### PATCH/update parcial

Para distinguir:

```txt
no cambiar
setear valor
borrar valor
```

se usa:

```txt
PatchField<T>
```

Estados:

```txt
Unchanged
Set(T)
Clear
```

Ejemplo:

```txt
middleName: PatchField<NonEmptyText>
```

Mapping desde JSON boundary:

```txt
{}                         -> Unchanged
{ "middleName": "Carlos" } -> Set("Carlos")
{ "middleName": null }     -> Clear
```

Regla final:

```txt
Si puede faltar como dato del dominio -> Option<T>
Si puede fallar una operación -> Result<T, E>
Si es una actualización parcial -> PatchField<T>
Si viene null/undefined/"" desde afuera -> se normaliza en Boundary
```

Motivo: `null` mezcla demasiados significados y crea contexto oculto. El Core IR debe conservar semántica explícita para verificación y razonamiento de LLMs.

### Números y tipos de dominio como Money

Decisión:

```txt
Money no es primitiva numérica del Core IR.
Money vive en stdlib/dominio.
```

El Core IR define la física numérica básica:

```txt
Int
UInt
Float
Decimal
```

Con variantes/parámetros:

```txt
Int32
Int64
UInt32
UInt64
Decimal<Scale, Precision>
```

La standard library o paquetes de dominio definen conceptos como:

```txt
CurrencyCode
Money<C>
NonNegativeMoney<C>
Percentage
```

Ejemplo:

```txt
type Money<C> = {
  amount: Decimal<scale=2, precision=18>
  currency: C
}
```

Esto permite seguridad por moneda:

```txt
Money<USD> + Money<EUR> // error
```

Debe existir conversión explícita:

```txt
fx_convert(eurAmount, target=USD, rate)
```

Regla:

```txt
El lenguaje hardcodea física.
La stdlib modela dominio común.
```

Comparación:

- TypeScript no tiene `Money`; suele usar `number`, `bigint` o librerías Decimal.
- Rust no tiene `Money`; suele usar integer minor units, `Decimal` crates y newtypes.
- Este lenguaje debería ofrecer mejores herramientas: nominal types, refinements, const params y stdlib segura.

#### Overflow, precisión y rounding

Decisión:

```txt
Int es matemático/unbounded.
Machine ints tienen rango fijo y operaciones explícitas.
Wrap nunca es default.
Decimal requiere scale/precision y rounding policy explícita cuando aplica.
Conversiones narrowing son explícitas y checked.
```

Tipos:

```txt
Int       entero matemático, sin overflow semántico
UInt      entero natural/matemático, sin overflow semántico
Int32     entero de máquina con rango fijo
Int64     entero de máquina con rango fijo
UInt32    entero de máquina con rango fijo
UInt64    entero de máquina con rango fijo
```

Operaciones sobre machine ints:

```txt
checked_add(a, b)    -> Result<Int64, OverflowError>
wrapping_add(a, b)   -> Int64
saturating_add(a, b) -> Int64
```

Regla:

```txt
El wraparound debe ser explícito.
No existe overflow silencioso en operaciones default.
```

Conversiones narrowing:

```txt
toUInt8(x: Int) -> Result<UInt8, RangeError>
```

No se permite cast silencioso:

```txt
UInt8(x) // inválido si puede truncar/fallar
```

Decimal:

```txt
Decimal<Scale, Precision>
```

Operaciones que puedan exceder precisión o requerir rounding deben declarar política o devolver error:

```txt
decimal_divide(a, b, rounding=Bankers)
  -> Result<Decimal<scale=2, precision=18>, DecimalError>
```

Motivo: el lenguaje no debe permitir que la IA introduzca overflow, truncamiento o rounding implícito.

### Text, Bytes y Unicode

Decisión:

```txt
Text = Unicode válido.
Bytes = datos binarios.
Text != Bytes.
Conversiones Text/Bytes son explícitas.
Validaciones son refinements.
Normalización Unicode es explícita.
```

Tipos core/textuales:

```txt
Text        texto Unicode válido
Bytes       datos binarios
CodePoint   punto de código Unicode
Grapheme    unidad visible de texto para usuario/UI
```

Regla:

```txt
Text no es array de bytes.
Bytes no es texto.
```

Conversiones:

```txt
text_to_bytes(text: Text, encoding=UTF8) -> Bytes
bytes_to_text(bytes: Bytes, encoding=UTF8) -> Result<Text, DecodeError>
```

`bytes_to_text` puede fallar porque no todo binario es texto válido.

Validaciones como refinements/stdlib:

```txt
NonEmptyText = Text where length_graphemes(value) > 0
Email = Text where matches_email(value)
Url = Text where valid_url(value)
Slug = Text where matches_slug(value)
```

Unicode normalization:

```txt
NormalizedText<NFC>
NormalizedText<NFD>
NormalizedText<NFKC>
NormalizedText<NFKD>
```

Ejemplo:

```txt
Username = Text where normalized=NFC && length_graphemes(value) <= 32
```

Motivo: encoding, normalización y graphemes son fuentes comunes de bugs. El Core IR debe distinguir texto humano, bytes binarios, codepoints y graphemes para que el verifier y la IA no mezclen conceptos.

### Collections

Decisión:

```txt
Las colecciones separan semántica de performance.
Son inmutables por default.
La mutabilidad eficiente se expresa con builders/cells.
El orden es explícito.
Hash/equality/order requieren constraints.
```

Tipos principales:

```txt
List<T>          colección ordenada, tamaño dinámico
Set<T>           colección sin duplicados, sin orden semántico
Map<K, V>        diccionario key/value, sin orden semántico
Vector<T, N>     colección de tamaño fijo conocido por ConstParam
OrderedSet<T>    set con orden semántico explícito
OrderedMap<K,V>  map con orden semántico explícito
Array<T>         colección contigua/performance-oriented en stdlib/runtime
```

#### List

```txt
items: List<CartItem>
```

Ordenada por definición. Inmutable por default:

```txt
newItems = List.append(items, item)
```

#### Set

```txt
roles: Set<Role>
```

Requiere constraints:

```txt
Set<T> where Eq<T> && Hashable<T>
```

No tiene orden semántico. Si el orden importa, usar `OrderedSet<T>`.

#### Map

```txt
usersById: Map<UserId, User>
```

Requiere constraints:

```txt
Map<K, V> where Eq<K> && Hashable<K>
```

No tiene orden semántico. Si el orden importa, usar `OrderedMap<K, V>`.

#### Vector

```txt
Vector<Float, 3>  // x,y,z
Vector<Byte, 32>  // hash
```

Usa `ConstParam` para tamaño fijo verificable.

#### Builders

Para construcción eficiente local:

```txt
builder = ListBuilder<T>.new()
builder.push(item)
list = builder.freeze()
```

Regla: builders son mutabilidad local/controlada. El valor final vuelve a ser colección inmutable.

#### Reglas

```txt
1. Collections son inmutables por default.
2. Mutabilidad eficiente se hace con builders o Cell<T> local.
3. El orden nunca se asume: está en el tipo (`List`, `OrderedMap`) o no existe.
4. `Eq`, `Hashable` y `Ord` son interfaces/constraints explícitas.
5. `Array<T>` es tipo de performance en stdlib/runtime, no la colección semántica default.
```

Motivo: el LLM y el verifier no deben inferir orden, igualdad o mutabilidad por accidente. La semántica de colección debe estar en el tipo.

### Equality

Decisión:

```txt
`==` solo existe si el tipo implementa Eq.
No todos los tipos tienen igualdad automática.
Structural Eq puede derivarse solo cuando es seguro.
Custom Eq es permitido y visible.
Floats no tienen igualdad exacta implícita para lógica de dominio.
Handles/resources no usan Eq normal.
```

#### Eq explícito

```txt
fn contains<T>(items: List<T>, value: T) -> Bool
  where Eq<T>
```

Si `T` no implementa `Eq`, no compila.

#### Structural Eq derivado

Permitido para tipos puros cuyos campos también implementan `Eq`:

```txt
derive Eq for Address structural
```

Esto significa que dos `Address` son iguales si todos sus campos relevantes son iguales.

#### Custom Eq

Permitido cuando la igualdad de dominio no coincide con igualdad de todos los campos.

```txt
impl Eq<User> {
  equals(a, b) = a.id == b.id
}
```

Esto evita bugs como comparar `lastLoginAt` cuando semánticamente dos users son iguales por identidad.

#### Floats

No hay igualdad exacta implícita para lógica de dominio:

```txt
approximately_equal(a, b, tolerance)
bitwise_equal(a, b)
```

La igualdad de floating point debe declarar intención: aproximada, bitwise o domain-specific.

#### Handles/resources

Los recursos no usan `Eq` normal. Si se necesita comparar identidad de handles, se usa operación explícita:

```txt
same_handle(a, b)
```

Regla:

```txt
La igualdad es semántica de dominio, no una operación universal automática.
```

### Ordering

Decisión:

```txt
El orden también es semántica explícita.
`<`, `>`, `<=`, `>=`, `sort`, `min` y `max` requieren Ord o PartialOrd según corresponda.
```

#### Total order

```txt
interface Ord<T> {
  fn compare(a: T, b: T) -> Ordering
}

type Ordering = Less | Equal | Greater
```

`Ord<T>` significa que dos valores de `T` siempre son comparables.

Ejemplos posibles:

```txt
Int
UInt
Decimal<Scale, Precision>
DateTimeInstant
NonNaNFloat
```

#### Partial order

```txt
interface PartialOrd<T> {
  fn partial_compare(a: T, b: T) -> Option<Ordering>
}
```

`None` significa que los valores no son comparables.

Casos:

```txt
Float con NaN
sets por inclusión
permisos/roles parcialmente ordenados
versiones o constraints parciales
```

#### Sorting

```txt
fn sort<T>(items: List<T>) -> List<T>
  where Ord<T>
```

Si solo existe `PartialOrd`, se requiere política explícita:

```txt
partial_sort(items, incomparable=error)
partial_sort(items, incomparable=last)
```

#### Floats

`Float` no tiene `Ord` default. Usar wrapper/refinement explícito:

```txt
FiniteFloat = Float where is_finite(value)
NonNaNFloat = Float where not is_nan(value)
```

o comparator explícito.

#### Text ordering

El orden de `Text` para usuarios requiere collation/política explícita:

```txt
lexicographic_order
locale_order(locale)
case_insensitive_order
```

Regla:

```txt
Ordenar es semántica de dominio, no magia universal.
```

### Serialization, decoding y boundaries

Decisión:

```txt
Serialization/decoding no es magia del type system.
Vive en Boundary Protocol + stdlib, conectado al verifier.
No hay auto-serialization universal.
```

Modelo:

```txt
External Data
  ↓
Decoder / Boundary Schema
  ↓
Validated Domain Type
```

Motivo: los datos externos pueden traer `null`, strings vacíos, campos ausentes, campos extra, números fuera de rango, fechas inválidas, emails mal formados, encoding roto, etc. Nada de eso debe entrar al dominio sin validación explícita.

#### Decoding

Ejemplo:

```txt
decode UserInput from Json {
  field email:
    input Text
    normalize trim
    validate Email
    output Email

  field age:
    input Int
    validate value >= 18
    output AdultAge

  field middleName:
    input Text
    normalize trim
    empty_as None
    output Option<NonEmptyText>
}
```

Salida:

```txt
Result<UserInput, DecodeError>
```

#### Encoding

La salida del dominio también es explícita:

```txt
encode UserOutput to Json {
  field id from user.id
  field email from user.email
}
```

No se serializa todo automáticamente. Esto evita filtrar:

```txt
passwordHash
internalNotes
permissions
tokens
```

#### Interfaces de stdlib

Puede existir:

```txt
Encoder<T, Format>
Decoder<T, Format>
```

Pero sus implementaciones deben ser explícitas o derivadas con schema visible:

```txt
derive Decoder<UserInput, Json>
  using schema UserInputJsonSchema
```

Reglas:

```txt
1. Nada externo entra al dominio sin Decoder.
2. Nada interno sale del dominio sin Encoder.
3. Decoders devuelven Result<T, DecodeError>.
4. Null/empty/missing se normalizan en Boundary.
5. Encoding declara qué campos salen.
6. Derivación permitida solo si el schema generado es visible y verificable.
```

### Time, dates y clock

Decisión:

```txt
Time no es una primitiva simple del Core IR.
Time vive en stdlib.
Leer el tiempo actual es capability explícita.
```

Tipos de stdlib:

```txt
Instant          punto absoluto en el tiempo
LocalDate        fecha sin hora
LocalTime        hora sin fecha
LocalDateTime    fecha+hora sin zona
ZonedDateTime    fecha+hora+zona
Duration         duración
TimeZone         zona horaria
```

Capabilities:

```txt
clock.now
clock.monotonic
```

Regla:

```txt
now() no es puro.
```

Ejemplo:

```txt
fn create_order(...)
  effects { clock.now }
```

#### Monotonic time

Para medir duraciones, timeouts o elapsed time, usar reloj monotónico:

```txt
start = clock.monotonic.now()
elapsed = clock.monotonic.elapsed_since(start)
```

No usar wall-clock para mediciones.

#### Time zones y DST

No existe timezone global implícita.

Conversión explícita:

```txt
instant.to_zoned(timeZone)
localDateTime.resolve(timeZone, policy)
```

Algunos horarios pueden ser ambiguos o inexistentes por DST. La resolución requiere política:

```txt
reject
earlier
later
next_valid
```

#### Tests/replay

Como `clock.now` es capability, tests y replay usan handlers:

```txt
handler FixedClock handles clock.now {
  returns Instant("2026-01-01T00:00:00Z")
}
```

Regla final:

```txt
El tiempo siempre es explícito: tipo, zona, fuente y política.
```

### Randomness

Decisión:

```txt
Randomness no es pura.
Randomness vive como capabilities explícitas de stdlib/runtime.
Crypto randomness y deterministic randomness son conceptos separados.
```

Capabilities:

```txt
random.bytes
random.int
random.float
crypto.random.bytes
```

Tipos de stdlib:

```txt
Seed
DeterministicRng
CryptoRng
RandomBytes<N>
```

Reglas:

```txt
1. No existe random() puro.
2. Tests/simulación/replay usan RNG deterministic con Seed explícita.
3. Seguridad/criptografía requiere CryptoRng o crypto.random.*.
4. No se puede usar RNG no criptográfico donde se requiere crypto randomness.
5. El verification report muestra fuentes de randomness.
```

Ejemplo deterministic:

```txt
fn generate_test_user(seed: Seed)
  -> User
  effects { random.int:DeterministicRng }
```

Ejemplo crypto:

```txt
fn generate_token()
  -> AuthToken
  effects { crypto.random.bytes }
```

Regla final:

```txt
El azar siempre declara fuente, propósito y reproducibilidad.
```

## Type system completo: especificación consolidada

Esta sección consolida las decisiones del type system. El objetivo es que el lenguaje sea general-purpose, verificable y fácil de manipular por LLMs sin depender de magia implícita.

### Objetivos

```txt
1. Hacer explícita la semántica importante.
2. Evitar coerciones y conversiones silenciosas.
3. Permitir inferencia segura sin perder contratos públicos.
4. Separar dominio interno de boundaries externos.
5. Preservar precisión de effects/capabilities en generics.
6. Soportar recursos, async y FFI sin ocultar incertidumbre.
```

### Categorías de tipos

```txt
Primitives:
  Unit, Never, Bool

Numbers:
  Int, UInt, Int32, Int64, UInt32, UInt64,
  Float, Decimal<Scale, Precision>

Text/Binary:
  Text, Bytes, CodePoint, Grapheme,
  NormalizedText<Form>

Algebraic data:
  Record, Variant, Tuple

Standard ADTs:
  Option<T>, Result<T, E>, PatchField<T>

Collections:
  List<T>, Set<T>, Map<K, V>, Vector<T, N>,
  OrderedSet<T>, OrderedMap<K, V>, Array<T>

Functions:
  Function<Params, Return, Effects>

Interfaces:
  Interface, Impl, AssociatedType, Dyn<Interface>

Refinements:
  Refinement<Base, Predicate>

Resources:
  Handle<Resource, Mode>

Effects:
  EffectRow, Capability, Handler

Concurrency:
  Task<T>, TaskGroup, Channel<T>

Boundaries:
  ForeignType, Encoded<Format>, Decoded<T>, BoundarySchema

Stdlib domains:
  Instant, LocalDate, LocalTime, LocalDateTime, ZonedDateTime,
  Duration, TimeZone, Seed, DeterministicRng, CryptoRng
```

### Type identity and compatibility

```txt
Types are nominal by default.
Structural compatibility is explicit via constraints.
No general implicit subtyping.
Conversions between nominal types are explicit.
Refinement erasure is allowed only when tracked.
```

Examples:

```txt
UserId != OrderId

fn get_email<T>(value: T) -> Email
  where T has field email: Email
```

### Inference and materialization

```txt
Inference proposes.
Verifier checks.
Canonical graph stores.
Boundary changes require explicit diff.
```

The LLM-facing ChangeSet may omit types/effects when it explicitly asks for inference:

```txt
op infer_boundary target=fn.checkout
```

But the canonical Semantic Graph must store the resolved signature:

```txt
fn.checkout(cartId: CartId)
  -> Result<OrderId, CheckoutError>
  effects { database.read:Cart, payment.charge:PaymentProvider }
```

If later inference changes a public boundary, it becomes a required diff/API migration, not a silent update.

### Generics and constraints

Supported parameter classes:

```txt
TypeParam
EffectParam
CapabilityParam
ConstParam limited to decidable/simple values
```

Goal:

```txt
Generic abstraction must preserve type/effect/capability precision.
```

Example:

```txt
fn traverse<T, U, E, e>(
  items: List<T>,
  f: T -> Result<U, E> effects e
) -> Result<List<U>, E>
  effects e
```

### Interfaces and dispatch

```txt
Static dispatch by default.
Dynamic dispatch only via Dyn<Interface>.
Interfaces require contracts.
Method effects are part of the interface.
Implementations must satisfy contracts.
Coherence is mandatory.
```

Interface system:

```txt
Associated types: yes, explicit/limited.
Default methods: yes, effects/contracts visible.
Blanket impls: yes, strict coherence checks.
Orphan rules: yes.
Ambiguous impls: compile error.
```

### Variance

```txt
Generics are invariant by default.
No user-defined variance in the initial Core IR.
Function types may apply safe internal variance rules.
Generic compatibility uses constraints/adapters, not implicit subtype variance.
```

### Absence, failure, and partial updates

```txt
No null/nil/undefined in Core IR.
Option<T> for domain absence.
Result<T, E> for fallible operations.
PatchField<T> for partial updates.
External null/undefined/empty values normalize at Boundary.
```

Patch states:

```txt
Unchanged
Set(T)
Clear
```

### Numeric policy

```txt
Int/UInt are mathematical/unbounded.
Machine ints have fixed ranges and explicit operations.
No default wraparound.
Narrowing conversions are checked.
Decimal requires scale/precision and explicit rounding policy when needed.
Money is stdlib/domain, not Core IR primitive.
```

### Text and binary policy

```txt
Text is valid Unicode.
Bytes is binary.
Text != Bytes.
Conversion is explicit.
CodePoint and Grapheme are distinct.
Validation uses refinements.
Unicode normalization is explicit.
```

### Collections policy

```txt
Collections are immutable by default.
Builders/Cell<T> provide controlled local mutation.
Order is encoded in the type.
Eq/Hashable/Ord are explicit constraints.
Array<T> is performance-oriented stdlib/runtime type, not default semantic collection.
```

### Equality and ordering

```txt
`==` requires Eq<T>.
Structural Eq can be derived only when safe.
Custom Eq is visible.
Float equality requires explicit approximate/bitwise/domain comparator.
Handles/resources use explicit identity operations, not normal Eq.
```

Ordering:

```txt
Ord<T> for total order.
PartialOrd<T> for partial order.
sort requires Ord<T>.
partial_sort requires policy for incomparable values.
Float has no default Ord.
Text user-facing ordering requires explicit collation.
```

### Boundaries and serialization

```txt
No universal auto-serialization.
External data enters through Decoder/BoundarySchema.
Internal data exits through Encoder.
Decoders return Result<T, DecodeError>.
Derived encoders/decoders must expose visible/verifiable schemas.
```

Rule:

```txt
Nothing external enters the domain without Decoder.
Nothing internal leaves the domain without Encoder.
```

### Time and randomness

```txt
Time lives in stdlib, not as one primitive.
clock.now and clock.monotonic are capabilities.
No implicit global timezone.
DST/ambiguous local time requires policy.
Randomness is a capability, not pure.
Deterministic RNG and crypto RNG are separate.
```

### Resources and mutability interaction

```txt
Normal values are immutable by default.
Local mutation uses Cell<T>.
External mutation uses EffectCall.
Resources use Handle<Resource, Mode>.
Modes: Copy, Affine, Linear, Shared.
Shared requires capability/concurrency-safe type.
```

### Trust and verification states

Every type-level claim can feed proof obligations. Results are reported as:

```txt
proven
runtime_checked
assumed
unverified
unsafe
failed
```

The type system never hides uncertainty. If something cannot be proven statically, it must be runtime-checked, assumed explicitly, marked unverified/unsafe, or rejected.

### Non-goals

```txt
- No implicit null.
- No implicit general subtyping.
- No implicit numeric narrowing.
- No implicit wraparound.
- No implicit serialization.
- No implicit timezone.
- No universal equality/order.
- No hidden dynamic dispatch.
- No random/clock as pure operations.
```

## Core IR completo: diseño propuesto

Estado: propuesta de diseño para revisar y ajustar. La intención es diseñar el lenguaje completo en papel, aunque la implementación sea gradual.

### Decisión base

El Core IR semántico debería ser:

```txt
ML-like + effect rows + contracts + capabilities + resource handles
```

No debería ser directamente WASM-like ni puro SSA-like. Esos sirven mejor como IR bajo y target de ejecución.

Arquitectura:

```txt
Semantic Graph
  ↓
Semantic Core IR      intención, tipos, effects, contratos, recursos
  ↓
ANF IR                compiler IR principal; orden explícito de efectos
  ↓
SSA                   lowering mecánico para backend/optimizador
  ↓
WASM/native           artefacto ejecutable
```

### 1. Identidad y metadata

Todo nodo relevante del Core IR debe tener identidad estable.

```txt
NodeId          id estable: fn.cart_total, type.Money, cap.payment.charge
Name            alias humano/LLM, no identidad real
Hash            hash estructural/content-addressed
Span/View       referencia opcional a vista textual generada
Provenance      ChangeSet que creó/modificó el nodo
TrustLevel      verified | assumed | unverified | unsafe
```

Esto permite refactors seguros, contexto semántico, auditoría e historial.

### 2. Definiciones top-level

Primitivas de definición:

```txt
ProgramDef
ModuleDef
ImportDef
ExportDef
TypeDef
FunctionDef
CapabilityDef
EffectAliasDef
ContractDef
InvariantDef
TestDef
BoundaryDef
PackageDef
```

Decisión: módulos y paquetes son parte del modelo semántico, no un sistema textual de imports.

### 3. Sistema de tipos

Tipos core:

```txt
Unit
Never
Bool
Int
Float
Decimal
Text
Bytes
Record
Variant
Tuple
List<T>
Map<K, V>
Set<T>
Option<T>
Result<T, E>
Function<Params, Return, Effects>
Handle<Capability, Resource>
Refinement<Base, Predicate>
Generic<T>
Existential
```

Notas:

- `Option` y `Result` pueden bajar a variants, pero son tipos estándar del lenguaje.
- `Handle` representa recursos externos o controlados por runtime: archivos, conexiones, streams, locks, procesos, etc.
- `Never` representa ramas imposibles, errores terminales o funciones que no retornan.

#### Refinement types

Decisión:

```txt
Refinement<Base, Predicate> es parte del Core IR.
```

Un refinement type es un tipo base más una condición lógica:

```txt
PositiveInt = Int where value > 0
Email = Text where matches_email(value)
Percentage = Decimal where value >= 0 && value <= 100
NonNegativeMoney = Money where value >= Money.zero
```

Esto permite que reglas de dominio viajen dentro del tipo, no solo como comentarios/tests.

Ejemplo:

```txt
fn charge(amount: PositiveMoney)
```

Al llamar:

```txt
charge(cart.total)
```

el verificador debe probar o exigir evidencia de:

```txt
cart.total > Money.zero
```

La verificación de refinements es gradual:

```txt
proven           probado estáticamente
runtime_checked  validado en runtime/boundary
assumed          asumido explícitamente
failed           no aceptado
```

Ejemplo de reporte:

```txt
Refinement checks:
- cart.total is NonNegativeMoney: proven
- payment.amount is PositiveMoney: runtime_checked
- external_payload.email is Email: validated at boundary
```

Regla:

```txt
El refinement pertenece al Core IR.
La prueba puede ser estática, runtime-checked o assumption explícita.
Nunca debe quedar implícita.
```

### 4. Tipos algebraicos y pattern matching

El Core IR debe tener ADTs como primitiva semántica:

```txt
RecordType
VariantType
PatternMatch
```

Ejemplo:

```txt
type PaymentResult =
  | Paid(receipt: PaymentReceipt)
  | Declined(reason: DeclineReason)
  | ProviderUnavailable
```

Esto es importante porque errores, estados de dominio y protocolos se modelan mejor como variants que como strings/códigos mágicos.

### 5. Expresiones puras

Primitivas de expresión:

```txt
Literal
Var
Let
If
Match
Lambda
Call
RecordNew
FieldGet
FieldUpdate
TupleNew
VariantNew
ListNew
MapNew
SetNew
IndexGet
```

Decisión propuesta: el Core IR es expression-oriented. Incluso ramas y matches producen valores.

### 6. Mutabilidad

La mutabilidad debe existir, pero no como default invisible.

Decisión:

```txt
Mutabilidad local sí,
pero explícita, no por default,
encerrada en Cell<T>,
no escapable sin permiso,
y normalizable a ANF/SSA.
```

Modelo:

```txt
Local immutable values       default
Local mutable cells          explícitas y acotadas
External mutation            solo por capabilities
Resource mutation            solo vía Handle<T>
```

Primitivas:

```txt
CellNew<T>
CellGet<T>
CellSet<T>
```

Reglas:

- La mutabilidad local no puede escapar sin tipo/capability que lo declare.
- La mutación externa siempre es `EffectCall`.
- El verificador debe poder distinguir cálculo puro, mutación local y mutación del mundo.
- Una `Cell<T>` no puede cruzar un boundary async/task sin `Shared` o capability concurrency-safe.
- Antes de verificación/backend, las cells locales deben normalizarse a ANF/SSA cuando sea posible.

Ejemplo:

```txt
let total = CellNew(Money.zero)

ForEach cart.items item:
  CellSet(total, Money.add(CellGet(total), item.price))

return CellGet(total)
```

Reporte esperado:

```txt
Local mutation:
- total: local-only, non-escaping, normalized
```

Caso inválido:

```txt
TaskSpawn(lambda: CellSet(total, x))
```

Sin `Shared`/capability segura, esto es error porque la cell local cruzaría una frontera concurrente.

### 7. Control flow

Primitivas:

```txt
If
Match
Loop
Break
Continue
Return
ForEach
Fold
```

Decisión propuesta:

- `Loop` existe en el lenguaje completo, pero debe llevar metadata de verificación.
- Si no tiene prueba/medida de terminación, queda marcado como `partial` o `unverified termination`.
- `ForEach` y `Fold` son formas estructuradas preferidas para colecciones finitas.

Metadata de loop:

```txt
termination:
  proven | bounded | assumed | unverified
variant:
  optional expression decreasing each iteration
```

### 8. Errores

Modelo propuesto:

```txt
Errores esperados     Result<T, E>
Ausencia              Option<T>
Fallo imposible       Never / unreachable assertion
Panic/abort           efecto explícito o boundary unsafe
```

Primitivas:

```txt
Ok
Err
Some
None
Unreachable
Abort
```

Decisión: no hay excepciones implícitas invisibles en el Core IR. Si el lenguaje surface ofrece sintaxis cómoda, debe desugar a `Result`/`Option`/effects declarados.

### 9. Effects y capabilities

Primitivas:

```txt
EffectRow
CapabilityUse
EffectCall
CapabilityDef
EffectHandler
BoundaryCall
```

Una función tiene firma:

```txt
Function<Params, Return, Effects>
```

Ejemplo:

```txt
fn checkout(cartId: CartId)
  -> Result<OrderId, CheckoutError>
  effects {
    database.read:Cart,
    database.write:Order,
    payment.charge:PaymentProvider,
    event.emit:OrderPaid
  }
```

El core no hardcodea `database`, `payment` ni `event`. Solo hardcodea el mecanismo de capabilities.

#### Effect handlers

Decisión:

```txt
EffectHandler es parte del Core IR.
```

Una capability declara QUÉ efecto existe. Un handler declara CÓMO se interpreta ese efecto.

```txt
Capability = contrato del efecto
Handler = implementación/interprete explícito de una capability
```

Ejemplo:

```txt
capability database.read<T> {
  fn read(id: Id<T>) -> Result<T, DbError>
}

handler PostgresDb handles database.read<Cart> {
  effects { network.call:Postgres }
  ensures result satisfies database.read contract
}

handler InMemoryDb handles database.read<Cart> {
  effects { pure }
  ensures result satisfies database.read contract
}
```

Reglas:

```txt
1. Todo handler declara qué capabilities maneja.
2. Todo handler declara qué effects usa internamente.
3. Todo handler debe cumplir/probar el contrato de la capability.
4. El runtime elige handlers explícitamente, no por magia global.
5. El verification report muestra qué handler interpreta cada capability.
6. Un handler puede transformar un efecto en otros effects, pero debe declararlo.
```

Ejemplo de transformación:

```txt
handler RetryPayment handles payment.charge:PaymentProvider {
  effects {
    payment.charge:PaymentProvider,
    clock.sleep
  }
}
```

La función de negocio declara QUÉ necesita:

```txt
fn checkout(cartId: CartId)
  effects { payment.charge:PaymentProvider }
```

La ejecución declara CÓMO se interpreta:

```txt
run checkout
  with handler StripePayment
  with handler PostgresDb
```

Esto permite testing, mocks, replay, sandboxing, auditoría y simulación sin cambiar la lógica del programa.

### 10. Contratos y verificación

Primitivas:

```txt
Requires
Ensures
Invariant
Assert
Assume
ProofObligation
VerificationCondition
```

Reglas:

- `Requires` restringe inputs válidos.
- `Ensures` define obligaciones sobre outputs.
- `Invariant` debe preservarse por cambios y efectos relevantes.
- `Assert` debe probarse o quedar como obligación fallida.
- `Assume` crea deuda explícita: no prueba, pero documenta frontera.

El resultado de compilar siempre incluye:

```txt
verified
assumed
unverified
unsafe
```

### 11. Recursos y lifecycle

Para ser general-purpose, el lenguaje necesita modelar recursos: archivos, sockets, streams, locks, procesos, handles GPU, etc.

Modelo propuesto:

```txt
Handle<Capability, Resource>
Acquire
Use
Release
Using
Transfer
```

Reglas:

- Un recurso adquirido debe liberarse o transferirse.
- El runtime puede enforcear cleanup.
- El verifier puede reportar leaks o uso después de release.

Esto puede inspirarse en RAII/linear types, pero sin copiar Rust completo desde el día uno.

#### Ownership de recursos

Decisión:

```txt
Los recursos externos se modelan como Handle<Resource, Mode>.
El modo default para recursos es Affine.
Linear existe para recursos críticos.
```

Modos:

```txt
Copy       valor normal, copiable/inmutable
Affine     se puede mover, no copiar; cleanup automático permitido
Linear     debe consumirse explícitamente exactamente una vez
Shared     acceso compartido solo con capability/tipo concurrency-safe
```

Ejemplos:

```txt
file: Handle<File, Affine>
transaction: Handle<Transaction, Linear>
cache: Handle<Cache, Shared>
```

Reglas:

```txt
1. Un `Handle<_, Affine>` no se copia; se mueve o se libera por scope cleanup.
2. Un `Handle<_, Linear>` debe consumirse explícitamente: commit, rollback, close, release, etc.
3. Un `Handle<_, Shared>` requiere capability o tipo seguro para concurrencia.
4. Usar un handle después de release/transfer es error.
5. Liberar dos veces un handle es error.
6. Todo build genera Resource Lifecycle Report.
```

Ejemplo de reporte:

```txt
Resource Lifecycle Report:
- file handle: released by scope cleanup
- transaction: committed
- lock: released
- stream: transferred to caller
```

Si falla:

```txt
Error:
transaction acquired but not consumed

Suggested repair:
op add_call target=transaction action=rollback
```

Esto da seguridad fuerte de recursos sin copiar completo el modelo de lifetimes de Rust.

### 12. Concurrencia

El diseño completo debe incluir concurrencia, aunque la implementación llegue por fases.

Modelo propuesto: structured concurrency.

Primitivas:

```txt
TaskSpawn
TaskAwait
TaskCancel
TaskGroup
ChannelNew<T>
ChannelSend<T>
ChannelReceive<T>
Select
Timeout
```

Reglas:

- Las tareas pertenecen a un scope.
- No hay tareas huérfanas por default.
- Cancellation es parte del tipo/efecto cuando corresponde.
- Compartir estado mutable entre tareas requiere capability o tipo seguro.

#### Async y tasks

Decisión:

```txt
Async se modela de dos maneras complementarias:
- `can_suspend` como effect.
- `Task*`, `Channel*`, `Select` y `Timeout` como primitivas del Core IR.
```

Razón: suspender y ejecutar concurrencia no son la misma cosa.

```txt
Suspensión = effect
Concurrencia = primitivas + handles + structured concurrency
```

Ejemplo:

```txt
fn fetch_price(productId: ProductId)
  -> Result<Money, HttpError>
  effects { can_suspend, http.call:PricingService }
```

Uso concurrente:

```txt
let task1 = TaskSpawn(fetch_price(productA))
let task2 = TaskSpawn(fetch_price(productB))

let price1 = TaskAwait(task1)
let price2 = TaskAwait(task2)
```

`TaskSpawn` devuelve un recurso:

```txt
Handle<Task<T>, Affine>
```

Reglas:

```txt
1. Toda task pertenece a un scope o TaskGroup.
2. No hay tareas huérfanas por default.
3. Al salir del scope, cada task debe estar awaited, cancelled o transferida explícitamente.
4. Cancellation es explícita y forma parte del modelo.
5. Shared mutable state entre tasks requiere `Handle<_, Shared>` + capability/tipo concurrency-safe.
6. El Resource Lifecycle Report incluye tasks, channels y timeouts.
```

Ejemplo estructurado:

```txt
TaskGroup {
  let priceA = TaskSpawn(fetch_price(productA))
  let priceB = TaskSpawn(fetch_price(productB))

  let a = TaskAwait(priceA)
  let b = TaskAwait(priceB)
}
```

Esto permite async real sin esconder suspensión, cancelación ni lifecycle de tareas.

### 13. FFI y boundaries

Para integrar código externo:

```txt
BoundaryDef
BoundaryCall
ForeignType
ForeignFunction
TrustLevel
AdapterContract
```

Niveles:

```txt
verified     código/modelo probado dentro del sistema
assumed      contrato externo asumido
unverified   no probado pero aislado
unsafe       puede romper garantías, requiere permiso explícito
```

Regla: todo FFI debe declarar capabilities, contratos esperados y trust level.

### 14. Módulos y paquetes

Primitivas:

```txt
PackageDef
ModuleDef
Import
Export
VersionConstraint
CapabilityExport
ContractExport
```

Los paquetes no son solo archivos: exportan tipos, funciones, capabilities, contratos y trust metadata.

Un paquete puede ser:

```txt
verified package
assumed package
unverified package
unsafe package
```

### 15. Traits / interfaces / typeclasses

Para general-purpose real necesitamos abstracción ad-hoc.

Decisión:

```txt
El lenguaje soporta dispatch estático por defecto
y dynamic dispatch explícito mediante Dyn<Interface>.
```

Modelo:

```txt
InterfaceDef     contrato nominal que define operaciones requeridas
ImplDef          implementación de una interfaz para un tipo
WhereConstraint  requisito estático sobre genéricos
AssociatedType   tipo asociado limitado/explicitable
Dyn<Interface>   valor con implementación resuelta en runtime
```

Reglas:

```txt
1. Toda Interface debe declarar contratos verificables.
2. Toda implementación debe cumplir/probar esos contratos.
3. El dispatch estático es el default para generics.
4. El dynamic dispatch solo existe si aparece Dyn<Interface>.
5. Los efectos de cada método forman parte de la interfaz.
6. El verification report debe indicar llamadas con dynamic dispatch.
```

Ejemplo:

```txt
interface PaymentProvider {
  fn charge(amount: Money)
    -> Result<Receipt, PaymentError>
    effects { payment.charge }
    requires amount > Money.zero
    ensures Ok(receipt) => receipt.amount == amount
}

fn checkout(provider: Dyn<PaymentProvider>, cart: Cart)
  -> Result<Order, CheckoutError>
```

La función `checkout` puede verificarse contra el contrato de `PaymentProvider`, aunque la implementación concreta se resuelva en runtime.

Primitivas:

```txt
InterfaceDef
ImplDef
Constraint
AssociatedType
Dyn
```

Ejemplo conceptual:

```txt
interface Serializable<T> {
  fn encode(value: T) -> Bytes
  fn decode(bytes: Bytes) -> Result<T, DecodeError>
}
```

Esta decisión combina lo mejor de typeclasses/interfaces estáticas con dynamic dispatch explícito y verificable.

### 16. Generics

Primitivas:

```txt
TypeParam
EffectParam
CapabilityParam
ConstParam
WhereConstraint
```

El lenguaje necesita parametrizar no solo tipos, sino también effects:

```txt
fn map<T, U, e>(list: List<T>, f: T -> U effects e)
  -> List<U>
  effects e
```

Esto es clave para no perder precisión de effects en funciones genéricas.

### 17. Memoria y ownership

Decisión propuesta: no copiar Rust completo como modelo obligatorio del Core IR, pero sí diseñar recursos con ownership explícito.

Capas:

```txt
Values            inmutables por defecto
Cells             mutación local explícita
Handles           recursos externos con lifecycle
Shared resources  requieren capability/concurrency-safe type
Unsafe memory     solo boundary unsafe
```

Esto deja abierta la puerta a optimización nativa sin meter lifetimes en todo el lenguaje surface.

### 18. Operaciones derivadas, no core

Estas features pueden existir en el lenguaje de alto nivel, pero deberían bajar a primitivas core:

```txt
for              -> ForEach/Fold/Loop
try/?            -> Match sobre Result
async/await      -> TaskSpawn/TaskAwait o efecto async
classes simples  -> Record + functions + interfaces
methods          -> functions con receiver explícito
properties       -> FieldGet/FieldUpdate
exceptions       -> Result/effect/panic explícito
```

### 19. Lista consolidada de primitivas core

```txt
Identity:
  NodeId, Name, Hash, Provenance, TrustLevel

Definitions:
  ProgramDef, PackageDef, ModuleDef, Import, Export,
  TypeDef, FunctionDef, CapabilityDef, ContractDef,
  InvariantDef, BoundaryDef, TestDef, InterfaceDef, ImplDef

Types:
  Unit, Never, Bool, Int, Float, Decimal, Text, Bytes,
  Record, Variant, Tuple, List, Map, Set, Option, Result,
  Function, Handle, Refinement, Generic, Existential

Expressions:
  Literal, Var, Let, If, Match, Lambda, Call,
  RecordNew, FieldGet, FieldUpdate, TupleNew,
  VariantNew, ListNew, MapNew, SetNew, IndexGet

Mutation:
  CellNew, CellGet, CellSet

Control:
  Loop, Break, Continue, Return, ForEach, Fold

Effects:
  EffectRow, CapabilityUse, EffectCall, EffectHandler, BoundaryCall

Contracts:
  Requires, Ensures, Invariant, Assert, Assume,
  ProofObligation, VerificationCondition

Resources:
  Acquire, Use, Release, Using, Transfer

Concurrency:
  TaskSpawn, TaskAwait, TaskCancel, TaskGroup,
  ChannelNew, ChannelSend, ChannelReceive, Select, Timeout

FFI:
  ForeignType, ForeignFunction, AdapterContract

Generics/abstraction:
  TypeParam, EffectParam, CapabilityParam, ConstParam,
  WhereConstraint, InterfaceDef, ImplDef, AssociatedType
```

### 20. Preguntas pendientes del Core IR

Antes de marcar este diseño como decidido, faltan estas decisiones:

```txt
1. ¿Interfaces nominales, traits tipo Rust o typeclasses tipo Haskell/Koka?
   - Decidido: interfaces/typeclasses estáticas por defecto + `Dyn<Interface>` explícito para dynamic dispatch.
2. ¿Effect handlers son primitiva v1 o solo capabilities simples al inicio?
   - Decidido: `EffectHandler` es parte del Core IR; cada handler declara capabilities manejadas, effects internos y contrato cumplido.
3. ¿Refinement types son parte core obligatoria o capa de verifier?
   - Decidido: `Refinement<Base, Predicate>` es parte del Core IR; verificación gradual: proven, runtime_checked, assumed o failed.
4. ¿Ownership de recursos será linear/affine o verificado por análisis más simple?
   - Decidido: recursos como `Handle<Resource, Mode>`; default `Affine`, `Linear` para recursos críticos, `Shared` con capability/tipo seguro.
5. ¿Async será effect, primitive task model o ambos?
   - Decidido: ambos. `can_suspend` es effect; `TaskSpawn`, `TaskAwait`, `TaskCancel`, `TaskGroup`, channels, select y timeout son primitivas Core IR.
6. ¿Compiler IR será ANF, SSA o ambos?
   - Decidido: ANF es el Compiler IR principal; SSA es lowering mecánico para backend/optimizador.
7. ¿Qué nivel de mutabilidad local permitimos sin romper verificación?
   - Decidido: mutabilidad local explícita con `Cell<T>`, inmutable por default, no escapable sin permiso, normalizable a ANF/SSA.
```

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

## Capabilities

El módulo WASM no toca base de datos, red, archivos ni reloj directamente. Pide permisos al runtime.

Ejemplo de manifest generado:

```json
{
  "requires": [
    "database.read:Cart",
    "database.write:Order",
    "http.call:PaymentProvider",
    "event.emit:OrderPaid"
  ]
}
```

Esto sale de dos lugares:

1. Declaración semántica generada por la IA.
2. Análisis automático de efectos sobre el IR.

Si no coinciden, el build falla.

## Efectos

Un efecto es cualquier acción que una función realiza más allá de calcular y devolver un valor.

Ejemplo de función pura:

```txt
fn cart_total(cart) -> Money
effects: pure
```

Ejemplo de función con efectos:

```txt
fn checkout(cartId) -> OrderId
effects:
  database.read:Cart
  database.write:Order
  http.call:PaymentProvider
  event.emit:OrderPaid
```

Regla del lenguaje:

```txt
Valor puro = matemática
Efecto = tocar el mundo
```

El compilador debe rechazar cualquier efecto usado pero no declarado.

## Sistema extensible de efectos

El lenguaje no debe hardcodear todos los efectos posibles. Eso lo haría rígido y no serviría como lenguaje general-purpose.

El core solo entiende el mecanismo:

```txt
capability declarada
capability usada
capability propagada
capability otorgada por runtime
capability auditada
```

Los efectos concretos los definen paquetes o runtimes:

```txt
capability database.read<T>
capability database.write<T>
capability http.call<Service>
capability file.read<Scope>
capability llm.invoke<Model>
capability gpu.compute<Job>
```

Así, si mañana aparece una nueva clase de efecto, no hay que cambiar el lenguaje. Se agrega una capability nueva.

Ejemplo:

```txt
package queue
  capability queue.publish<Topic>
  capability queue.consume<Topic>
end

fn publish_order_paid(event) -> Unit
effects:
  queue.publish:OrderEvents
```

El compilador no necesita saber qué es una queue. Solo necesita verificar que la capability existe, que la función la declara y que el runtime puede proveerla.

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

## Context Server

Para que el LLM no lea muchos archivos, el lenguaje necesita contexto nativo.

En vez de:

```txt
read src/cart.ts
read src/order.ts
read src/payment.ts
```

La IA consulta:

```txt
context uc.checkout
impact field.cart_item.price
contracts module.payment
callers fn.reserve_stock
```

Y recibe slices mínimos:

```txt
UseCase: Checkout

Inputs:
- cartId: CartId

Reads:
- Cart
- Product.stock

Writes:
- Order
- Payment

Calls:
- PaymentProvider.charge

Invariants:
- stock_never_negative
- paid_order_has_payment
- order_total_matches_cart_total
```

El contexto deja de ser textual y pasa a ser computable.

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
  unsafe = block
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
runtime_checked
approved assumptions
```

Bloquea:

```txt
failed
unsafe salvo security exception
unverified
unapproved assumptions
```

#### critical

Uso: pagos, auth, safety, infraestructura crítica.

Permite:

```txt
proven
runtime_checked solo si policy lo acepta
assumed solo con strong approval
```

Bloquea:

```txt
unverified
unsafe
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

## Runtime / capability protocol: propuesta completa

El runtime ejecuta artefactos verificados y controla todo acceso al mundo exterior mediante capabilities explícitas.

### Tesis

```txt
El programa compilado no accede al mundo directamente.
Todo acceso externo pasa por capabilities otorgadas por el host runtime.
```

Principio:

```txt
deny by default
```

Si el módulo pide una capability no otorgada, el host la deniega aunque el WASM intente llamarla.

### Arquitectura

```txt
program.wasm
program.capabilities.json
program.verification.json
program.runtime-profile.json
        ↓
AI Runtime Host
        ↓
Handlers / adapters
        ↓
DB, HTTP, files, clock, random, OS, external APIs
```

### Artefactos de runtime

```txt
program.wasm                 lógica ejecutable sandboxed
program.capabilities.json    capabilities requeridas/verificadas
program.verification.json    report autorizado por hashes
program.runtime-profile.json grants, handlers, limits y policies del profile
```

Regla:

```txt
El runtime solo ejecuta si hashes y manifests coinciden con el verification report.
```

### Capability manifest

Generado desde effects verificados.

Ejemplo:

```json
{
  "module": "module.checkout",
  "requires": [
    "database.read:Cart",
    "database.write:Order",
    "payment.charge:PaymentProvider",
    "event.emit:OrderPaid"
  ]
}
```

El manifest no es autoridad por sí solo. Debe coincidir con:

```txt
verified Core IR effects
ANF effect analysis
handler transformations
verification report artifact hashes
runtime profile grants
```

### Grants por profile

Capabilities se otorgan por profile, módulo/paquete y scope.

Ejemplo:

```txt
profile prod
  grant module.checkout database.read:Cart
  grant module.checkout database.write:Order
  grant module.checkout payment.charge:PaymentProvider via handler.StripePayment
  grant module.checkout event.emit:OrderPaid
end

profile test
  grant module.checkout database.read:Cart via handler.InMemoryDb
  grant module.checkout database.write:Order via handler.InMemoryDb
  grant module.checkout payment.charge:PaymentProvider via handler.FakePayment
  grant module.checkout event.emit:OrderPaid via handler.TestEventBus
end
```

Rules:

```txt
1. Grants are least-privilege.
2. Grants are profile-scoped.
3. Grants can expire or be revoked.
4. Broad grants require approval.
5. Runtime denies capability not granted in active profile.
```

### Host ABI

Decisión:

```txt
Host ABI genérico con schemas tipados.
No imports hardcodeados por DB/HTTP/etc. como modelo principal.
```

Forma conceptual:

```txt
host.call(capability_id, operation, encoded_payload) -> HostResult<encoded_response>
```

Ejemplo:

```txt
host.call(
  capability="database.read:Cart",
  operation="read_by_id",
  payload=encoded(CartId)
)
```

El runtime valida:

```txt
capability granted?
payload schema valid?
handler bound?
limits available?
policy allows call?
```

Puede existir ABI especializada como optimización, pero debe bajar semánticamente al mismo capability call model.

### Payload schemas

Todo payload de capability tiene schema explícito:

```txt
CapabilityInputSchema
CapabilityOutputSchema
CapabilityErrorSchema
```

Ejemplo:

```txt
capability payment.charge:PaymentProvider {
  input PaymentChargeRequest
  output Result<PaymentReceipt, PaymentError>
  errors PaymentProviderUnavailable | PaymentDeclined
}
```

El host valida boundary encoding/decoding con el Boundary Protocol.

### Handler binding

Handlers interpretan capabilities.

```txt
bind payment.charge:PaymentProvider -> handler.StripePayment profile=prod
bind payment.charge:PaymentProvider -> handler.FakePayment profile=test
```

Rules:

```txt
1. Handler must declare handled capabilities.
2. Handler must declare internal effects.
3. Handler must satisfy capability contract.
4. Handler trust level must satisfy profile policy.
5. Handler binding is explicit per profile/environment.
```

### Handler execution model

Handlers pueden ejecutarse:

```txt
inside host runtime
as separate sandboxed process
as remote adapter
as native extension boundary
```

Trust levels:

```txt
verified
assumed
unverified
unsafe
```

Rules:

```txt
unsafe native handler requires strong approval
remote handler requires boundary contract
unverified handler blocked in prod/critical unless policy exception
```

### Runtime checks

Runtime host ejecuta checks materializados:

```txt
decoder validations
refinement checks
capability response validation
range/bounds checks
boundary schema validation
```

Regla:

```txt
runtime_checked only counts if check exists in verified artifact hash.
```

### Audit log

Cada capability call produce audit event.

Campos:

```txt
timestamp
profile
module
function
capability
operation
handler
input_hash
output_hash
result_state
duration
trace_id
verification_report_hash
```

Sensitive payloads no se loguean crudos por default. Se loguean hashes/redacted views según policy.

### Limits and sandboxing

Runtime aplica límites:

```txt
timeout
memory limit
fuel/instruction limit
max capability calls
rate limits
payload size limit
concurrency limit
recursion/stack limit
output size limit
```

Si se excede:

```txt
HostError.LimitExceeded
```

Failure behavior debe estar declarado:

```txt
return Err
abort module
rollback transaction
cancel task group
deny capability response
```

### Determinism, replay and testing

Profiles pueden usar handlers determinísticos:

```txt
FixedClock
SeededRandom
RecordedHttp
InMemoryDb
FakePayment
```

Replay mode:

```txt
replay trace_id=trace_123
  use recorded capability responses
  verify same output hashes
end
```

Rules:

```txt
clock.now is capability
random is capability
external HTTP is capability
deterministic tests bind deterministic handlers
```

### Transactions and rollback

Runtime supports transactional capability groups when handlers provide them.

Example:

```txt
transaction group checkout_tx
  database.write:Order
  event.emit:OrderPaid
end
```

Rules:

```txt
transactional effects must declare commit/rollback semantics
non-transactional external effects must be marked as such
compensation actions must be explicit when needed
```

Example non-transactional:

```txt
payment.charge is non_rollbackable
requires idempotency + compensation/refund policy
```

### Error model

Host calls return typed results:

```txt
HostResult<T> = Ok(T) | Err(HostError)
```

HostError examples:

```txt
CapabilityDenied
HandlerNotBound
PayloadDecodeError
PayloadEncodeError
ContractViolation
Timeout
LimitExceeded
HandlerUnavailable
BoundaryFailure
AuditFailure
ManifestMismatch
```

No implicit exceptions cross the WASM boundary.

### Security model

```txt
deny by default
least privilege grants
profile-scoped capabilities
explicit handler binding
manifest/report hash validation
runtime denial of ungranted imports
audit everything
no raw secrets to modules unless capability grants it
```

Secret access is capability-controlled:

```txt
secret.read:StripeApiKey
```

Rules:

```txt
secrets are never embedded in WASM
handlers receive secrets through host-controlled vault
secret reads are audited/redacted
```

### Capability lifecycle

Capabilities can be:

```txt
declared
verified
bound
active
revoked
expired
denied
```

Revocation:

```txt
revoke module.checkout payment.charge:PaymentProvider profile=prod
```

After revoke, runtime denies new calls. In-flight behavior follows policy:

```txt
allow_complete
cancel
timeout_then_cancel
```

### Runtime profile

Runtime profile includes:

```txt
profile name
verification_report_hash
module hash
capability grants
handler bindings
limits
policies
secrets mapping
audit config
replay config
```

Example:

```txt
runtime_profile prod
  report hash=ver_abc123
  module hash=wasm_def456

  grants
    module.checkout database.read:Cart
    module.checkout payment.charge:PaymentProvider via handler.StripePayment
  end

  limits
    timeout 5s
    memory 128MiB
    max_capability_calls 100
  end

  audit redacted
end
```

### Startup validation

Before running, host validates:

```txt
1. wasm hash matches verification report
2. capabilities manifest hash matches report
3. runtime profile references same report/module
4. required capabilities are granted or intentionally denied by mode
5. handlers are bound and satisfy trust policy
6. limits are configured
7. assumptions used by profile are active/not expired
```

If not:

```txt
runtime_start rejected
```

### Runtime report

Runtime emits execution reports:

```txt
runtime_report <id>
profile prod
module module.checkout
verification_report hash=ver_abc123
status completed | failed | denied | timeout | limit_exceeded

capability_calls
  ...
end

runtime_checks
  ...
end

limits
  ...
end

audit_log hash=audit_123
end
```

### Relation to verification

Verification proves or classifies before execution. Runtime enforces during execution.

```txt
Verifier checks: should this module be allowed to run?
Runtime checks: does this execution obey granted capabilities and materialized checks?
```

Runtime cannot upgrade verification state. It can only enforce and produce evidence.

### Final rules

```txt
1. WASM has no direct world access.
2. Every world access is a capability call.
3. Every capability call requires grant + handler binding.
4. Every handler declares internal effects and trust.
5. Runtime is deny-by-default.
6. Runtime validates artifact hashes before execution.
7. Runtime audits capability calls.
8. Runtime enforces limits.
9. Runtime checks are materialized and hash-covered.
10. Runtime profiles are explicit and versioned.
```

### Open design questions

```txt
1. Exact binary encoding for host.call payloads: canonical JSON, MessagePack, CBOR, or custom binary schema?
2. Whether WASI is used as substrate or avoided behind our host ABI.
3. How much of handler execution can itself be compiled/verified modules.
4. How to standardize distributed tracing across capability calls.
5. Whether capability calls are always async/can_suspend or can be sync by type.
```

## Storage / versioning model: propuesta completa

El storage model define dónde vive el programa. En este lenguaje, el source of truth no son archivos de texto, sino un Semantic Graph versionado.

### Tesis

```txt
Source of truth = content-addressed Semantic Graph.
History = append-only ChangeSets + graph snapshots.
Files/text views = derived artifacts.
```

### Principios

```txt
1. Append-only history.
2. Content-addressed objects.
3. Transactional snapshots.
4. Structural diffs, not text diffs.
5. Branchable graph history.
6. Schema migration aware.
7. Logical delete separated from physical GC.
8. Retention policies control growth.
```

### Object store layout

Conceptual layout:

```txt
graph_store/
  nodes/
  edges/
  snapshots/
  changes/
  diffs/
  reports/
  approvals/
  assumptions/
  boundaries/
  runtime_profiles/
  artifacts/
  indexes/
  migrations/
```

### Graph objects

Cada nodo relevante se guarda como objeto direccionado por contenido.

```txt
node_object
  node_id fn.checkout
  kind FunctionDef
  content_hash hash:abc123
  content ...
  provenance change.add_checkout
  schema core_ir/2
  trust_metadata ...
end
```

Edges también son objetos semánticos:

```txt
edge_object
  source fn.checkout
  relation uses
  target cap.payment.charge
  hash hash:def456
end
```

### Snapshots

Un snapshot es una raíz inmutable del grafo.

```txt
snapshot snapshot_123
  graph_root_hash hash:root123
  parent snapshot_122
  applied_change change.add_checkout
  verification_report ver_abc
  created_at 2026-05-21T00:00:00Z
end
```

Regla:

```txt
Todo ChangeSet aplicado produce un nuevo snapshot.
```

### Change history

Cada cambio aplicado guarda:

```txt
submitted_change
canonical_change
structural_diff
verification_report
policy_report
approval_record
new_snapshot
```

Esto permite auditar:

```txt
qué pidió la IA
qué canonicalizó el toolchain
qué cambió en el grafo
qué se verificó
quién aprobó
qué snapshot resultó
```

### Structural diffs

Los diffs son semánticos:

```txt
structural_diff change.add_checkout
  creates fn.checkout
  modifies module.checkout
  connects fn.checkout uses cap.payment.charge
  exposes api.checkout
end
```

No son diffs de líneas.

### Logical delete

Borrar un nodo es un cambio lógico en un snapshot nuevo.

```txt
op delete target=fn.old_checkout
```

El nodo deja de estar activo en el nuevo snapshot, pero snapshots anteriores pueden seguir referenciándolo.

Se puede representar con tombstone:

```txt
tombstone fn.old_checkout
  deleted_by change.remove_old_checkout
  replacement fn.checkout_v2
end
```

### Physical garbage collection

El storage físico no crece indefinidamente. GC elimina objetos no alcanzables por policies de retención.

Objetos protegidos por:

```txt
active branches
tags/releases
protected snapshots
approval records
audit requirements
retention policies
security/legal holds
```

Regla:

```txt
Semantic history is append-only.
Physical storage is garbage-collected by policy.
```

### Retention policies

Ejemplos:

```txt
keep all snapshots for 90 days
keep releases forever
keep security-critical approvals forever
keep failed draft changes for 7 days
keep audit logs for 1 year
keep prod verification reports forever
```

Policy example:

```txt
retention_policy default
  snapshots all keep_for=90d
  snapshots tagged_release keep_forever
  changes failed_draft keep_for=7d
  reports prod keep_forever
  audit_logs keep_for=1y
end
```

### Compaction

Para evitar cadenas enormes de diffs:

```txt
snapshot_1000_compacted
  graph_root_hash hash:root_compacted
  covers snapshot_1..snapshot_1000
end
```

Compaction preserva:

```txt
current graph state
protected audit records
release snapshots
schema migration metadata
```

Puede archivar diffs antiguos según retention.

### Branching

Branches apuntan a snapshots.

```txt
branch main -> snapshot_200
branch feature.checkout -> snapshot_180
branch experiment.new_effects -> snapshot_150
```

Merges son semánticos:

```txt
merge feature.checkout into main
  base snapshot_180
  target snapshot_200
  semantic_rebase required
end
```

Conflicts son graph-level:

```txt
same node modified incompatibly
public API changed in both branches
capability grant conflict
invariant changed while dependent function changed
```

### Tags and releases

Tags protegen snapshots importantes:

```txt
tag release.v1.0 snapshot_300
tag prod.2026-05-21 snapshot_320
```

Release snapshot incluye:

```txt
graph_root_hash
verification_report_hash
runtime_profile_hash
artifact_hashes
```

### Schema versioning and migrations

Storage debe versionar:

```txt
graph_schema
core_ir_schema
acl_version
verification_schema
runtime_schema
artifact_schema
```

Migrators:

```txt
migration graph_schema_3_to_4
  input graph_schema=3
  output graph_schema=4
  verifies structural_equivalence
end
```

Reglas:

```txt
1. Old snapshots remain readable through compatibility or migrators.
2. Migration creates new snapshot; old snapshot not overwritten.
3. Migration report records equivalence/preserved semantics.
4. Breaking schema changes require explicit migrator.
```

### Indexes

Indexes are derived, rebuildable artifacts.

Examples:

```txt
call graph index
type graph index
capability graph index
invariant dependency index
context server index
full-text docs index
```

Regla:

```txt
Indexes are not source of truth.
They can be rebuilt from graph snapshots.
```

### Artifact store

Large/generated artifacts live separately but are hash-linked.

```txt
artifacts/
  wasm/
  manifests/
  reports/
  runtime_logs/
  generated_sdks/
  generated_docs/
```

Rules:

```txt
1. Artifacts are content-addressed.
2. Verification reports reference artifact hashes.
3. Generated artifacts are derived, not source of truth.
4. Artifact retention can differ from graph retention.
```

### Approval and audit records

Approvals are stored as immutable records.

```txt
approval_record approval_123
  subject change.add_checkout
  canonical_change_hash hash:change_abc
  approver role:maintainer
  approves public_api_changed
  timestamp 2026-05-21T00:00:00Z
end
```

Regla:

```txt
Approval expires if canonical_change_hash changes.
```

### Assumption storage

Assumptions are tracked independently and linked to boundaries.

```txt
assumption stripe_idempotency
  boundary boundary.Stripe
  status active
  expires 2026-12-31
  owner team.payments
end
```

Expired/revoked assumptions affect verification gates.

### Views and files

Text files can exist as views:

```txt
views/
  human_readable/
  llm_context/
  generated_source/
  docs/
```

Rules:

```txt
1. Views are derived from graph.
2. Editing a view directly does not mutate source of truth.
3. To change the program, produce a ChangeSet.
4. View hashes can be used for caching/diff display.
```

### Backup and portability

A project can export:

```txt
graph objects
snapshots
ChangeSets
verification reports
runtime profiles
schemas/migrators
protected artifacts
```

Export bundle:

```txt
project_export
  root_snapshot snapshot_320
  include history=protected
  include artifacts=release
  include schemas=true
end
```

### Integrity checks

Storage verifier checks:

```txt
object hashes match content
snapshot root resolves
changes link to reports
reports link to artifact hashes
approvals reference existing canonical changes
assumptions link to boundaries
indexes match snapshot or are marked stale
```

### Final rules

```txt
1. Source of truth is Semantic Graph, not files.
2. Applied changes create immutable snapshots.
3. History is append-only semantically.
4. Deletes are logical tombstones.
5. Physical deletion happens through GC policy.
6. Retention/compaction prevent unbounded growth.
7. Branches point to snapshots.
8. Diffs are structural, not textual.
9. Schemas evolve through migrators.
10. Views/artifacts are derived and hash-linked.
```

### Open design questions

```txt
1. Concrete storage backend: embedded DB, content-addressed filesystem, object DB, or hybrid?
2. Hash algorithm and canonical serialization format.
3. Distributed collaboration protocol for graph branches.
4. How much history to keep by default for local projects.
5. Whether protected audit history can be externally archived/pruned locally.
```

## Context Server protocol: propuesta completa

El Context Server es la capa que permite que LLMs trabajen con contexto semántico preciso sin leer archivos tradicionales.

### Tesis

```txt
El LLM no debería buscar contexto en archivos.
El sistema debe entregarle el slice mínimo correcto, verificable y actualizado.
```

Regla fundamental:

```txt
Context Server no es RAG sobre archivos.
Es query semántica sobre Semantic Graph + verification/runtime/storage indexes.
```

### Objetivos

```txt
1. Reducir contexto textual irrelevante.
2. Evitar contexto viejo/stale.
3. Dar dependencias, effects, contracts y riesgos explícitos.
4. Ayudar a refactors seguros.
5. Explicar por qué algo existe o falla.
6. Entregar respuestas parseables por LLM/tooling.
7. Respetar permisos, redaction y policies.
```

### Fuentes de verdad

El Context Server lee de:

```txt
Semantic Graph snapshots
Core IR / ANF indexes
Verification reports
Structural diffs
ChangeSet history
Runtime profiles
Capability manifests
Boundary/assumption registry
Package trust metadata
Audit/runtime reports when allowed
Derived indexes
```

No inventa facts. Si algo no está en estas fuentes, debe marcarlo como missing/unknown.

### Respuesta en dos capas

Decisión:

```txt
Responses tienen dos capas:
- structured: autoritativa
- summary: ayuda no autoritativa
```

Ejemplo:

```txt
context_slice fn.checkout
snapshot snapshot_123
hash ctx_abc

structured
  signature ...
  effects ...
  contracts ...
  dependencies ...
  assumptions ...
  verification_state ...
end

summary
  "checkout reads Cart, charges PaymentProvider, writes Order, and depends on stripe_idempotency assumption."
end
```

Regla:

```txt
Si summary y structured difieren, gana structured.
```

### Snapshot binding and freshness

Toda respuesta debe estar ligada a snapshot/hash.

```txt
snapshot snapshot_123
context_hash ctx_abc
graph_root_hash hash:root123
generated_at 2026-05-21T00:00:00Z
```

Si el LLM usa ese contexto en un ChangeSet, debe referenciarlo:

```txt
requires
  assert_context hash=ctx_abc
  assert_base snapshot_123
end
```

Si el graph cambió:

```txt
E_CONTEXT_STALE
```

Repair:

```txt
query context fn.checkout at latest
rebase change onto snapshot_124
```

### Query model

Forma conceptual:

```txt
context_server.query
  kind <query_kind>
  target <NodeRef|Pattern|ChangeRef|ClaimRef>
  snapshot <SnapshotRef|latest>
  scope <scope>
  budget <budget>
  redaction <policy>
end
```

Common queries:

```txt
context <id>
impact <id>
callers <id>
callees <id>
effects <id>
contracts <id>
proofs <id>
resources <id>
boundaries <id>
history <id>
why <claim|node|edge>
diff <change|snapshotA..snapshotB>
risks <id|change>
todo <id|change>
```

### Context query

Devuelve slice general de un nodo.

```txt
context fn.checkout
```

Response fields:

```txt
identity
signature
visibility
effects
contracts
dependencies
callers/callees summary
resources
boundaries
assumptions
verification_state
recent_changes
risks
```

### Impact query

Devuelve qué se puede romper si cambia un nodo.

```txt
impact type.CartItem.price
```

Response:

```txt
direct_dependents
transitive_dependents
affected_contracts
affected_invariants
affected_tests
affected_capabilities
affected_public_apis
required_reverification
risk_level
```

### Call graph queries

```txt
callers fn.cart_total
callees fn.checkout
```

Debe distinguir:

```txt
direct
transitive
dynamic via Dyn<Interface>
handler calls
effect calls
```

Dynamic dispatch entries deben mostrar interface contract y possible impls by profile.

### Effect/capability queries

```txt
effects module.checkout
capabilities profile=prod module.checkout
handlers cap.payment.charge:PaymentProvider profile=prod
```

Response:

```txt
declared_effects
inferred_effects
granted_capabilities
missing_grants
handler_bindings
handler_internal_effects
manifest_requirements
runtime_denials if known
```

### Contract/proof queries

```txt
contracts fn.checkout
proofs invariant.stock_never_negative
obligations fn.checkout
```

Response:

```txt
requires
ensures
invariants
proof_obligations
proof_status
evidence
failed_attempts
repair_options
dependent_nodes
```

### Resource/concurrency queries

```txt
resources fn.process_file
concurrency module.checkout
tasks fn.fetch_prices
```

Response:

```txt
handles_acquired
ownership_modes
release/transfer points
task groups
await/cancel status
channels
shared state
violations or warnings
```

### Boundary/runtime queries

```txt
boundaries module.payment
assumptions boundary.Stripe
runtime profile=prod module.checkout
```

Response:

```txt
boundaries
trust levels
assumptions
expiration/review status
handlers
runtime profile grants
limits
audit availability
```

### History queries

```txt
history fn.checkout
why fn.checkout.effects.payment.charge
diff snapshot_120..snapshot_124
```

Response:

```txt
created_by
modified_by
relevant_changes
approval_records
verification_reports
structural_diffs
rationale annotations
```

`why` queries should trace provenance:

```txt
why does fn.checkout require payment.charge?
  because body node call.payment.charge uses cap.payment.charge
  added_by change.add_checkout
  verified_by report ver_123
```

### Refactor support queries

For safe refactors:

```txt
refactor_context fn.checkout split_by=effects
extract_candidates fn.checkout
move_safety fn.cart_total to=module.pricing
```

Response:

```txt
behavior_locks_needed
contracts_to_preserve
effects_to_preserve
callers_to_update
proofs_to_rerun
possible_conflicts
suggested_refactor_ops
```

### Response schema

Base envelope:

```txt
context_response <id>
schema context/1.0
query_hash hash:query123
snapshot snapshot_123
graph_root_hash hash:root123
context_hash hash:ctx123
generated_at 2026-05-21T00:00:00Z
freshness fresh | stale | unknown
redaction none | partial | restricted

structured
  ...
end

summary
  "..."
end

limits
  tokens_estimated <n>
  truncated true|false
  omitted_sections [...]
end

provenance
  sources [...]
  indexes [...]
  reports [...]
end

end
```

### Budgets and scoping

Queries must be scoped to avoid dumping the world.

Budget fields:

```txt
max_depth
max_nodes
max_tokens
include_private true|false
include_transitive true|false
include_runtime_logs true|false
profile prod|dev|test
```

If budget is exceeded:

```txt
truncated true
omitted_sections [transitive_callers, runtime_logs]
next_queries suggested
```

The server should suggest follow-up queries instead of flooding context.

### Security and redaction

Context Server enforces access policies.

Redaction applies to:

```txt
secrets
PII
restricted business logic
security-sensitive handlers
runtime payloads
audit logs
```

Rules:

```txt
1. Context query cannot bypass project permissions.
2. Redacted fields must be marked as redacted, not omitted silently.
3. Summary cannot reveal redacted structured data.
4. Security-sensitive context may require approval/session capability.
```

### Freshness and invalidation

Context slices are invalidated by:

```txt
snapshot change
node hash change
dependency hash change
verification report change
runtime profile change
assumption expiration/revocation
policy change
```

ChangeSets using context must include:

```txt
assert_context hash=ctx123
assert_base snapshot_123
```

### Caching and indexes

Context Server can use derived indexes:

```txt
call graph index
type graph index
effect graph index
capability graph index
proof obligation index
invariant dependency index
history/provenance index
runtime audit index
```

Rules:

```txt
1. Indexes are derived, not source of truth.
2. Each response lists index versions/hashes used.
3. Stale index returns freshness=unknown/stale or triggers rebuild.
```

### Failure modes

Possible errors:

```txt
E_CONTEXT_STALE
E_NODE_NOT_FOUND
E_ACCESS_DENIED
E_BUDGET_EXCEEDED
E_INDEX_STALE
E_SNAPSHOT_NOT_FOUND
E_REDACTION_REQUIRED
E_QUERY_AMBIGUOUS
```

Errors should include repair options:

```txt
repair_options
  option query_latest
    context fn.checkout snapshot=latest
  end
  option narrow_scope
    impact type.Cart max_depth=2
  end
end
```

### LLM usage protocol

Before writing a ChangeSet, LLM should request:

```txt
context target
impact target
contracts target
effects target
```

For refactor:

```txt
refactor_context target
impact target
callers target
proofs target
```

For runtime/capability changes:

```txt
effects target
capabilities profile=<profile> target
boundaries target
runtime profile=<profile> target
```

### Non-goals

```txt
- Not a file search engine.
- Not free-form RAG over source text.
- Not an authority for semantic facts outside the graph/reports.
- Not a way to bypass verification.
- Not a replacement for ChangeSets.
```

### Final rules

```txt
1. Every context response is snapshot/hash-bound.
2. Structured data is authoritative; summary is helper only.
3. Context Server reads graph/reports/indexes, not vibes.
4. Redaction is explicit.
5. Stale context must be detected by ChangeSets.
6. Queries are scoped and budgeted.
7. Refactor and impact support are first-class.
8. Context does not approve changes; verifier/policy do.
9. Indexes are rebuildable derived artifacts.
10. The LLM should consume context slices, not source files.
```

### Open design questions

```txt
1. Exact query syntax: line-oriented DSL, RPC JSON, or both?
2. How summaries are generated and verified against structured data.
3. Whether context slices can be signed for distributed agents.
4. Default budgets for different model/context sizes.
5. How much runtime/audit context should be exposed to LLMs by default.
```

## Package / trust model: propuesta completa

El package model define cómo se comparten, importan y verifican unidades reutilizables del ecosistema.

### Tesis

```txt
Un package no es solo código.
Un package exporta tipos, funciones, contracts, capabilities, handlers, proofs y trust metadata.
```

Problema a evitar:

```txt
instalar un paquete sin saber qué effects usa,
qué permissions necesita,
qué assumptions trae,
si está verificado,
o si contiene unsafe.
```

### PackageDef

Un package declara:

```txt
PackageDef
  name
  version
  graph_schema
  core_ir_schema
  trust_level
  exports
  imports
  required_capabilities
  exported_capabilities
  handlers
  contracts
  verification_report
  assumptions
  boundaries
  unsafe_surface
  license
  provenance
end
```

### Trust levels

```txt
verified
assumed
unverified
unsafe
```

#### verified

Package con verification report válido para sus exports y artifacts.

Debe tener:

```txt
verification_report_hash
artifact_hashes
contracts verified
effects declared
capabilities manifest
```

#### assumed

Package aceptado por assumptions/boundaries explícitos.

Debe tener:

```txt
assumptions
boundaries
owner
expiration/review policy
approval records if profile requires
```

#### unverified

Package sin suficiente evidencia.

Regla:

```txt
No puede entrar en prod/critical salvo policy exception explícita.
```

#### unsafe

Package que puede romper garantías del lenguaje/verifier.

Ejemplos:

```txt
native extension
raw memory access
unchecked FFI
capability bypass
non-sandboxed code
```

Regla:

```txt
unsafe package requiere security approval fuerte y unsafe_surface explícita.
```

### Imports

Importar un package no otorga capabilities automáticamente.

```txt
import payments.stripe version="1.2.0"
```

Solo trae símbolos semánticos:

```txt
types
functions
interfaces
contracts
capability definitions
handler definitions
```

Pero runtime grants se declaran aparte por profile.

Regla:

```txt
import != grant
```

### Exports

Todo export público debe declarar:

```txt
signature
effects
contracts
trust state
visibility
stability
```

Ejemplo:

```txt
export fn charge
  signature "PaymentRequest -> Result<PaymentReceipt, PaymentError>"
  effects { payment.charge:PaymentProvider }
  contracts { idempotent_by_key }
  stability stable
end
```

### Capability exports

Un package puede definir capabilities:

```txt
export capability payment.charge:PaymentProvider
```

O handlers:

```txt
export handler StripePayment handles payment.charge:PaymentProvider
```

Reglas:

```txt
1. Capability definition is not a grant.
2. Handler export is not a binding.
3. Runtime profile must explicitly bind handler and grant capability.
```

### Example package

```txt
package payments.stripe version=1.2.0
trust assumed

exports
  handler StripePayment
  capability payment.charge:PaymentProvider
end

requires
  capability http.call:Stripe
  secret StripeApiKey
end

assumptions
  stripe_idempotency
end

boundaries
  boundary.Stripe
end

verification
  report ver_stripe_1_2_0
end

unsafe_surface none
```

Runtime profile still needs:

```txt
grant module.checkout payment.charge:PaymentProvider via handler.StripePayment profile=prod
grant handler.StripePayment http.call:Stripe profile=prod
grant handler.StripePayment secret.read:StripeApiKey profile=prod
```

### Versioning

Packages use semantic versioning plus schema compatibility metadata.

```txt
package_version 1.2.0
graph_schema 3
core_ir_schema 2
acl_version 1.0
```

Compatibility rules:

```txt
patch: no public contract/effect changes
minor: additive compatible exports
major: breaking signatures/contracts/effects
```

Breaking changes require migration metadata:

```txt
migration payments.stripe 1.x -> 2.0
  changed capability payment.charge
  replacement payment.authorize + payment.capture
end
```

### Dependency resolution

Resolver considers:

```txt
version constraints
schema compatibility
trust requirements
profile policy
capability conflicts
handler conflicts
license policy
```

Example:

```txt
dependency payments.stripe "^1.2"
  require trust>=assumed
  deny unsafe
end
```

### Package trust by profile

Trust gates vary by profile:

```txt
draft:    unverified allowed with warning
dev:      unverified private allowed by policy
test:     test-only assumed/unverified allowed
staging:  unverified blocked
prod:     verified or approved assumed only
critical: verified preferred; assumed requires strong approval
```

### Unsafe surface

Unsafe package must expose unsafe surface:

```txt
unsafe_surface
  unchecked_ffi fn.native_hash
  raw_memory module.fast_image
  non_sandboxed handler.NativeCompressor
end
```

Policy can allow specific unsafe surface, not whole-package blanket approval.

### Assumptions and boundaries in packages

Packages can ship assumptions, but they are not automatically trusted.

Consumer project must accept or reject them.

```txt
assumption stripe_idempotency
  boundary boundary.Stripe
  owner payments.stripe
  expires 2026-12-31
end
```

Importing project records approval:

```txt
approve_assumption stripe_idempotency for project.checkout by=security
```

### Package verification report

Package release includes:

```txt
package_verification_report
  package payments.stripe
  version 1.2.0
  exports_verified [...]
  effects_declared [...]
  assumptions [...]
  unsafe_surface [...]
  artifact_hashes [...]
end
```

### Reproducibility

Package should be content-addressed:

```txt
package_hash hash:pkg_abc123
```

Lockfile records:

```txt
name
version
package_hash
trust_level
verification_report_hash
accepted_assumptions
```

### Revocation and advisories

Packages can be revoked or warned:

```txt
security_advisory adv_123
  package payments.stripe
  affected <1.2.3
  severity critical
  reason "idempotency handler bug"
end
```

Verifier checks advisories during dependency verification.

### Package capabilities and least privilege

Package may request capabilities, but project grants them.

```txt
requested_capabilities
  http.call:Stripe
  secret.read:StripeApiKey
end
```

Policy can reject broad requests:

```txt
deny capability file.write:*
deny capability http.call:* unless approved
```

### Importing generated artifacts

Generated SDKs/docs are not package source of truth.

```txt
generated_artifacts
  sdk.typescript hash=...
  docs hash=...
end
```

They are derived and must link back to package graph hash.

### Namespaces

Packages own namespaces:

```txt
pkg.payments.stripe
type.payments.stripe.PaymentRequest
handler.payments.stripe.StripePayment
cap.payments.stripe.payment.charge
```

Imports can alias:

```txt
import payments.stripe as stripe
```

Aliases do not change stable identity.

### Orphan/coherence across packages

Interface impl coherence applies across packages.

Rule:

```txt
A package can implement Interface<T> only if it owns the interface or owns T,
unless an explicit adapter/newtype is created.
```

Conflicting impls are compile errors.

### Final rules

```txt
1. import != grant.
2. capability definition != runtime permission.
3. handler export != handler binding.
4. package trust is explicit and profile-gated.
5. unsafe surface must be explicit and narrowly approved.
6. assumptions from packages require project acceptance.
7. package verification reports are hash-bound.
8. generated artifacts are derived, not source of truth.
9. dependency resolution includes trust/policy, not only version.
10. packages cannot hide effects, capabilities, boundaries, or unsafe.
```

### Open design questions

```txt
1. Package registry protocol and signing model.
2. Whether verified packages require reproducible builds.
3. How to federate trust across organizations.
4. How package proofs are checked locally vs trusted remotely.
5. How to handle package yanking while preserving old builds.
```

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

## Standard library shape: propuesta completa

La standard library define la semántica común del lenguaje. No debe ser un framework, pero tampoco puede limitarse a tipos puros si el lenguaje pretende ser general-purpose.

### Tesis

```txt
Stdlib = semántica común.
Runtime = permiso y ejecución.
```

La stdlib define APIs, tipos, errores, contracts y capabilities. El runtime profile decide si una capability está otorgada.

Ejemplo:

```txt
std.fs.read(path: Path)
  -> Result<Bytes, FsError>
  effects { file.read }
```

La stdlib define `file.read`. El runtime otorga:

```txt
grant module.app file.read:/config profile=prod
```

Regla:

```txt
stdlib defines capabilities
runtime grants capabilities
```

### Non-goals

```txt
- No acceso libre al sistema operativo.
- No framework web obligatorio.
- No DB ORM obligatorio.
- No vendor lock-in.
- No capabilities otorgadas automáticamente.
- No APIs con effects ocultos.
```

### Módulos propuestos

```txt
std.core
std.option
std.result
std.numeric
std.decimal
std.text
std.bytes
std.collections
std.iter
std.encoding
std.json
std.time
std.random
std.crypto
std.io
std.fs
std.net
std.http
std.process
std.env
std.concurrent
std.sync
std.log
std.trace
std.testing
std.boundary
std.capability
std.diagnostics
std.verify
std.runtime
```

### std.core

Base language helpers:

```txt
Unit
Never
Bool
Ordering
Identity
Function helpers
```

Interfaces:

```txt
Eq
Hashable
Ord
PartialOrd
Debug
Display
```

### std.option / std.result

```txt
Option<T> = Some(T) | None
Result<T, E> = Ok(T) | Err(E)
PatchField<T> = Unchanged | Set(T) | Clear
```

Helpers:

```txt
map
and_then
unwrap_or
transpose
collect_results
```

No implicit exceptions.

### std.numeric / std.decimal

Types:

```txt
Int
UInt
Int32
Int64
UInt32
UInt64
Float
Decimal<Scale, Precision>
```

Operations:

```txt
checked_add
wrapping_add
saturating_add
checked_sub
checked_mul
narrowing conversions returning Result
rounding policies
```

Contracts:

```txt
no silent overflow
no silent narrowing
rounding explicit when needed
```

Domain types live above this:

```txt
Money<C>
Percentage
NonNegativeDecimal
```

### std.text / std.bytes

Types:

```txt
Text
Bytes
CodePoint
Grapheme
NormalizedText<Form>
```

Helpers:

```txt
trim
split
join
normalize
length_graphemes
text_to_bytes
bytes_to_text -> Result<Text, DecodeError>
```

Refinements:

```txt
NonEmptyText
Email
Url
Slug
```

### std.collections

Types:

```txt
List<T>
Set<T>
Map<K, V>
Vector<T, N>
OrderedSet<T>
OrderedMap<K, V>
Array<T>
```

Builders:

```txt
ListBuilder<T>
MapBuilder<K, V>
SetBuilder<T>
```

Contracts:

```txt
length >= 0
Set has no duplicates
Map keys unique
order explicit by type
```

### std.iter

Iterator-like abstractions without hiding effects.

```txt
map<T, U, e>
filter<T>
fold<T, U, e>
traverse<T, U, E, e>
```

Effect-polymorphic helpers preserve `EffectParam`.

### std.encoding / std.json

Types:

```txt
Json
BinaryFormat
EncodeError
DecodeError
Encoder<T, Format>
Decoder<T, Format>
BoundarySchema
```

Rules:

```txt
no universal auto-serialization
derive allowed only with visible schema
decoders return Result
encoders declare exported fields
```

### std.time

Types:

```txt
Instant
LocalDate
LocalTime
LocalDateTime
ZonedDateTime
Duration
TimeZone
```

Capabilities:

```txt
clock.now
clock.monotonic
```

Rules:

```txt
now() is not pure
no implicit global timezone
DST ambiguity requires policy
```

### std.random

Types:

```txt
Seed
DeterministicRng
CryptoRng
RandomBytes<N>
```

Capabilities:

```txt
random.bytes
random.int
random.float
crypto.random.bytes
```

Rules:

```txt
randomness is not pure
deterministic and crypto randomness are separate
```

### std.crypto

Common cryptographic primitives with explicit safety contracts.

Examples:

```txt
Hash
Hmac
Signature
PasswordHash
ConstantTimeEq
SecureBytes
```

Capabilities may include:

```txt
crypto.random.bytes
secret.read
```

Rules:

```txt
unsafe/custom crypto discouraged by policy
secrets not exposed as plain Text by default
constant-time comparisons explicit
```

### std.io / std.fs

Types:

```txt
Path
FileHandle
DirectoryHandle
FileError
```

Capabilities:

```txt
file.read
file.write
file.delete
file.list
```

Rules:

```txt
file access requires grants
paths/scopes are capability-constrained
handles use Handle<Resource, Mode>
```

### std.net / std.http

Types:

```txt
Url
HttpRequest
HttpResponse
HttpError
HeaderMap
StatusCode
```

Capabilities:

```txt
network.connect
http.call
```

Rules:

```txt
network access requires grants
hosts/scopes can be constrained
timeouts explicit
retries explicit
```

### std.process / std.env

Types:

```txt
ProcessHandle
ExitCode
EnvVar
```

Capabilities:

```txt
process.spawn
process.signal
env.read
env.write
```

Rules:

```txt
process/env are sensitive capabilities
prod/critical require strict grants
```

### std.concurrent / std.sync

Types:

```txt
Task<T>
TaskGroup
Channel<T>
Mutex<T>
RwLock<T>
Atomic<T>
Timeout
CancellationToken
```

Rules:

```txt
structured concurrency by default
no orphan tasks
shared state requires safe type/capability
timeouts use clock.monotonic
```

### std.log / std.trace

Capabilities:

```txt
log.write
trace.emit
metric.emit
```

Types:

```txt
LogLevel
TraceId
SpanId
Metric
```

Rules:

```txt
logs are effects
PII/secrets redacted by policy
runtime audit separate from app logs
```

### std.testing

Types:

```txt
Test
PropertyTest
Fixture
Golden
TestResult
```

Helpers:

```txt
assert_eq where Eq<T>
assert_approx
expect_error
generate_cases_from_contract
```

Test handlers:

```txt
FixedClock
SeededRandom
InMemoryDb
RecordedHttp
FakeHandler
```

Rules:

```txt
tests are evidence, not automatic proof
property tests link to contracts/invariants
```

### std.boundary

Boundary helpers:

```txt
BoundaryDef
AdapterContract
ForeignType
ForeignFunction
TrustLevel
Assumption
```

Used for:

```txt
FFI
external APIs
native extensions
LLM providers
OS/runtime integration
```

### std.capability

Common capability abstractions:

```txt
CapabilityId
CapabilityGrant
CapabilityManifest
HandlerBinding
HostResult<T>
HostError
```

Common capability definitions:

```txt
database.read
database.write
http.call
file.read
file.write
event.emit
secret.read
clock.now
clock.monotonic
random.bytes
crypto.random.bytes
log.write
trace.emit
```

Again:

```txt
definition != grant
```

### std.diagnostics / std.verify

Types:

```txt
Diagnostic
RepairOption
ProofObligation
VerificationEntry
VerificationReport
PolicyReport
RuntimeCheck
```

Helpers for tooling/LLM repair:

```txt
format_diagnostic
extract_repair_ops
group_obligations
```

### std.runtime

Runtime-facing types:

```txt
RuntimeProfile
RuntimeReport
AuditEvent
LimitConfig
ReplayConfig
ArtifactManifest
```

### Stability tiers

Stdlib APIs should have stability markers:

```txt
stable
experimental
deprecated
unsafe
internal
```

Rules:

```txt
stable changes follow semver
experimental cannot be used in prod without policy
deprecated emits diagnostics
unsafe requires approval
```

### Final rules

```txt
1. Stdlib defines common semantics.
2. Runtime grants execution permissions.
3. Every effectful stdlib API declares capability/effects.
4. No stdlib API hides world access.
5. No universal serialization magic.
6. No time/random/env/process as pure operations.
7. Collections/text/numbers carry contracts.
8. Testing tools produce evidence, not automatic proof.
9. Common capabilities are definitions, not grants.
10. Stdlib remains framework-neutral.
```

### Open design questions

```txt
1. How large v1 stdlib should be versus package ecosystem.
2. Whether database capability belongs in stdlib core or separate official package.
3. Exact crypto API surface and safe defaults.
4. Whether async runtime primitives live in std.concurrent or std.runtime.
5. How to version stdlib independently from language/Core IR.
```

## Tooling / developer workflow: propuesta completa

El tooling define cómo humanos, LLMs y sistemas CI interactúan con el lenguaje.

### Tesis

```txt
El usuario no programa archivos.
El usuario dirige cambios, inspecciona contexto, verifica y ejecuta snapshots.
```

El tooling no es un extra. Es parte del modelo de programación.

### Capas de tooling

```txt
CLI
Interactive shell
LLM agent protocol
Context UI
Graph inspector
Verifier/report viewer
Runtime runner
Package manager
Migration/refactor tools
CI integration
Editor integration
```

### CLI principal

Nombre placeholder:

```txt
ail
```

Comandos base:

```txt
ail init
ail status
ail context
ail change
ail verify
ail apply
ail compile
ail run
ail inspect
ail diff
ail rollback
ail package
ail policy
ail approve
ail doctor
```

### Output modes

Todo comando debe soportar:

```txt
human mode      salida legible/resumida
machine mode    salida estructurada parseable
```

Ejemplos:

```txt
ail verify --json
ail context fn.checkout --format=json
ail inspect diff --format=acl
```

Regla:

```txt
LLMs/tools usan machine mode.
Humanos usan human mode por default.
```

### Project lifecycle

#### Init

```txt
ail init
```

Crea/inicializa:

```txt
graph store
default branch
project policy
runtime profiles
stdlib baseline
package lock
context indexes
```

#### Status

```txt
ail status
```

Muestra:

```txt
current snapshot
branch
pending changes
verification state
stale indexes
runtime profile status
package advisories
```

### Context workflow

```txt
ail context fn.checkout
ail impact type.CartItem.price
ail callers fn.cart_total
ail effects module.payment
ail proofs invariant.stock_never_negative
```

Outputs are snapshot/hash-bound context slices.

Rules:

```txt
1. Context commands never mutate graph.
2. Context output includes snapshot/hash.
3. Context can be used in ChangeSet requires via assert_context.
```

### Change workflow

Create a ChangeSet from text:

```txt
ail change "add pure cart_total function"
```

Create from file/stdin:

```txt
ail change --file change.acl
ail change --stdin
```

Outputs:

```txt
submitted_change
parsed_change
canonical_change
structural_diff preview
```

Rules:

```txt
ail change does not apply by default.
It creates a draft ChangeSet.
```

### Verify workflow

```txt
ail verify change.add_cart_total --profile dev
ail verify --profile prod
```

Outputs:

```txt
verification_report
diagnostics
proof_obligations
policy_report
approval_requirements
```

Rules:

```txt
verify never applies changes.
verify can update derived indexes/reports.
```

### Apply workflow

```txt
ail apply change.add_cart_total
```

Before apply, CLI shows:

```txt
canonical_change hash
structural_diff
verification_report status
policy status
approval status
target snapshot
```

Rules:

```txt
1. apply requires accepted verification report for selected profile.
2. apply creates new snapshot.
3. apply is atomic.
4. apply refuses stale base unless rebase is requested.
```

Automation mode:

```txt
ail apply --yes --policy=ci.allowed
```

Only allowed if project policy permits automation.

### Compile workflow

```txt
ail compile --target wasm --profile dev
ail compile --target wasm --profile prod
```

Inputs:

```txt
snapshot
accepted verification report for profile
runtime profile
```

Outputs:

```txt
wasm/native artifact
capabilities manifest
semantic source map
artifact manifest
compiler report
```

Rules:

```txt
draft/dev/test artifacts are profile-bound
prod runtime rejects non-prod artifacts
```

### Run workflow

```txt
ail run --profile dev module.checkout
ail run --profile test --replay trace_123
```

Runtime validates:

```txt
artifact hashes
verification report
runtime profile
capability grants
handler bindings
limits
```

Outputs:

```txt
runtime_report
audit log reference
capability call summary
runtime check results
```

### Inspect workflow

```txt
ail inspect node fn.checkout
ail inspect snapshot snapshot_123
ail inspect report ver_123
ail inspect artifact checkout.wasm
ail inspect capability payment.charge:PaymentProvider
```

Used for debugging/audit.

### Diff workflow

```txt
ail diff snapshot_120..snapshot_124
ail diff change.add_checkout
ail diff --semantic
```

Diff is structural:

```txt
creates
modifies
deletes/tombstones
connects/disconnects
exposes/hides
effects changed
contracts changed
capabilities changed
```

Text diff is optional derived view only.

### Rollback workflow

```txt
ail rollback to snapshot_123
ail rollback change.add_checkout
```

Rules:

```txt
rollback creates new snapshot
history is not deleted
rollback requires verification if it affects public/prod state
```

### Rebase/merge workflow

```txt
ail rebase change.add_checkout --onto snapshot_124
ail merge feature.checkout into main
```

Uses semantic rebase. Conflicts are graph-level.

Outputs:

```txt
rebase_report
conflicts
repair_options
```

### Refactor workflow

```txt
ail refactor extract-function fn.checkout --range @range.payment_logic --to fn.charge_payment
ail refactor move fn.cart_total --to module.pricing
```

Refactor commands produce ChangeSets, not direct mutations.

Must show:

```txt
behavior locks
contracts preserved
effects preserved
proofs to rerun
```

### Approval workflow

```txt
ail approve change.add_checkout --for public_api_changed
ail approve assumption stripe_idempotency --role security
ail reject change.add_checkout --reason "capability too broad"
```

Rules:

```txt
approval references canonical_change_hash
approval expires if canonical diff changes
approval records are immutable
```

### Policy workflow

```txt
ail policy check change.add_checkout --profile prod
ail policy explain no_unverified_public_api
ail policy set max_new_capabilities=2
```

Policy changes are themselves ChangeSets or admin records, depending project mode.

### Package workflow

```txt
ail package add payments.stripe@1.2
ail package verify
ail package publish
ail package audit
ail package explain payments.stripe
```

Package install does not grant capabilities.

CLI must show:

```txt
trust level
verification report
requested capabilities
assumptions
unsafe surface
advisories
```

### Doctor workflow

```txt
ail doctor
```

Checks:

```txt
graph integrity
index freshness
schema compatibility
artifact hash consistency
runtime profile validity
package advisories
assumption expirations
```

### LLM agent protocol

LLM agents should use machine-mode commands.

Recommended loop:

```txt
1. ail context <target> --json
2. ail impact <target> --json
3. produce ChangeSet
4. ail change --stdin --json
5. ail verify <change> --profile <profile> --json
6. if diagnostics, apply repair ChangeSet
7. show structural diff + report to human/policy
8. ail apply when approved
```

Rules:

```txt
LLM cannot bypass verify/apply gates.
LLM cannot self-approve critical changes.
LLM should include context hashes in ChangeSet requires.
```

### Editor integration

Editor should show:

```txt
semantic graph tree
context slices
structural diffs
verification diagnostics
proof obligations
capability/effect view
runtime profile view
```

Text views are read-only or generate ChangeSets on edit.

### CI workflow

```txt
ail verify --profile prod
ail compile --profile prod --target wasm
ail package audit
ail doctor
```

CI artifacts:

```txt
verification_report
artifact_manifest
capabilities_manifest
policy_report
runtime_profile_validation
```

### Human UX rules

```txt
1. Never show only text diff for semantic changes.
2. Always show structural diff + verification status.
3. Highlight public API, effects, capabilities, assumptions, unsafe.
4. Explain what needs approval and why.
5. Keep summaries short but link to structured details.
```

### Machine output rules

```txt
1. Stable schemas.
2. Versioned outputs.
3. Hash-bound references.
4. No hidden truncation.
5. Repair options machine-actionable.
```

### Safety rules

```txt
1. No command mutates graph without explicit apply/admin action.
2. apply requires accepted report for profile.
3. compile requires accepted report for profile.
4. run validates artifact/runtime profile hashes.
5. approvals cannot be forged by LLM output.
6. package install cannot grant capabilities.
```

### Example daily flow

```txt
ail status
ail context module.cart
ail change "add cart_total"
ail verify change.add_cart_total --profile dev
ail inspect report ver_123
ail apply change.add_cart_total
ail compile --profile dev --target wasm
ail run --profile dev module.cart
```

### Final rules

```txt
1. Tooling operates on graph snapshots and ChangeSets.
2. Human mode summarizes; machine mode structures.
3. Verification/report/diff are visible before apply.
4. Context is semantic and hash-bound.
5. Refactors generate ChangeSets.
6. Packages do not grant capabilities.
7. Runtime profiles are explicit.
8. Rollback creates snapshots, never deletes history.
9. CI uses prod/critical profiles as policy requires.
10. Tooling makes hidden effects and risks impossible to ignore.
```

### Open design questions

```txt
1. Final CLI name.
2. Whether interactive shell is required for v1.
3. How editor edits convert into ChangeSets.
4. Default human approval UX.
5. Whether local projects can disable graph storage for experiments.
```

## Risks / research questions: propuesta completa

Este bloque captura qué puede matar el proyecto, qué partes requieren investigación seria y qué preguntas abiertas deben resolverse durante la implementación completa del producto.

### Tesis

```txt
El riesgo principal no es “hacer un lenguaje”.
El riesgo principal es hacer un sistema demasiado complejo para ser confiable, usable y verificable.
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

### Preguntas de investigación abiertas consolidadas

#### Core IR / type system

```txt
1. Exact formal semantics of Core IR.
2. How powerful refinement predicates can be before solver performance collapses.
3. Whether effect handlers need algebraic effect semantics or simpler handler model is enough.
4. How far resource ownership should go toward linear/affine type theory.
5. Exact model for Dyn<Interface> + contracts + effects.
```

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

Cada riesgo abierto debe registrarse así:

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

### Final rule

```txt
No open question should remain hidden in prose.
Every serious unknown becomes a tracked research question or risk.
```

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
| Source of truth | Programa como Semantic Graph / Core IR, no como source files clásicos. | Parcialmente decidido | v0 |
| AI Change Language | Formato exacto de ChangeSets que escriben los LLMs. | Pendiente | v0 |
| Core IR semántico | Primitivas completas del lenguaje interno verificable. | En discusión | v0 diseño completo, implementación gradual |
| Compiler IR | Si usamos SSA-like como IR bajo para optimización/codegen. | Propuesto | v1 |
| Type system | Primitives, records, variants, generics, refinements, traits/typeclasses. | Pendiente | v0/v1 |
| Error model | `Result`, `Option`, ausencia de excepciones implícitas o alternativa. | Propuesto | v0 |
| Effects/capabilities | Mecanismo extensible de efectos, propagación y autorización runtime. | Parcialmente decidido | v0 |
| Contracts | `requires`, `ensures`, `invariant`, `assert`, `assume`. | Propuesto | v0/v1 |
| Verification model | Qué se prueba formalmente, qué se asume, qué queda unverified. | Parcialmente decidido | v0/v1 |
| Refactor model | Operaciones semánticas de refactor y prueba de preservación observable. | Parcialmente decidido | v1 |
| Module/package system | Imports semánticos, versiones, capabilities exportadas, paquetes no verificados. | Pendiente | v1 |
| Storage/versioning | Graph store, snapshots, deltas, ChangeSet history, hashes. | Parcialmente decidido | v0/v1 |
| Context Server | Consultas semánticas para LLMs: context, impact, callers, contracts. | Parcialmente decidido | v0/v1 |
| Runtime host | Host de WASM que provee capabilities, límites, auditoría y sandbox. | Parcialmente decidido | v0 |
| Executable target | WASM primero; native posible después. | Parcialmente decidido | v0/v1 |
| Native compilation | LLVM/Cranelift/backend propio, sandboxing OS/capability ABI. | Pendiente | v2 |
| Concurrency | Async, tasks, cancellation, channels, structured concurrency. | Pendiente | v1/v2 |
| Resource lifecycle | Ownership/borrowing, linear types, RAII, disposables o capabilities de recurso. | Pendiente | v1/v2 |
| FFI/boundaries | Integración con código externo, trust levels, contracts y aislamiento. | Pendiente | v1 |
| Standard library | Colecciones, text, numbers, time abstracto, serialization, testing. | Pendiente | v0/v1 |
| Package trust model | Verified packages, assumed packages, unverified packages, unsafe boundaries. | Pendiente | v1 |
| Debugging/profiling | Debug semántico sobre graph/IR, no solo stack traces textuales. | Pendiente | v1/v2 |
| Security model | Capabilities, least privilege, audit log, denial by default. | Parcialmente decidido | v0/v1 |
| LLM repair loop | Errores estructurados con reparaciones sugeridas por operación. | Propuesto | v0 |

## Principios para decidir el Core IR completo

El Core IR debe diseñarse completo en papel antes de implementar, pero manteniendo un núcleo estable.

Principios:

```txt
1. Si una feature cambia la semántica del lenguaje, se diseña ahora.
2. Si una feature puede desugarse sin pérdida, no necesita ser primitiva core.
3. Si afecta verificación, effects, concurrencia, FFI o módulos, no se posterga su diseño.
4. El Core IR debe permitir general-purpose real, no solo apps CRUD.
5. El Core IR debe ser fácil de consultar por el Context Server.
6. El Core IR debe poder bajar a SSA/WASM sin perder metadata de verificación.
```

Arquitectura propuesta para IRs:

```txt
Semantic Graph
  ↓
Semantic Core IR      ML-like + effects/capabilities, para razonar/verificar
  ↓
ANF IR                compiler IR principal, para lowering/verificación de efectos
  ↓
SSA                   artifact mecánico para backend/optimizador
  ↓
Executable target     WASM primero; native después
```

Decisión: ANF es el Compiler IR principal; SSA existe como lowering mecánico para backends como LLVM/Cranelift/WASM.

Razón:

```txt
En ANF, el orden de efectos está en la estructura del programa.
En SSA, el orden de efectos tiende a ser metadata/análisis adicional.
```

Para un lenguaje donde effects/capabilities son centrales, ANF es el mejor punto de control para verificación, resource lifecycle, contracts y debugging semántico. SSA sigue siendo útil, pero como artefacto bajo para optimización/codegen.

## Decisiones abiertas

- ¿Conviene forkear Unison o construir un prototipo mínimo desde cero?
- ¿Qué forma exacta tendrá el Core IR completo?
- ¿Qué parte del sistema será trusted core?
- ¿Cómo se versionan snapshots y ChangeSets?
- ¿Cómo se representa concurrencia y async de forma verificable?
- ¿Cómo se integran paquetes externos no verificados?
- ¿Qué subset inicial se implementa sin dejar sin diseñar el lenguaje completo?

## Próximo paso recomendado

Definir el MVP técnico mínimo:

```txt
1. ChangeSet textual simple
2. Parser/canonicalizer
3. Semantic graph en memoria
4. Type checker básico
5. Effect checker básico
6. Capability manifest
7. Compilación mínima a WASM o ejecución interpretada
8. Verification report
```

La meta del MVP no debería ser competir con Python todavía. Debería demostrar esta tesis:

```txt
Un LLM puede construir y modificar programas mediante operaciones semánticas verificables,
sin editar source files tradicionales.
```
