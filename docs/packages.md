# Package / trust model

<!-- Implementation Status: package manifest, trust, signing, resolver, registry, advisories, yanking, lockfile, handlers, and policy primitives exist. -->

> Full extracted design. Related: [Runtime](runtime.md), [Verification](verification.md), [Standard library](stdlib.md), [Storage](storage.md).

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

### Implementation Notes

The original package questions are resolved for the current milestone:

| Topic | Status |
|-------|--------|
| Registry/signing | Implemented primitives for registry, remote registry, and signing. |
| Reproducible builds | Required by design for `verified`; metadata primitives exist, ecosystem validation remains future work. |
| Federated trust | Trust metadata and remote registry primitives exist; cross-org operations remain future work. |
| Proof checking | Package verification surfaces exist; deep proof distribution policy remains future work. |
| Yanking | Yank model exists while content-addressed artifacts preserve old resolved builds. |

Code references: `crates/ail-package/src/*`.
