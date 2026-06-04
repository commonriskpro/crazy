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

/// The 27 operation variants of a `ChangeSet`, grouped by canonical phase.
///
/// Phase ordering (used by `canonical::phase_order`):
///
/// | Phase | Variants |
/// |-------|----------|
/// | 0 | `Create` |
/// | 1 | `Set`, `Add`, `Remove`, `Delete`, `Disconnect`, `Rename`, `Move`, `Replace` |
/// | 2 | `Connect`, `Bind`, `Expose`, `Hide`, `Grant`, `Revoke` |
/// | 3 | `Infer`, `Derive`, `Generate` |
/// | 4 | `Assert`, `Lock`, `Refactor`, `Migrate`, `Approve`, `Reject`, `Deprecate`, `Annotate`, `Verify` |
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ChangeSetOp {
    // ── Phase 0: creation ────────────────────────────────────────────────
    /// Create a new node in the graph.
    Create,

    // ── Phase 1: property / structural mutations ──────────────────────────
    /// Set (overwrite) a property on an existing node.
    Set,
    /// Add a value to a collection property.
    Add,
    /// Remove a value from a collection property.
    Remove,
    /// Delete a node entirely from the graph (always requires impact analysis).
    Delete,
    /// Sever a typed edge between two nodes.
    Disconnect,
    /// Change a node's visible name without altering its stable identity.
    Rename,
    /// Move a node between modules/packages while preserving its identity.
    Move,
    /// Replace a definition or block in its entirety.
    Replace,

    // ── Phase 2: relationship / security / visibility ─────────────────────
    /// Establish a typed edge between two nodes.
    Connect,
    /// Associate a handler with a capability in an environment/run-profile.
    Bind,
    /// Make a node part of a public API surface.
    Expose,
    /// Remove a node from the public API surface.
    Hide,
    /// Grant a capability to a module, package, or run-profile.
    Grant,
    /// Revoke a previously granted capability.
    Revoke,

    // ── Phase 3: inference / materialization ──────────────────────────────
    /// Trigger an inference rule (e.g., infer boundary, effects, return type).
    Infer,
    /// Generate a derived implementation from a type/schema under verifiable rules.
    Derive,
    /// Request controlled generation of derived artifacts (tests, SDK, docs).
    Generate,

    // ── Phase 4: workflow / semantic / verification ───────────────────────
    /// Declare a precondition about the current graph state before applying ops.
    Assert,
    /// Lock an API, behavior, or contract to prevent accidental mutation.
    Lock,
    /// Apply a semantics-preserving transformation.
    Refactor,
    /// Declare an intentional change of API, contract, or behavior.
    Migrate,
    /// Accept an inferred boundary, assumption, or proposal.
    Approve,
    /// Reject an inferred boundary, assumption, or proposal.
    Reject,
    /// Mark a node as superseded without removing it.
    Deprecate,
    /// Attach metadata, rationale, or review notes to a node.
    Annotate,
    /// Validate a change, scope, or node against its contracts and effects.
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

// ── ConflictReason ────────────────────────────────────────────────────────

/// Typed classification of why two concurrent ChangeSets cannot be reconciled.
///
/// Set by the coordinator layer after semantic rebase analysis; `apply()` itself
/// never emits this — it returns `RebaseRequired` for stale-base situations and
/// the coordinator escalates to `ConflictIrresolvable` when rebase fails.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictReason {
    /// Two agents modified the same node in incompatible ways (e.g., rename vs. set).
    SameNodeModifiedIncompatibly,
    /// One agent deleted a node that another agent is still modifying.
    NodeDeletedWhileModified,
    /// Both agents produced changes that alter the same public API surface.
    PublicApiConflict,
    /// Both agents touched a graph invariant that cannot be composed.
    InvariantTouchedConcurrently,
    /// Both agents modified the same node's semantic content (return_type, body,
    /// or effect_row) in incompatible ways during a semantic merge.
    ///
    /// Unlike `SameNodeModifiedIncompatibly` (which is NodeRef-based at the op
    /// level), this variant is emitted by the semantic merge layer when the
    /// actual field values differ and cannot be auto-resolved.
    IncompatibleNodeModification,
}

// ── ChangeSetOutcome ──────────────────────────────────────────────────────

/// Result of attempting to apply a `CanonicalChangeSet` to a graph.
///
/// `ConflictIrresolvable` is produced by the coordinator layer (not by
/// `apply()` itself) after semantic rebase analysis determines that two
/// concurrent ChangeSets cannot be reconciled.
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
    /// The coordinator determined that the changeset conflicts irresolvably
    /// with already-applied ops; carries the typed conflict classification.
    ConflictIrresolvable {
        /// Why the conflict cannot be resolved by semantic rebase.
        reason: ConflictReason,
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
