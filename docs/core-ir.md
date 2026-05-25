# Core IR and Semantic Graph

<!-- Status: Target design with implemented subset. Core IR data structures cover many designed primitives; executable parsing/lowering currently supports a narrower subset. -->

> Target design. Current implementation scope is called out in status notes and code references. Related: [Type system](type-system.md), [Compiler](compiler.md), [Verification](verification.md), [Storage](storage.md).

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

### Primitivas iniciales consideradas

> Nota histórica: esta lista fue el primer borrador de primitivas. La decisión consolidada está en “Core IR completo: diseño propuesto” más abajo.

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

## Core IR completo: diseño propuesto

Estado: decisión de diseño consolidada. La intención es diseñar el lenguaje completo en papel, aunque la implementación pueda secuenciarse internamente.

### Decisión base

El Semantic Core IR es:

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
  ↓ backend (Cranelift / wasm-encoder)
WASM/native           artefacto ejecutable
```

Note: SSA is managed internally by Cranelift during backend compilation. It is not a compiler-produced artifact or a named stage in the implemented pipeline.

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

**Decisión**: AIL targets Rust-level mature, usable reliability — memory/resource safety and zero-cost abstractions — but is not a Rust clone. The mechanism is different: ownership, resource lifecycle, and safety guarantees are encoded as first-class Semantic Graph nodes (Handle<Resource, Mode>, EffectRow, CapabilityDef), verified before lowering, and lowered through Core IR → ANF → SSA to efficient code without runtime overhead or borrow-checker syntax.

Capas:

```txt
Values            inmutables por defecto
Cells             mutación local explícita
Handles           recursos externos con lifecycle (Affine/Linear/Shared/Copy)
Shared resources  requieren capability/concurrency-safe type
Unsafe memory     solo boundary unsafe
```

El Semantic Graph hace visible lo que Rust hace mediante el borrow checker en texto: quién posee qué, qué efectos se emiten, qué capabilities se requieren. El resultado es la misma confiabilidad sin necesitar lifetime annotations en el lenguaje surface.

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
  ↓ backend (Cranelift / wasm-encoder) — SSA es interno a Cranelift
Executable target     WASM primero; native después
```

Decisión: ANF es el Compiler IR principal. SSA es un detalle interno de Cranelift; no existe como artefacto de compilador ni como etapa nombrada en el pipeline implementado.

Razón:

```txt
En ANF, el orden de efectos está en la estructura del programa.
En SSA, el orden de efectos tiende a ser metadata/análisis adicional.
```

Para un lenguaje donde effects/capabilities son centrales, ANF es el mejor punto de control para verificación, resource lifecycle, contracts y debugging semántico. SSA sigue siendo útil, pero como artefacto bajo para optimización/codegen.

## Implementation Notes

The Rust Core IR in `crates/ail-compiler/src/core_ir.rs` includes serializable `CoreExpr` and `CoreType` variants for the major design primitives: literals, variables, let/if/match/call, arithmetic/comparison, lambdas, records, tuples, variants, lists, loops, short-circuit booleans, effect calls, dispatch, tasks, channels, runtime checks, resources, and cells.

Current executable support is narrower than the full IR:

- `expr_parser.rs` parses the prefix-form executable subset (expanded in `feat/expr-parser-expand`):
  - **Literals**: integers, floats (`3.14`, `-2.5`), booleans, strings (`"text"`, with `\\`/`\"` escapes), unit.
  - **Arithmetic/comparison**: `add`, `sub`, `mul`, `div`, `mod`, `eq`, `ne`, `lt`, `le`, `gt`, `ge`, `not`.
  - **Boolean short-circuit**: `and`, `or`.
  - **Compound values**: `record`, `field`, `update`, `tuple`, `variant`, `list`.
  - **Option/Result conveniences**: `none()`, `some(x)`, `ok(x)`, `err(x)`.
  - **Control flow**: `let`, `if`, `match`, `loop`, `while`, `break`, `continue`, `return`.
  - **Effects**: `effect_call(capability, operation, args...)`.
  - **Lambdas**: `lambda(params..., body)` — all but last arg are param names.
  - **Iteration**: `foreach(binding, collection, body)` — WASM emit is implemented (inline loop, no call_indirect). `fold(init, list, func)` — parse correct; WASM emit is a stub (requires call_indirect + element section).
  - **Cells**: `cell_new(init)`, `cell_get(cell)`, `cell_set(cell, value)`.
- `wasm.rs` emits real bodies for simple values, control flow, effect calls, cells (`CellNew`/`CellGet`/`CellSet`), collection constructors (`MapNew`, `SetNew`), indexed access (`IndexGet`), and `ForEach` (inline loop over length-prefixed list); `Fold` (requires call_indirect + element section), tasks, channels, and resources still emit stubs/traps.
- Executable `Match` supports integer literal, boolean literal, wildcard, tag-only constructor (`None`), and single-binding constructor (`Ok(val)`, `Some(x)`, `Err(e)`) patterns. The WASM backend loads the i32 tag at offset 0 and, for payload-binding patterns, loads the i64 payload at offset 8. Multi-binding patterns (e.g. `Ok(a, b)`) are not yet supported and emit `Unreachable`.
- Full memory/value layout for handles, text, bytes, and nested structured payloads is still tracked as ABI validation work; records, variants, lists, `Option`, and `Result` are currently executable for scalar-slot payloads.

**Executable gaps — primitives that parse or are defined in `CoreExpr`/`CoreType` but do not yet produce real WASM emit (emit `Unreachable` or trap stubs):**

| Primitive | Status |
|-----------|--------|
| `MapNew` | **Implemented** — linear-memory layout `[count:i64, k:i64, v:i64, ...]`; validates |
| `SetNew` | **Implemented** — linear-memory layout `[count:i64, elem:i64, ...]`; validates |
| `IndexGet` | **Implemented** — dynamic address `ptr + 8 + index*8`; validates |
| `CellNew` | **Implemented** — alloc 8 bytes, store init value; returns I32 ptr; validates |
| `CellGet` | **Implemented** — I64Load at offset 0 from cell ptr; validates |
| `CellSet` | **Implemented** — I64Store at offset 0 from cell ptr; validates |
| `ForEach` | **Implemented** — inline WASM loop over `[count:i64, elem:i64, ...]` list; no call_indirect needed; validates |
| `Fold` | Parses; WASM emit is a stub (trap) — requires call_indirect + element section (function table); not yet implemented |
| `ResourceAcquire`, `ResourceRelease` | Emit `Unreachable` in WASM |
| `TaskSpawn`, `TaskAwait`, `TaskCancel`, `TaskGroup` | Emit `Unreachable` in WASM |
| `ChannelNew`, `ChannelSend`, `ChannelReceive`, `Select`, `Timeout` | Emit `Unreachable` in WASM |
| `Dispatch` (dynamic dispatch) | Emits `Unreachable` in WASM |
| `Bytes` type | No executable emit or derive path; see [ABI value contract](abi-value-contract.md) |
