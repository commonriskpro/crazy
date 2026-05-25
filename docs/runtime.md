# Runtime / capability protocol

<!-- Status: Implemented subset. Wasmtime preflight, handler dispatch, schema checks, rollback integration, replay hashes, reports, and compiled effect dispatch exist for the current milestone. Rich typed ABI/value layout remains validation work. -->

> Target design. Current implementation scope is called out in the status note and Implementation Notes. Related: [Verification](verification.md), [Compiler](compiler.md), [Package trust](packages.md), [Standard library](stdlib.md).

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

## Runtime / capability protocol: target design

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
Runtime Host
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

Nota de implementación actual: el protocolo textual mínimo `key=value` valida claves planas,
records anidados mediante rutas con punto, por ejemplo `receipt.id=rcpt-42,receipt.risk.score=7`,
`Option` mediante tag explícito `receipt.$tag=Some|None`, y `Result` mediante `payment.$tag=Ok|Err`;
solo la rama seleccionada exige sus campos de payload.

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

Implementation note: rule 4 is implemented as an opt-in preflight gate. `Handler::trust_level()` is a default method on the `Handler` trait (`crates/ail-runtime/src/handler.rs`) that returns `TrustLevel::Assumed` for backward compatibility — existing handlers that do not override it continue to work unchanged. Profiles that call `RuntimeProfile::with_min_handler_trust(level)` enforce a minimum trust level in preflight stage 6; profiles that omit `min_handler_trust` skip the gate entirely (default-disabled).

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

Implementation note: handler trust-level classification is implemented as an opt-in preflight gate. `Handler::trust_level()` (default: `Assumed`) lets each handler declare its implementation trust level. `RuntimeProfile::with_min_handler_trust(level)` activates the gate; preflight stage 6 then checks every bound handler that serves a granted capability and fails with `HandlerTrustViolation` if its declared level does not satisfy the minimum. Profiles without `min_handler_trust` skip the gate, so existing handlers and profiles are backward compatible.

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

Implementation note: an in-memory vault (`SecretVault`) and a `secret.read` capability handler (`SecretReadHandler`) are implemented in `crates/ail-runtime/src/secret.rs`. The handler resolves logical secret IDs through `RuntimeProfile::secrets_mapping` (id → vault_path), fetches from the in-memory vault, and returns bytes to the caller without logging them (audit records only BLAKE3 hashes). Grant enforcement by `RuntimeHost::call_capability` and the WASM-side dispatch (`ail/host_call` / `ail/host_call_write`) both apply before the handler is reached. Secret bytes can be injected into WASM linear memory via the `ail/host_call_write` import; end-to-end coverage of this path (WASM module → dispatch → handler → vault → memory) is in `crates/ail-runtime/tests/secret_wasm_e2e_tests.rs`. A real external vault client remains future work.

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

Implementation note: in-flight policy is stored in `RevocationRecord.in_flight_policy` (`crates/ail-runtime/src/profile.rs`) but is not currently enforced. The host (`crates/ail-runtime/src/host.rs`) performs a boolean `is_revoked` check and returns `CapabilityDenied` for new calls only. The `allow_complete`/`cancel`/`timeout_then_cancel` semantics are target design.

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

Implementation note: step 7 is implemented as an opt-in preflight gate. `ProfileAssumption` records (`id`, `status`, optional `expires_at`) are declared directly on `RuntimeProfile` via `RuntimeProfile::with_assumptions(Vec<ProfileAssumption>)`. Preflight stage 7 rejects startup with `PreflightFailure::AssumptionExpired` when any assumption has `AssumptionStatus::Expired`, `AssumptionStatus::Inactive`, or an `expires_at` timestamp in the past. Profiles with an empty assumption list (the default) skip stage 7 entirely — existing profiles and call sites are unaffected.

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
4. Every handler declares internal effects and trust.  [trust level: opt-in via Handler::trust_level(), implemented]
5. Runtime is deny-by-default.
6. Runtime validates artifact hashes before execution.
7. Runtime audits capability calls.
8. Runtime enforces limits.
9. Runtime checks are materialized and hash-covered.
10. Runtime profiles are explicit and versioned.
```

### Implementation Notes

The current implemented runtime subset resolves the original open questions for the first executable milestone as follows:

| Topic | Implementation status |
|-------|-----------------------|
| Host call ABI | Scalar calls: `ail/host_call(cap_ptr, cap_len, op_ptr, op_len, args_ptr, args_len) -> i64`. Structured calls: `ail/host_call_write(…, out_ptr, out_max) -> i32` writes handler response bytes to WASM linear memory and returns bytes written. Both are registered in `RuntimeHost::new` and dispatched to `Handler::handle`. |
| Typed boundary codec | `RuntimeInstance::invoke_typed(export, args, ValueLayout)` returns a `StructuredValue` by reading WASM linear memory and decoding with `ValueDecoder`. The milestone codec supports `Scalar`, `Record`, `Variant`, `List`, `Option`, `Result`, and `Handle` layouts; full ABI/value-layout parity remains future validation work. |
| Memory access | `RuntimeInstance::read_wasm_memory(ptr, len)` and `write_wasm_memory(ptr, bytes)` provide direct access to WASM linear memory for structured result decoding. |
| Handler structured dispatch | `Handler::handle_structured` default method encodes `StructuredValue` args as LE i64 bytes, dispatches to `handle`, and decodes the response as `StructuredValue::Scalar`. |
| Capability call limits | `ResourceLimits::max_capability_calls` is enforced after the grant check and before handler dispatch; denied ungranted capabilities still return `CapabilityDenied` first. |
| WASI exposure | Hidden behind the host runtime. The workspace owns direct `wasmtime` usage in `ail-runtime`; programs interact through host calls and exported functions. |
| Handler execution | In-process Rust `Handler` trait implementations. Verified-module handlers remain future work. |
| Handler trust enforcement | `Handler::trust_level()` default method returns `TrustLevel::Assumed` (backward compatible). `RuntimeProfile::with_min_handler_trust(level)` enables a minimum-trust preflight gate (stage 6): fails with `HandlerTrustViolation` when a bound handler's declared level does not satisfy the minimum. Gate is opt-in; profiles without `min_handler_trust` skip it entirely. |
| Startup assumption expiry (step 7) | Implemented as opt-in stage 7 in `host_preflight`. `ProfileAssumption` (`id`, `status`, `expires_at`) is declared on `RuntimeProfile` via `with_assumptions`. Preflight fails with `AssumptionExpired` for Expired/Inactive status or past `expires_at`. Empty list → gate disabled (backward compatible). |
| Secret vault | In-memory vault implemented. `SecretVault` (`crates/ail-runtime/src/secret.rs`) maps vault paths to secret bytes with a redacted `Debug` impl (values never appear in logs). `SecretReadHandler` implements the `Handler` trait for `secret.read:<secret_id>` capabilities: it resolves the logical ID through `RuntimeProfile::secrets_mapping` (id → vault_path), fetches from `SecretVault`, and returns the raw bytes to the caller. Audit events record only BLAKE3 hashes of input/output — secret values never appear in audit logs. Unmapped IDs and missing vault paths both return `HostError::CapabilityDenied` without distinguishing which step failed. Grant checks apply on both the host-side path (`RuntimeHost::call_capability`) and the WASM-side path (`ail/host_call` / `ail/host_call_write`); the full dispatch pipeline (WASM → dispatch → handler → vault → WASM memory) is covered by `crates/ail-runtime/tests/secret_wasm_e2e_tests.rs`. A real external vault client remains future work. |
| In-flight revocation policy | `InFlightPolicy` enum variants are stored but not enforced. `revoke_capability` denies new calls only (`CapabilityDenied`). `allow_complete`/`cancel`/`timeout_then_cancel` semantics are target design. |
| Tracing | OpenTelemetry dependencies exist; runtime audit/reporting is implemented. Full distributed tracing across capability calls remains future hardening. |
| Sync/async calls | Current host capability calls are synchronous from the Rust API perspective. Async-native capability typing remains part of the full design, not this milestone. |

Code references: `crates/ail-runtime/src/host.rs`, `crates/ail-runtime/src/handler.rs`, `crates/ail-runtime/src/codec.rs`, `crates/ail-runtime/src/schema.rs`, `crates/ail-runtime/tests/effect_runtime_tests.rs`, `crates/ail-runtime/tests/typed_abi_tests.rs`.
