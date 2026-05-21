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
// 3. **Per-block hashing**: every `CanonicalOp` carries a blake3 `BlockHash`
//    computed from the op's CBOR encoding and its position index.
// 4. **Determinism**: calling `canonicalize` twice with the same input always
//    produces CBOR-identical output.

use ail_core::semantic_graph::{GraphEdge, GraphNode, NodeRef};
use serde::{Deserialize, Serialize};

use crate::model::{
    AssertExists, AssertHash, BlockHash, ChangeSet, ChangeSetOp, SnapshotId, Timestamp,
};

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
fn phase_order(op: &ChangeSetOp) -> u8 {
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalMeta {
    /// Identity of the change author.
    pub author: String,
    /// Human-readable description; always non-empty after canonicalization.
    pub description: String,
    /// When the changeset was created.
    pub timestamp: Timestamp,
}

// ── OpPayload ─────────────────────────────────────────────────────────────

/// Concrete graph mutation payload for a `CanonicalOp`.
///
/// For ops originating from a raw `ChangeSet` (which has no payload data),
/// `canonicalize` produces `Noop`. Apply tests construct `CanonicalOp`s
/// directly with real payloads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpPayload {
    /// Create a new node in the graph.
    CreateNode(GraphNode),
    /// Add a directed edge to the graph.
    AddEdge(GraphEdge),
    /// Remove an existing node by ref.
    RemoveNode(NodeRef),
    /// Rename a node (minimal Set semantics).
    SetNodeName { node_id: NodeRef, name: String },
    /// No-op placeholder; used for Infer/Verify and raw-ChangeSet-derived ops.
    Noop,
}

// ── CanonicalOp ───────────────────────────────────────────────────────────

/// A single canonicalized operation: phase classifier + payload + block hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalOp {
    /// Phase classifier (used for ordering and labelling).
    pub kind: ChangeSetOp,
    /// Concrete graph mutation payload.
    pub payload: OpPayload,
    /// blake3 hash of this op's canonical encoding.
    pub block_hash: BlockHash,
}

// ── Precondition ──────────────────────────────────────────────────────────

/// A precondition evaluated before ops are applied.
///
/// If any precondition fails, `apply` returns `Failed` and restores the
/// pre-apply graph clone (rollback).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Precondition {
    /// The referenced node must exist in the graph.
    AssertExists(AssertExists),
    /// The referenced node's canonical hash must match the expected value.
    AssertHash(AssertHash),
}

// ── CanonicalChangeSet ────────────────────────────────────────────────────

/// A fully canonicalized changeset ready for atomic application.
///
/// Constructed either via `canonicalize(ChangeSet)` or directly in tests
/// when explicit payloads are required.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalChangeSet {
    /// Canonicalized authorship and intent metadata.
    pub meta: CanonicalMeta,
    /// Snapshot identity against which this changeset was authored.
    pub base_snapshot_id: SnapshotId,
    /// Preconditions evaluated before any op is applied.
    pub preconditions: Vec<Precondition>,
    /// Phase-ordered, hash-stamped operations.
    pub ops: Vec<CanonicalOp>,
}

// ── canonicalize ──────────────────────────────────────────────────────────

/// Transform a raw `ChangeSet` into its canonical form.
///
/// Steps:
/// 1. Materialize `description`: replace `""` with `"<no description>"`.
/// 2. Stable-sort ops by phase ordinal (see `phase_order`).
/// 3. Compute a blake3 `BlockHash` per op from its CBOR encoding + index.
/// 4. Wrap each op with `OpPayload::Noop` (raw ops carry no payload data).
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
        preconditions: vec![],
        ops: canonical_ops,
    }
}

// ── helpers ───────────────────────────────────────────────────────────────

/// Compute blake3 hash of `(op CBOR encoding | phase ordinal | index)`.
///
/// The index ensures two identical ops at different positions produce
/// distinct hashes, providing per-block uniqueness.
fn compute_block_hash(op: &ChangeSetOp, idx: usize) -> BlockHash {
    let mut op_bytes: Vec<u8> = Vec::new();
    ciborium::into_writer(op, &mut op_bytes).expect("ChangeSetOp serialization must not fail");

    let mut hasher = blake3::Hasher::new();
    hasher.update(&op_bytes);
    hasher.update(&phase_order(op).to_le_bytes());
    hasher.update(&(idx as u64).to_le_bytes());

    BlockHash(*hasher.finalize().as_bytes())
}
