# Type system

<!-- Implementation Status: design remains broader than current checker implementation; type/effect/contract/resource/concurrency checkers exist as milestone subsets. -->

> Full extracted design. Related: [Core IR](core-ir.md), [Verification](verification.md), [Standard library](stdlib.md).

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
