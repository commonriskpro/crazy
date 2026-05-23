# Coordinator / multi-agent serialization

<!-- Status: Implemented subset. `ail-coordinator` contains Coordinator, rebase, conflict classification, semantic merge, and remote submission verification for the current milestone. Durable multi-hop collaboration remains target design. -->

> Target design. Current implementation scope is called out in the status note and Implementation Notes. Related: [AI Change Language](change-language.md), [Storage](storage.md), [Runtime](runtime.md).

## Propósito

Cuando múltiples agentes LLM trabajan en paralelo sobre el mismo programa, cada uno genera un `CanonicalChangeSet` basado en un snapshot. Si dos changesets llegan al mismo tiempo, solo uno puede aplicarse directamente; el otro está desactualizado.

El `Coordinator` es el punto de serialización autoritative: garantiza que los changesets concurrentes se apliquen uno a la vez y que los changesets desactualizados reciban un intento de rebase semántico antes de ser rechazados.

```txt
Agente A ──┐
            ├──▶  Coordinator.submit()  ──▶  snapshot nuevo
Agente B ──┘         (Mutex)
```

## Arquitectura

El `Coordinator` envuelve todo su estado mutable en un `Arc<Mutex<CoordinatorState>>`, lo que le permite ser `Clone + Send + Sync` y compartirse libremente entre tareas `tokio`.

```txt
Coordinator
  └── Arc<Mutex<CoordinatorState>>
        ├── graph: SemanticGraph          (el grafo vivo)
        ├── bridge: MemorySnapshotBridge  (snapshot id vivo)
        ├── committed_diff: StructuralDiff  (último changeset aplicado)
        └── committed_removes: BTreeSet<NodeRef>
```

Cada llamada a `submit()` adquiere el mutex exclusivo. No hay concurrencia dentro del ciclo apply/rebase.

## Protocolo de submit

```txt
1. Adquirir mutex
2. Comparar cs.base_snapshot_id con el snapshot vivo
   a. Coincide  →  clean apply
   b. No coincide  →  semantic rebase
3. Clean apply:
   - Construir StructuralDiff desde cs.ops
   - Aplicar con FixedSnapshotBridge
   - Avanzar snapshot id
   - Actualizar committed_diff / committed_removes
4. Semantic rebase:
   - Llamar rebase(pending, &committed_diff, live_id)
   - Rebased(cs)  →  aplicar rebased; devolver RebaseApplied
   - Conflict(_)  →  classify_conflict; devolver ConflictIrresolvable
```

### FixedSnapshotBridge

Un `SnapshotBridge` que siempre devuelve el mismo `SnapshotId` capturado antes de la llamada. Se usa para romper el conflicto de borrow entre `&mut state.graph` y `&state.bridge` que existirían si ambos se tomaran del mismo struct.

## StructuralDiff

```txt
StructuralDiff {
    touched_nodes: BTreeSet<NodeRef>
}
```

Contiene los `NodeRef`s tocados por las operaciones del último changeset aplicado. Se construye con `StructuralDiff::from_ops(&ops)`.

Payloads reconocidos: `CreateNode`, `RemoveNode`, `SetNodeName`, `AddEdge`. Los payloads by-name y `Noop` no generan NodeRefs porque se resuelven contra el grafo vivo durante el apply.

El coordinator guarda solo el diff del último changeset aplicado (Phase 13 conservative rule). Rebase multi-hop (base más de un snapshot atrás) usa el mismo diff.

## rebase()

Función pura: `rebase(pending, &diff, live_id) → RebaseResult`.

```txt
1. Extraer NodeRefs de pending.ops
2. Intersectar con diff.touched_nodes
3. Intersección vacía  →  Rebased (base_snapshot_id actualizado a live_id)
4. Intersección no vacía  →  Conflict(SameNodeModifiedIncompatibly)
         ↓
   El coordinator llama classify_conflict() con el contexto completo
```

La regla conservadora: cualquier op pendiente que referencia un NodeRef ya tocado por el diff committed es un conflicto. Permitir ops aditivas sobre el mismo nodo es trabajo futuro.

## classify_conflict()

```txt
classify_conflict(conflicts: &BTreeSet<NodeRef>, removed: &BTreeSet<NodeRef>) → ConflictReason
```

| Condición | Resultado |
|-----------|-----------|
| Algún `NodeRef` conflictivo fue eliminado por el diff committed | `NodeDeletedWhileModified` |
| Solapamiento sin removes | `SameNodeModifiedIncompatibly` |

El coordinator llama esta función después de que `rebase()` devuelve `Conflict`, usando `state.committed_removes` para distinguir removes de modifies.

## CoordinatorOutcome

| Variante | Descripción |
|----------|-------------|
| `Applied { applied_snapshot_id }` | Changeset aplicado directamente. Snapshot avanzó. |
| `RebaseApplied { rebased_onto, applied_snapshot_id }` | Changeset rebased y aplicado. Snapshot avanzó. |
| `ConflictIrresolvable { reason }` | Conflicto semántico irresolvable. Snapshot no avanzó. |
| `StaleBase { current_snapshot_id }` | Base desactualizada sin intento de rebase (fallback de seguridad; Phase 13 siempre intenta rebase). |
| `Failed { reason }` | Error interno durante el apply. |

## verify_remote_submission()

```rust
pub async fn verify_remote_submission(
    &self,
    rcs: RemoteChangeSet,
) -> Result<CoordinatorOutcome, RemoteError>
```

1. Verifica la firma Ed25519 del `RemoteChangeSet`.
2. Si la firma es inválida → devuelve `Err(RemoteError::SignatureInvalid)`.
3. Si la firma es válida → llama `submit(rcs.changeset)`.
4. Si `submit` devuelve `Failed` → devuelve `Err(RemoteError::CoordinatorFailed(reason))`.
5. Cualquier otro outcome → devuelve `Ok(outcome)`.

El snapshot vivo no avanza si la firma falla.

## semantic_merge()

Además del ciclo submit/rebase, el módulo `rebase` expone `semantic_merge(left, right) → MergeResult` para fusionar dos grafos.

```txt
Reglas:
- Nodos de right no presentes en left → adición directa
- Nodos idénticos en ambos → deduplicación silenciosa
- Nodos con campos semánticos distintos (return_type / body_expr / effect_row) → Conflict
- Edges de right no presentes en left → adición directa
```

## Diseño conservador (Phase 13)

| Decisión | Razón |
|----------|-------|
| Solo se guarda el último `StructuralDiff` | Simplicidad; suficiente para detectar conflictos inmediatos |
| Rebase multi-hop usa el mismo diff | El diff del último apply es la ventana de conflicto |
| Audit log en memoria | Sin persistencia por ahora |
| Siempre se intenta rebase | `StaleBase` es un fallback de seguridad, nunca el path normal |

### Implementation Notes

`Coordinator` vive en `crates/ail-coordinator/src/coordinator.rs`. Las funciones puras `rebase()` y `classify_conflict()` viven en `crates/ail-coordinator/src/rebase.rs`. `ConflictReason` es re-exportado desde `ail-change` vía `crates/ail-coordinator/src/conflict.rs`.

Code references: `crates/ail-coordinator/src/coordinator.rs`, `crates/ail-coordinator/src/rebase.rs`.
