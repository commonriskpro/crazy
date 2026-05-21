// ── ail-change::model ─────────────────────────────────────────────────────
//
// Typed value types for the ChangeSet transaction model.
// These are pure value types — no I/O, no side effects.
//
// # Identity types
//
// `SnapshotId(u64)` identifies a graph snapshot in the storage layer.
// `BlockHash([u8; 32])` is a blake3 hash of a canonical op block.
// `Timestamp(u64)` is seconds since Unix epoch (UTC).
//
// # Invariants
//
// `ChangeSet` is always valid with an empty `ops` vec.
// All types derive `Clone + Eq + Debug + Serialize + Deserialize`.

use ail_core::semantic_graph::NodeRef;
use serde::{Deserialize, Serialize};

// ── Scalar newtypes ───────────────────────────────────────────────────────

/// Opaque snapshot identifier; used for stale-base detection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SnapshotId(pub u64);

/// blake3 hash of a single canonical op block.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockHash(pub [u8; 32]);

/// Seconds since Unix epoch (UTC).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Timestamp(pub u64);

// ── ChangeSetMeta ─────────────────────────────────────────────────────────

/// Authorship and intent metadata attached to every `ChangeSet`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeSetMeta {
    /// Identity of the change author.
    pub author: String,
    /// Human-readable description of the intended change.
    pub description: String,
    /// When the changeset was created.
    pub timestamp: Timestamp,
}

// ── ChangeSetOp ───────────────────────────────────────────────────────────

/// The seven operation phases of a `ChangeSet`, in canonical order:
/// `Create` → `Set`/`Add`/`Remove` → `Connect` → `Infer` → `Verify`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeSetOp {
    /// Create a new node in the graph.
    Create,
    /// Set (overwrite) a property on an existing node.
    Set,
    /// Add a value to a collection property.
    Add,
    /// Remove a value from a collection property or remove a node.
    Remove,
    /// Connect two nodes with a typed edge.
    Connect,
    /// Declare an inference rule to evaluate.
    Infer,
    /// Declare a verification assertion to evaluate.
    Verify,
}

// ── ChangeSet ─────────────────────────────────────────────────────────────

/// A pure value type representing an atomic batch of graph operations.
///
/// An empty `ops` vec is valid — it represents an identity changeset.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeSet {
    /// Authorship and intent.
    pub meta: ChangeSetMeta,
    /// The snapshot id against which this changeset was authored.
    pub base_snapshot_id: SnapshotId,
    /// Ordered list of operations. May be empty.
    pub ops: Vec<ChangeSetOp>,
}

// ── ChangeSetOutcome ──────────────────────────────────────────────────────

/// Result of attempting to apply a `CanonicalChangeSet` to a graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangeSetOutcome {
    /// All ops were applied successfully.
    Applied,
    /// The changeset's base snapshot is stale; carries the live snapshot id.
    RebaseRequired {
        /// The current live snapshot id at the time of the attempt.
        current_snapshot_id: SnapshotId,
    },
    /// An op failed during application; graph was rolled back.
    Failed {
        /// Human-readable explanation of the failure.
        reason: String,
    },
}

// ── Assertion types ───────────────────────────────────────────────────────

/// Precondition: asserts that the given node exists in the graph.
/// If the node is absent, apply rolls back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssertExists {
    /// The node that must be present.
    pub node_id: NodeRef,
}

/// Precondition: asserts that a node's canonical hash matches the expected value.
/// If the hash differs, apply rolls back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssertHash {
    /// The node whose hash must match.
    pub node_id: NodeRef,
    /// Expected blake3 hash of the node's canonical encoding.
    pub expected_hash: BlockHash,
}
