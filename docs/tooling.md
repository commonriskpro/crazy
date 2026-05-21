# Tooling / developer workflow

> Full extracted design. Related: [AI Change Language](change-language.md), [Context Server](context-server.md), [Verification](verification.md), [Runtime](runtime.md).

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
