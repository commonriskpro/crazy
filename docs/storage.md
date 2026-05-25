# Storage / versioning model

<!-- Status: Implemented subset. CAS, GraphStore, Postgres, tempfile filesystem store, retention, migrations, branch/tag/approval/export/integrity primitives exist. Conceptual directory layout is target design, not the filesystem layout. -->

> Target design. Current implementation scope is called out in status notes and Implementation Notes. Related: [AI Change Language](change-language.md), [Context Server](context-server.md), [Verification](verification.md).

## Storage / versioning model: target design

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

<!-- Status: Target design. Current filesystem CAS stores flat `<blake3-hex>` files; Postgres stores `cas_objects` plus `snapshots_index`. -->

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

Implementation note: `ail-storage` currently ships a built-in v0 -> v3 catalog
of structural no-op migrations. `MigrationCatalog::dry_run` reports the current
version, target version, pending steps, and any catalog gap without applying
migration bodies or writing version records.

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

Implementation note: `ail-storage` exposes two executable integrity paths today.
`verify_integrity` validates snapshot dependencies (`graph_root_hash`, parent
links, change/report/artifact links, approvals, assumptions, and indexes).
`verify_object_store_integrity` enumerates CAS object ids from stores that
implement read-only `EnumerableObjectStore`, reloads each object, and reports
listed-missing objects or BLAKE3 hash mismatches without mutating storage.
`MutableObjectStore` extends that enumerable contract with deletion for GC. The
in-memory and tempfile stores support executable object GC through `run_gc`.
`gc_unreferenced` now accepts a `SnapshotHolds` parameter that enforces
branch-head locks, tag locks, and audit/legal holds: snapshots pointed to by
active branches or tags survive GC even when `RetentionPolicy` alone would
not protect them. Use `collect_branch_holds` / `collect_tag_holds` to derive
holds from `BranchRegistry` / `TagRegistry` before each GC run.

**Ancestry protection**: `collect_branch_holds` protects only the HEAD
snapshot of each branch. Intermediate ancestors are NOT automatically held;
they survive only if the retention policy independently protects them (e.g.
`max_age_days`). Use `collect_branch_holds_with_ancestry` to hold the full
parent chain of every live branch.

**Compaction interaction**: `compact_snapshots` replaces original snapshot IDs
with a new covering-snapshot ID. Holds built before compaction become stale
and no longer protect the covering snapshot. Always refresh holds (by calling
`collect_branch_holds` / `collect_tag_holds` after updating branch/tag
pointers) before the next `gc_unreferenced` run.

**Snapshot envelope CAS reachability** (critical when `ObjectStore` is shared):
`ObjectBackedGraphStore::save_snapshot` encodes each `SnapshotEnvelope` as
CBOR and stores the bytes as a `RawObject` in the backing `ObjectStore`.  The
content-addressed id of those bytes (`cas_id`) is recorded in the internal
`snapshot_index` and is **distinct** from `envelope.graph_root_hash`.

If `run_gc` is called on the same `ObjectStore` with a reachable set that
contains only `graph_root_hash` values, the stored envelope bytes are treated
as unreachable and deleted — corrupting the store with no error at GC time
(`list_snapshots` subsequently returns `StorageError::NotFound`).

Use `collect_reachable_object_ids_for_snapshots` to build the correct reachable
set before calling `run_gc`.  This helper encodes each retained envelope with
`CborCodec` (the same codec as `save_snapshot`) to recompute the CAS id, and
returns a `BTreeSet` containing both the envelope CAS ids and the
`graph_root_hash` values:

```rust
// Phase 1: remove unreachable snapshot index entries.
gc_unreferenced(&graph_store, &policy, &holds, now_ms).await?;
// Phase 2: enumerate retained snapshots.
let retained = graph_store.list_snapshots().await?;
// Phase 3: build CAS reachable set (envelope bytes + graph roots).
let reachable = collect_reachable_object_ids_for_snapshots(&retained)?;
// Phase 4: delete unreachable raw CAS objects.
run_gc(&object_store, &reachable).await?;
```

Always call `list_snapshots` immediately before `collect_reachable_object_ids_for_snapshots`
to ensure the envelope structs match what is currently indexed.

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

### Storage backend decisions

| Area | Decision |
|------|----------|
| Storage API | Async-native `GraphStore`. Compiler consumes immutable snapshots and an in-memory/mmap compilation database — not repeated live queries. |
| Storage model | FoundationDB-compatible: ordered keys, immutable snapshots, ChangeSet log, CAS blobs, transactionally updated indexes. |
| Initial backend | Postgres (metadata + indexes) + CAS object store / filesystem (blobs). FDB is the aspirational production backend; a spike is required to confirm operational cost justifies it over Postgres. SQLite/libSQL is an optional simple/local backend — not the primary architecture. |
| Hash algorithm | BLAKE3 |
| Canonical serialization | Deterministic CBOR for storage objects and runtime payloads. |
| Distributed collaboration | Agents submit ChangeSets against base snapshots; a coordinator serializes authoritative commits; stale changes rebase and reverify. Detailed protocol is a validation spike — see [Risks](risks.md) V-06. |
| Default local retention | Configurable per project; defaults defined in retention policy examples above. |
| Protected audit archive | External archival via export bundles; local pruning allowed by retention policy once audit obligations are satisfied. |

### Implementation Notes

Current storage keeps path/database layout deliberately simpler than the conceptual graph-store tree above:

- `TempfileObjectStore` writes each object as a single file named by lower-hex `ObjectId`.
- `PostgresObjectStore` writes raw CAS bytes to `cas_objects(id BYTEA PRIMARY KEY, data BYTEA NOT NULL)`.
- `PostgresGraphStore` stores snapshots as CBOR CAS objects and records `envelope_id -> cas_id` in `snapshots_index` for listing.
- Semantic meaning stays in CBOR objects and graph envelopes, not directory names.

Code references: `crates/ail-storage/src/object.rs`, `crates/ail-storage/src/backends/tempfile.rs`, `crates/ail-storage/src/backends/postgres.rs`.
