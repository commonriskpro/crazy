# AI Change Language

<!-- Implementation Status: hand-written ACL parser, canonicalizer, typed blocks, verify sections, composition metadata, and apply path exist for the current grammar subset. -->

> Full extracted design. Related: [Core IR](core-ir.md), [Verification](verification.md), [Context Server](context-server.md), [Tooling](tooling.md).

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

<!-- Implementation Status: implemented by `crates/ail-change/src/parser.rs`, not a parser generator. The implemented grammar is intentionally narrower than the full protocol design. -->

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

### Implementation Notes

The current parser is a pure hand-written line parser. It supports the implemented ACL subset documented in `crates/ail-change/src/parser.rs`: `change`, inline attrs, metadata/requires/ops/expect/approval sections, typed blocks, verify short/block forms, comments, and `key=value` op arguments.

Known gaps against the full design:

- Complex values are kept as strings for downstream semantic validation in several paths.
- Expression blocks are carried through as typed blocks; the compiler expression parser only handles the current executable subset.
- Formatting/canonical output is implemented for tested paths, not every future grammar feature.

Code references: `crates/ail-change/src/parser.rs`, `crates/ail-change/src/canonical.rs`, `crates/ail-change/src/apply.rs`.
