# Standard library shape

<!-- Implementation Status: `ail-stdlib` contains modules matching the semantic-core shape; service capabilities/adapters remain package/runtime concerns. -->

> Full extracted design. Related: [Type system](type-system.md), [Runtime](runtime.md), [Packages](packages.md).

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

### Implementation Notes

The current `ail-stdlib` implementation resolves the original v1 questions this way:

| Topic | Status |
|-------|--------|
| v1 size | Semantic-core modules are implemented broadly across `option`, `result`, `numeric`, `decimal`, `text`, `bytes`, collections, time, random, crypto, IO/network/process/env, concurrency/sync, diagnostics, verify, runtime, and capability. |
| Database capability | Remains outside stdlib core as an official package/runtime capability direction. |
| Crypto defaults | Modules exist for the documented defaults; production-grade API hardening remains future work. |
| Async runtime placement | Concurrency primitives live in `concurrent`/`sync`; runtime-facing types live in `runtime`. |
| Versioning | Workspace lockstep versioning is documented in `docs/release-policy.md`; independent stdlib versioning remains a future product concern. |

Code references: `crates/ail-stdlib/src/*`, `docs/release-policy.md`.
