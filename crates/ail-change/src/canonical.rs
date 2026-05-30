// ── ail-change::canonical ─────────────────────────────────────────────────
//
// Deterministic canonicalization of a `ChangeSet`.
//
// # Guarantees
//
// 1. **Stable phase order**: Create → Set/Add/Remove → Connect → Infer → Verify.
//    The stable sort preserves relative order among ops of the same phase.
// 2. **Default materialization**: an empty `description` is replaced with
//    `"<no description>"` so downstream consumers never handle empty strings.
//    `create_function` and `create_type` ops without `visibility` get
//    `visibility=private` materialized.
// 3. **Per-block hashing**: every `CanonicalOp` carries a blake3 `BlockHash`
//    computed from the op's CBOR encoding and its position index.
// 4. **Determinism**: calling `canonicalize` / `canonicalize_parsed` twice
//    with the same input always produces CBOR-identical output.
// 5. **Precondition carry-through**: `canonicalize_parsed` carries preconditions
//    from `ParsedChangeSet` into the resulting `CanonicalChangeSet`.
// 6. **ACL version**: `CanonicalChangeSet` records the `acl_version` from the
//    source document (defaults to `"1.0"`).

use std::collections::BTreeMap;

use ail_core::semantic_graph::{
    Assertion, Binding, EdgeKind, GeneratedArtifact, GraphEdge, GraphNode, InferredFact, NodeRef,
    Visibility, WorkflowState,
};
use serde::{Deserialize, Serialize};

use crate::acl_migrator::{CURRENT_ACL_VERSION, MigrateError, run_migration_chain};
use crate::model::{
    AssertExists, AssertHash, BlockHash, ChangeSet, ChangeSetOp, SnapshotId, Timestamp,
};
use crate::parser::{
    ApprovalRequirements, ChangeComposition, ExpectClaims, OpArgs, ParsedBlock, ParsedChangeSet,
};
mod defaults;
mod expand;
mod hash;
mod model;
mod normalize;
mod parsed;

#[path = "canonical_ops.rs"]
mod canonical_ops;

pub use model::{CanonicalChangeSet, CanonicalMeta, CanonicalOp, OpPayload, Precondition};
pub use normalize::normalize_id;
pub use parsed::{canonicalize_parsed, try_canonicalize_parsed};

use canonical_ops::materialize_payload;
use defaults::materialize_defaults;
use expand::expand_infer_boundary;
use hash::compute_block_hash;
use normalize::normalize_op_args;

// ── Phase ordering ────────────────────────────────────────────────────────

/// Canonical phase ordinal for stable sorting.
///
/// | Phase | Ops |
/// |-------|-----|
/// |     0 | Create |
/// |     1 | Set, Add, Remove, Delete, Disconnect, Rename, Move, Replace |
/// |     2 | Connect, Bind, Expose, Hide, Grant, Revoke |
/// |     3 | Infer, Derive, Generate |
/// |     4 | Assert, Lock, Refactor, Migrate, Approve, Reject, Deprecate, Annotate, Verify |
pub(super) fn phase_order(op: &ChangeSetOp) -> u8 {
    match op {
        ChangeSetOp::Create => 0,
        ChangeSetOp::Set
        | ChangeSetOp::Add
        | ChangeSetOp::Remove
        | ChangeSetOp::Delete
        | ChangeSetOp::Disconnect
        | ChangeSetOp::Rename
        | ChangeSetOp::Move
        | ChangeSetOp::Replace => 1,
        ChangeSetOp::Connect
        | ChangeSetOp::Bind
        | ChangeSetOp::Expose
        | ChangeSetOp::Hide
        | ChangeSetOp::Grant
        | ChangeSetOp::Revoke => 2,
        ChangeSetOp::Infer | ChangeSetOp::Derive | ChangeSetOp::Generate => 3,
        ChangeSetOp::Assert
        | ChangeSetOp::Lock
        | ChangeSetOp::Refactor
        | ChangeSetOp::Migrate
        | ChangeSetOp::Approve
        | ChangeSetOp::Reject
        | ChangeSetOp::Deprecate
        | ChangeSetOp::Annotate
        | ChangeSetOp::Verify => 4,
    }
}

// ── CanonicalMeta ─────────────────────────────────────────────────────────

/// Canonicalized metadata with all optional fields materialized.
///
/// `description` is guaranteed non-empty: an empty source value becomes
/// `"<no description>"`.

// ── canonicalize ──────────────────────────────────────────────────────────

/// Transform a raw `ChangeSet` into its canonical form.
///
/// Steps:
/// 1. Materialize `description`: replace `""` with `"<no description>"`.
/// 2. Stable-sort ops by phase ordinal (see `phase_order`).
/// 3. Compute a blake3 `BlockHash` per op from its CBOR encoding + index.
/// 4. Wrap each op with `OpPayload::Noop` (raw ops carry no payload data).
///
/// This is the legacy path — no kv args, no defaults, no preconditions.
/// Prefer `canonicalize_parsed` when a `ParsedChangeSet` is available.
pub fn canonicalize(cs: ChangeSet) -> CanonicalChangeSet {
    // Step 1: materialize description default.
    let description = if cs.meta.description.is_empty() {
        "<no description>".to_string()
    } else {
        cs.meta.description
    };

    // Step 2: stable-sort ops by canonical phase order.
    let mut sorted_ops = cs.ops;
    sorted_ops.sort_by_key(phase_order);

    // Step 3+4: compute per-block hash and wrap.
    let canonical_ops: Vec<CanonicalOp> = sorted_ops
        .into_iter()
        .enumerate()
        .map(|(idx, op)| {
            let block_hash = compute_block_hash(&op, idx);
            CanonicalOp {
                kind: op,
                verb: String::new(),
                args: BTreeMap::new(),
                payload: OpPayload::Noop,
                block_hash,
            }
        })
        .collect();

    CanonicalChangeSet {
        meta: CanonicalMeta {
            author: cs.meta.author,
            description,
            timestamp: cs.meta.timestamp,
        },
        base_snapshot_id: cs.base_snapshot_id,
        acl_version: "1.0".to_string(),
        op_schema_version: None,
        graph_schema_version: None,
        core_ir_schema_version: None,
        diagnostics_schema_version: None,
        verification_schema_version: None,
        preconditions: vec![],
        ops: canonical_ops,
        expect: None,
        approval: None,
        composition: ChangeComposition::default(),
        blocks: vec![],
        verify: vec![],
    }
}

#[cfg(test)]
mod tests;
