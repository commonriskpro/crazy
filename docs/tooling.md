# Tooling / developer workflow

<!-- Status: Target design with implemented subset. Workspace and CLI entry points exist; the full command surface below is design target unless called out in Implementation Notes. -->

> Target design. Current implementation scope is called out in the status note and Implementation Notes. Related: [AI Change Language](change-language.md), [Context Server](context-server.md), [Verification](verification.md), [Runtime](runtime.md).

## Tooling / developer workflow: target design

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
ail eval
ail inspect
ail diff
ail rollback
ail rebase
ail merge
ail refactor
ail approve
ail reject
ail package
ail remote
ail policy
ail doctor
ail gc
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
workflow_state
```

Machine mode includes `workflow_state` so tools can choose the next step without
guessing from prose:

```txt
applyable
approval_required
rebase_required
missing_changeset
next_action
repair_options
```

For stale-base changesets, `verify --json` reports `workflow_state.rebase_required = true`, `applyable = false`, `next_action = "rebase"`, and a `rebase_required` repair option instead of treating the change as applyable.

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
workflow_state
target snapshot
```

Rules:

```txt
1. apply requires accepted verification report for selected profile.
2. apply creates new snapshot.
3. apply is atomic.
4. apply refuses stale base unless rebase is requested.
```

In `apply --json`, stale-base failures still exit non-zero and keep the human error on stderr, but stdout includes a machine-readable envelope:

```txt
status = "error"
data.error = "rebase_required"
data.workflow_state.rebase_required = true
data.workflow_state.applyable = false
data.workflow_state.next_action = "rebase"
data.workflow_state.repair_options[] includes code = "rebase_required"
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
ail policy list
ail policy add "deny capability file.write:*"
ail policy check change.add_checkout --profile prod
ail policy explain no_unverified_public_api
ail policy set max_new_capabilities=2
```

Policy changes are themselves ChangeSets or admin records, depending project mode.

#### policy list

Lists all active policy rules persisted for the project.

```txt
ail policy list
```

Outputs:

```txt
active policies: <N>
<rule 1>
<rule 2>
...
```

#### policy add

Appends a policy rule to the project's persisted rule list.

```txt
ail policy add "deny capability file.write:*"
```

Rules are stored as plain text entries. The `check` subcommand parses capability deny/allow rules from this list when enforcing policy against a ChangeSet.

### Package workflow

```txt
ail package init [--name <name>] [--version <version>]
ail package add payments.stripe@1.2
ail package install payments.stripe@1.2
ail package search <query>
ail package verify
ail package publish
ail package audit
ail package advisory add <package> <constraint> --id <id> --severity <low|medium|high|critical> --reason <text>
ail package advisory list
ail package yank <package> <version> --reason <text>
ail package yanked
ail package explain payments.stripe
```

Package install does not grant capabilities.
Package add, install, publish, and explain report verification evidence from the
manifest as-is: when no `verification_report` is present, human output shows
`verification_report: none` and JSON includes `verification_report: null` with
`verification_report_status: "none"`.
When a manifest has a report, add/install pin its deterministic CBOR+BLAKE3 hash
in `.ail/packages/lock.cbor` as `verification_report_hash`. `ail package verify`
compares that pinned hash with the current local registry manifest report hash
and emits stable JSON under `verification_report_integrity` and
`verification_report_mismatches`. Missing legacy lock hashes are mismatches, not
silent full-integrity success. This is local pinning only, not remote proof
validation.

#### package audit

Audits the project's installed package lockfile against local package registry
metadata only. It reads `.ail/packages/lock.cbor` and local registry metadata in
`.ail/packages/registry.cbor`; it does not fetch a remote advisory database.

Clean audit output is explicit:

```txt
audit: clean
packages_checked: <N>
issues: 0
blocked: 0
warnings: 0
```

Machine output includes `status`, `issues`, and summary counts:

```json
{
  "status": "ok",
  "data": {
    "status": "blocked|warning|clean",
    "packages_checked": 1,
    "issues": [
      {
        "package": "payments.stripe",
        "version": "1.0.0",
        "kind": "advisory",
        "status": "blocked",
        "advisory_id": "adv_123",
        "advisory_title": "idempotency handler bug",
        "title": "idempotency handler bug",
        "severity": "critical",
        "affected_range": "<1.2.3",
        "reason": "idempotency handler bug"
      }
    ],
    "summary": {
      "packages_checked": 1,
      "issues": 1,
      "advisories": 1,
      "yanked": 0,
      "blocked": 1,
      "warnings": 0
    }
  }
}
```

Exit semantics are conservative: yanked packages and high/critical advisories
are `blocked` and return a non-zero exit after printing the human/JSON audit
payload. Low/medium advisories are `warning` and return zero.

#### package advisory / yank metadata

Local advisory and yank metadata is managed directly in `.ail/packages/registry.cbor`:

```txt
ail package advisory add payments.stripe "<1.2.3" --id adv_123 --severity critical --reason "idempotency handler bug"
ail package advisory list
ail package yank payments.stripe 1.2.0 --reason "bad local release"
ail package yanked
```

These commands are local-only admin records. They preserve existing signed package
records, local advisories, and yank records in the registry CBOR file. They do not
ingest a remote advisory database and do not mutate any remote registry.

Machine output uses lowercase `severity`, `status`, `kind`, and `trust` values;
enum debug names are not part of the JSON contract.

#### package init

Creates a `PackageManifest` for the current graph and persists it to the project store.

```txt
ail package init --name my.package --version 0.1.0
```

Outputs:

```txt
package initialized
name: <name>
version: <version>
manifest_hash: <blake3-hex>
```

#### package install

Installs a specific package from the local registry into the project lockfile.

```txt
ail package install payments.stripe@1.2
```

Shows trust level and package hash. Does not grant any capabilities; capabilities must be declared explicitly in the program's semantic graph.

```txt
installed: <name>@<version>
trust: <level>
package_hash: <blake3-hex>
verification_report: <attached|none>
note: package install does not grant capabilities
```

If the lockfile already contains the same package at a different version,
`package install` treats the operation as a local upgrade. Patch/minor upgrades
are accepted. Major upgrades, or target versions locally declared with
`compatibility: major`, require local migration metadata in
`.ail/packages/registry.cbor`; without it, install is blocked. With migration
metadata whose package, normalized `from_version`, and normalized `to_version`
match the actual upgrade path, install succeeds and reports a warning plus the
local migration hash.

Machine output includes stable local compatibility issue objects:

```json
{
  "package": "payments.stripe",
  "current_version": "1.0.0",
  "target_version": "2.0.0",
  "kind": "migration",
  "status": "blocked",
  "reason": "breaking upgrade requires local migration metadata",
  "migration_id": null,
  "migration_hash": null
}
```

This is local metadata enforcement only. The CLI does not contact a remote
migration service and does not execute package migrations.

#### package verify

Verifies package lockfile integrity against the local registry. In addition to
hash, signature, and verification report hash checks, machine output includes
`compatibility_integrity` and `compatibility_issues` for installed entries whose
local compatibility/migration metadata is invalid or migration-bearing. Blocked
compatibility issues return a non-zero exit after printing the JSON payload;
warnings return zero.

Machine output also includes `reproducible_evidence_integrity` (`"ok"` or
`"warning"`) and a `verified_packages_missing_evidence` list. When a
`TrustLevel::Verified` package in the local registry lacks
`reproducible_evidence`, `reproducible_evidence_integrity` is set to `"warning"`
and the package identifier is added to `verified_packages_missing_evidence`.

**Important**: `package verify` treats missing reproducible evidence as an
advisory warning, not a blocker — the command exits 0 and the JSON `verified`
field remains `true`. However, **runtime preflight hard-fails** on `Verified`
packages that lack evidence. Human output makes this asymmetry visible with a
prominent `WARNING` line naming the affected packages and a summary header of
`packages: all verified (reproducible evidence warning)`. Operators who see this
warning should add `reproducible_evidence` to their package manifests before
deploying to a runtime environment.

#### package search

Searches the local package registry for packages matching a query string.

```txt
ail package search stripe
```

Returns up to 20 results with name, latest version, and description.

```txt
packages found: <N>
<name>@<version>
...
```

CLI must show:

```txt
trust level
verification report
requested capabilities
assumptions
unsafe surface
advisories
```

### Eval workflow

```txt
ail eval "add(20, 22)"
ail eval "mul(6, 7)"
ail eval 42
```

Evaluates an inline arithmetic expression without initializing a project or requiring a graph snapshot. Supported forms:

```txt
<integer literal>          e.g. 42
add(<a>, <b>)
sub(<a>, <b>)
mul(<a>, <b>)
div(<a>, <b>)
mod(<a>, <b>)
double(<x>)
```

The expression is compiled directly to ANF → WASM via the `dev` profile, instantiated in the runtime host, and the result is printed.

Outputs:

```txt
expression: add(20, 22)
result: 42
```

Rules:

```txt
eval does not require ail init.
eval does not read or write graph snapshots.
eval uses the dev profile with no capability requirements.
```

### GC workflow

```txt
ail gc
```

Deletes objects in `.ail/store/objects/` that are no longer reachable from any branch tip. Requires an initialized file store (`.ail/` directory); returns an error for memory and Postgres backends.

Outputs:

```txt
objects before: <N>
objects after: <M>
bytes freed: <B>
```

Rules:

```txt
gc only removes objects unreachable from branch tips.
gc does not delete snapshots, change logs, or branch refs.
gc requires an initialized file project (ail init).
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

### Implementation Notes

The current implemented tooling subset resolves the original tooling questions as follows:

| Topic | Status |
|-------|--------|
| Final CLI name | `ail`. |
| Interactive shell | Not required for the first full product release. No shell is implemented. |
| Editor edits | Design direction is editor-generated ChangeSets; full editor integration is not implemented. |
| Approval UX | Approval records and requirements exist in storage/change models; final human UX remains future work. |
| Local experiments | Test/local object stores exist. Disabling graph storage entirely is not part of the implemented model. |
| `ail eval` | Implemented. Compiles and runs inline integer/arithmetic expressions without project initialization. |
| `ail gc` | Implemented for the file store. Collects unreachable objects under `.ail/store/objects/`. Not supported for memory or Postgres backends. |
| `policy list` / `policy add` | Implemented. Rules are persisted as text entries; `policy check` parses capability deny/allow rules from the list. |
| `package init` / `package install` / `package search` | Implemented against the local in-process registry. `install` adds to the lockfile; `search` queries by name prefix. |
| `remote submit` | Implemented as `ail remote submit <change-id> --signer <key-ref> [--json]` against the local in-process `Coordinator::handle_remote_exchange` boundary. It loads and validates `.ail/remote.json` when present, and missing config defaults to deny-all in the loader, but submit still uses ephemeral in-process signer identity until durable key loading exists. It does not claim network transport. |
| `remote push` / `remote pull` | Implemented as `ail remote push --root <object-id> [--json]` and `ail remote pull <root> [--json]` for initialized file-backed projects. Raw roots are bundled alone and report `bundle_scope=single_root_object`. Roots that decode as `SnapshotEnvelope` include required direct CAS dependencies declared by the envelope (`graph_root_hash`, `applied_change_id`, `audit_record_ids`, and `migration_metadata_ids` as applicable) and report `bundle_scope=root_with_snapshot_envelope_dependencies`. Missing real direct CAS dependencies fail bundle construction instead of producing partial bundles. `parent_id` remains snapshot identity metadata, not a direct CAS object dependency. Bundles are persisted under `.ail/remote/bundles/<root>.cbor` and checked through the in-process bundle exchange boundary. Transport scope is `local_file_bundle_store+in_process`; it does not claim network transport, federation, remote discovery, remote config loading for push/pull, raw graph traversal, or general transitive traversal. |

Code references: `crates/ail-cli/src/main.rs`, `crates/ail-change/src/parser.rs`, `crates/ail-storage/src/approval.rs`.
