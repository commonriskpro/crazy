use ail_core::semantic_graph::NodeRef;
use ail_storage::object::ObjectId;
use serde::{Deserialize, Serialize};

use super::QueryBudget;

// ── SnapshotSelector ──────────────────────────────────────────────────────

/// Identifies which `SnapshotEnvelope` to materialise.
///
/// `ById` is always supported.  `Latest` is supported by both
/// `InMemoryContextSource` (returns the snapshot with the highest
/// `created_at`) and `StoreContextSource` (lists all snapshots and returns
/// the most recent).
///
/// # Doc spec
///
/// ```txt
/// context fn.checkout at latest
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SnapshotSelector {
    /// Look up a specific snapshot by its `SnapshotEnvelope.id`.
    ById(ObjectId),
    /// Resolve to the most-recently created snapshot (highest `created_at`).
    ///
    /// Ties are broken deterministically by `ObjectId` byte order (highest wins).
    Latest,
}

// ── QueryScope ────────────────────────────────────────────────────────────

/// Traversal scope for a context query.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueryScope {
    /// For `Node` queries: target node only.
    /// For `Graph` queries: equivalent to `Full`.
    Local,
    /// For `Node` queries: target plus all reachable nodes (BFS).
    /// For `Graph` queries: all nodes ordered by `NodeRef`.
    Full,
}
// ── ContextQuery ──────────────────────────────────────────────────────────

/// Input contract for a context query.
///
/// `budget` is a `QueryBudget` that controls traversal depth, node count,
/// byte limit, visibility, transitive inclusion, runtime logs, and profile.
/// A zero `budget.max_tokens` is invalid and will be rejected with
/// `ContextError::InvalidBudget`.
///
/// # Query kinds
///
/// | Variant          | Doc query kind    | Description                                       |
/// |------------------|-------------------|---------------------------------------------------|
/// | `Node`           | `context`         | General slice for a single node                   |
/// | `Graph`          | —                 | Whole-graph dump (bounded by budget)              |
/// | `Impact`         | `impact`          | What breaks if `target` changes                   |
/// | `Callers`        | `callers`         | Who calls `target` (optionally transitive)        |
/// | `Callees`        | `callees`         | What `target` calls (optionally transitive)       |
/// | `Effects`        | `effects`         | Effect/capability declarations on `target`        |
/// | `Contracts`      | `contracts`       | Requires/ensures clauses on `target`              |
/// | `History`        | `history`         | ChangeSet provenance chain for `target`           |
/// | `Proofs`         | `proofs`          | Proof obligations and status for `target`         |
/// | `Resources`      | `resources`       | Resource handles, ownership, concurrency info     |
/// | `Boundaries`     | `boundaries`      | Architectural boundaries and trust levels         |
/// | `Why`            | `why`             | Provenance trace explaining a claim or edge       |
/// | `RefactorContext`| `refactor_context`| Safe-refactor prerequisites for `target`         |
/// | `Runtime`        | `runtime`         | Runtime profile grants and limits for `target`   |
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextQuery {
    /// Context centered on a single node.
    Node {
        /// The node to centre the query on.
        target: NodeRef,
        /// Traversal scope from the target.
        scope: QueryScope,
        /// Budget and scoping dimensions (must have `max_tokens > 0`).
        budget: QueryBudget,
    },
    /// Context spanning the whole graph.
    Graph {
        /// Traversal scope.
        scope: QueryScope,
        /// Budget and scoping dimensions (must have `max_tokens > 0`).
        budget: QueryBudget,
    },
    /// Impact query: returns the set of nodes that depend on `target` and
    /// would require re-verification if `target` changed.
    ///
    /// The response `structured` slice contains the dependent nodes, sorted
    /// by `NodeRef`.  Edges with `EdgeKind::BreaksIfChanged` pointing at
    /// `target` are used as the direct-dependency set; further transitive
    /// hops follow `DependsOn`, `Calls`, `Reads`, and `Writes` edges.
    Impact {
        /// The node whose change-impact is being assessed.
        target: NodeRef,
        /// Budget and scoping dimensions (must have `max_tokens > 0`).
        budget: QueryBudget,
    },
    /// Callers query: returns nodes that call `target` via `EdgeKind::Calls`.
    ///
    /// When `transitive` is `false`, only direct callers (one hop) are
    /// returned.  When `true`, a BFS follows `Calls` edges backward from
    /// `target` until no new callers are found or `budget` is exhausted.
    Callers {
        /// The node whose callers are requested.
        target: NodeRef,
        /// Whether to include transitive callers (BFS) in addition to
        /// direct callers.
        transitive: bool,
        /// Budget and scoping dimensions (must have `max_tokens > 0`).
        budget: QueryBudget,
    },
    /// Callees query: returns nodes that `target` calls via `EdgeKind::Calls`.
    ///
    /// When `transitive` is `false`, only direct callees (one hop) are
    /// returned.  When `true`, a BFS follows `Calls` edges forward from
    /// `target` until no new callees are found or `budget` is exhausted.
    Callees {
        /// The node whose callees are requested.
        target: NodeRef,
        /// Whether to include transitive callees (BFS) in addition to
        /// direct callees.
        transitive: bool,
        /// Budget and scoping dimensions (must have `max_tokens > 0`).
        budget: QueryBudget,
    },
    /// Effects query: returns declared effects and capabilities for `target`.
    ///
    /// The response `structured` slice contains only the target node (with
    /// its `effect_row` and `capability_reqs` fields populated if present).
    /// Nodes reachable via `EdgeKind::Emits` are also included.
    Effects {
        /// The node whose effects and capabilities are requested.
        target: NodeRef,
        /// Budget and scoping dimensions (must have `max_tokens > 0`).
        budget: QueryBudget,
    },
    /// Contracts query: returns contract clauses (requires/ensures) for `target`.
    ///
    /// The response `structured` slice contains only the target node (with
    /// its `contract_clauses` field populated if present).
    Contracts {
        /// The node whose contracts are requested.
        target: NodeRef,
        /// Budget and scoping dimensions (must have `max_tokens > 0`).
        budget: QueryBudget,
    },
    /// History query: returns the provenance chain for `target`.
    ///
    /// The response `history_entries` field on `ContextResponse` contains
    /// `SnapshotEnvelope` records (ordered oldest-first) in which the
    /// node's containing snapshot appears.  The `structured` slice contains
    /// the target node itself (from the most recent snapshot).
    History {
        /// The node whose provenance chain is requested.
        target: NodeRef,
        /// Budget and scoping dimensions (must have `max_tokens > 0`).
        budget: QueryBudget,
    },
    /// Proofs query: returns proof obligations and their current status for
    /// `target`.
    ///
    /// The response `structured` slice contains the target node (with
    /// `contract_clauses` populated) plus nodes reachable via `EdgeKind::Proves`
    /// edges (proof witnesses).  This covers the `proofs` and `obligations`
    /// doc query kinds.
    Proofs {
        /// The node whose proof obligations are requested.
        target: NodeRef,
        /// Budget and scoping dimensions (must have `max_tokens > 0`).
        budget: QueryBudget,
    },
    /// Resources query: returns resource handles, ownership modes, and
    /// concurrency information for `target`.
    ///
    /// The response `structured` slice contains the target node plus nodes
    /// reachable via `EdgeKind::Reads` and `EdgeKind::Writes` edges (data
    /// dependencies that imply resource acquisition).
    Resources {
        /// The node whose resource usage is requested.
        target: NodeRef,
        /// Budget and scoping dimensions (must have `max_tokens > 0`).
        budget: QueryBudget,
    },
    /// Boundaries query: returns architectural boundary nodes and trust
    /// metadata for `target`.
    ///
    /// The response `structured` slice contains nodes with `NodeKind::Boundary`
    /// reachable from `target` via any edge, plus the target itself.  Trust
    /// metadata (`trust_metadata` field on `GraphNode`) is preserved in the
    /// returned nodes.
    Boundaries {
        /// The node or module whose boundaries are requested.
        target: NodeRef,
        /// Budget and scoping dimensions (must have `max_tokens > 0`).
        budget: QueryBudget,
    },
    /// Why query: returns a provenance trace explaining why a claim or edge
    /// exists for `target`.
    ///
    /// The response `structured` slice contains the target node plus nodes
    /// reachable via `EdgeKind::Proves` edges (proof witnesses) and
    /// `EdgeKind::BreaksIfChanged` edges (change-impact dependencies).
    /// The `history_entries` field carries the snapshot chain as provenance
    /// context (same as `History`).
    Why {
        /// The node whose existence/behaviour is being traced.
        target: NodeRef,
        /// Budget and scoping dimensions (must have `max_tokens > 0`).
        budget: QueryBudget,
    },
    /// RefactorContext query: returns prerequisites and risk analysis for
    /// safely refactoring `target`.
    ///
    /// The response `structured` slice contains the target node plus:
    /// - Callers (via `EdgeKind::Calls` reverse BFS) — nodes to update.
    /// - Contract nodes (via `EdgeKind::Proves`) — proofs to rerun.
    /// - Effect nodes (via `EdgeKind::Emits`) — effects to preserve.
    RefactorContext {
        /// The node to be refactored.
        target: NodeRef,
        /// Budget and scoping dimensions (must have `max_tokens > 0`).
        budget: QueryBudget,
    },
    /// Runtime query: returns runtime profile grants, limits, and audit
    /// availability for `target`.
    ///
    /// The response `structured` slice contains the target node (with
    /// `capability_reqs` and `effect_row` populated if available) plus
    /// nodes reachable via `EdgeKind::Emits` (runtime effects).
    /// The `profile` label is stored in the response summary for traceability.
    Runtime {
        /// The node whose runtime profile is requested.
        target: NodeRef,
        /// Runtime profile identifier (e.g., `"prod"`, `"dev"`, `"test"`).
        profile: String,
        /// Budget and scoping dimensions (must have `max_tokens > 0`).
        budget: QueryBudget,
    },
    /// Diff query: returns structural differences between two snapshots or the
    /// nodes changed by a specific change reference.
    ///
    /// When `snapshot_b` is `None`, returns nodes changed relative to the parent
    /// of the current snapshot.  The `structured` slice contains affected nodes
    /// sorted by `NodeRef`.
    Diff {
        /// First snapshot reference (older); `None` means the parent snapshot.
        snapshot_a: Option<ObjectId>,
        /// Second snapshot reference (newer); `None` means the current snapshot.
        snapshot_b: Option<ObjectId>,
        /// Budget and scoping dimensions (must have `max_tokens > 0`).
        budget: QueryBudget,
    },
    /// Risks query: returns risk annotations for `target` or a proposed change.
    ///
    /// The response `structured` slice contains the target node plus nodes
    /// reachable via `EdgeKind::BreaksIfChanged` (change-impact dependencies).
    /// A `risk_level` string is attached to the summary.
    Risks {
        /// The node or change whose risks are being assessed.
        target: NodeRef,
        /// Budget and scoping dimensions (must have `max_tokens > 0`).
        budget: QueryBudget,
    },
    /// Todo query: returns outstanding obligations for `target` or a change.
    ///
    /// The response `structured` slice contains nodes with unverified
    /// proof obligations reachable from `target` via `EdgeKind::Proves`.
    Todo {
        /// The node or change whose outstanding obligations are listed.
        target: NodeRef,
        /// Budget and scoping dimensions (must have `max_tokens > 0`).
        budget: QueryBudget,
    },
    /// Capabilities query: returns granted capabilities for `target` in a profile.
    ///
    /// The response `structured` slice contains the target node plus capability
    /// nodes reachable via `EdgeKind::Emits` and `EdgeKind::DependsOn`.
    Capabilities {
        /// The node or module whose capabilities are requested.
        target: NodeRef,
        /// Runtime profile identifier (e.g., `"prod"`, `"dev"`, `"test"`).
        profile: String,
        /// Budget and scoping dimensions (must have `max_tokens > 0`).
        budget: QueryBudget,
    },
    /// Handlers query: returns handler bindings for a capability in a profile.
    ///
    /// The response `structured` slice contains nodes bound as handlers for the
    /// `target` capability via `EdgeKind::Calls` edges from boundary nodes.
    Handlers {
        /// The capability node whose handler bindings are requested.
        target: NodeRef,
        /// Runtime profile identifier (e.g., `"prod"`, `"dev"`, `"test"`).
        profile: String,
        /// Budget and scoping dimensions (must have `max_tokens > 0`).
        budget: QueryBudget,
    },
    /// Concurrency query: returns task groups, channels, and shared state for `target`.
    ///
    /// The response `structured` slice contains the target node plus nodes
    /// reachable via `EdgeKind::Reads`, `EdgeKind::Writes`, and `EdgeKind::Calls`.
    Concurrency {
        /// The node or module whose concurrency information is requested.
        target: NodeRef,
        /// Budget and scoping dimensions (must have `max_tokens > 0`).
        budget: QueryBudget,
    },
    /// Tasks query: returns async task groups and await/cancel status for `target`.
    ///
    /// The response `structured` slice contains the target node plus async-task
    /// nodes reachable via `EdgeKind::Calls` and `EdgeKind::Emits` edges.
    Tasks {
        /// The node whose task groups are requested.
        target: NodeRef,
        /// Budget and scoping dimensions (must have `max_tokens > 0`).
        budget: QueryBudget,
    },
    /// Assumptions query: returns trust assumptions for `target` boundary.
    ///
    /// The response `structured` slice contains assumption nodes reachable from
    /// `target` via any edge, filtered to nodes with trust metadata.
    Assumptions {
        /// The node or boundary whose assumptions are listed.
        target: NodeRef,
        /// Budget and scoping dimensions (must have `max_tokens > 0`).
        budget: QueryBudget,
    },
    /// ExtractCandidates query: returns sub-expressions or sub-functions within
    /// `target` that are candidates for extraction (refactor support).
    ///
    /// The response `structured` slice contains nodes reachable from `target`
    /// via `EdgeKind::Calls` and `EdgeKind::DependsOn` that have no callers
    /// outside `target` (i.e., safe to extract).
    ExtractCandidates {
        /// The node whose extractable sub-components are identified.
        target: NodeRef,
        /// Budget and scoping dimensions (must have `max_tokens > 0`).
        budget: QueryBudget,
    },
    /// MoveSafety query: assesses whether `target` can be safely moved to `destination`.
    ///
    /// Returns callers, contracts, effects, and proof obligations that would be
    /// affected.  The `destination` is a `NodeRef` for the target module/scope.
    MoveSafety {
        /// The node to be moved.
        target: NodeRef,
        /// The destination scope/module `NodeRef`.
        destination: NodeRef,
        /// Budget and scoping dimensions (must have `max_tokens > 0`).
        budget: QueryBudget,
    },
}

impl ContextQuery {
    /// The effective byte budget for the structured layer.
    ///
    /// Returns `budget.effective_bytes()` (`budget.max_tokens`) — the
    /// primary dimension used by `ResponseBuilder` as the byte limit.
    pub fn budget(&self) -> usize {
        match self {
            ContextQuery::Node { budget, .. }
            | ContextQuery::Graph { budget, .. }
            | ContextQuery::Impact { budget, .. }
            | ContextQuery::Callers { budget, .. }
            | ContextQuery::Callees { budget, .. }
            | ContextQuery::Effects { budget, .. }
            | ContextQuery::Contracts { budget, .. }
            | ContextQuery::History { budget, .. }
            | ContextQuery::Proofs { budget, .. }
            | ContextQuery::Resources { budget, .. }
            | ContextQuery::Boundaries { budget, .. }
            | ContextQuery::Why { budget, .. }
            | ContextQuery::RefactorContext { budget, .. }
            | ContextQuery::Runtime { budget, .. }
            | ContextQuery::Diff { budget, .. }
            | ContextQuery::Risks { budget, .. }
            | ContextQuery::Todo { budget, .. }
            | ContextQuery::Capabilities { budget, .. }
            | ContextQuery::Handlers { budget, .. }
            | ContextQuery::Concurrency { budget, .. }
            | ContextQuery::Tasks { budget, .. }
            | ContextQuery::Assumptions { budget, .. }
            | ContextQuery::ExtractCandidates { budget, .. }
            | ContextQuery::MoveSafety { budget, .. } => budget.effective_bytes(),
        }
    }

    /// Return a reference to the `QueryBudget` for the full budget dimensions.
    pub fn query_budget(&self) -> &QueryBudget {
        match self {
            ContextQuery::Node { budget, .. }
            | ContextQuery::Graph { budget, .. }
            | ContextQuery::Impact { budget, .. }
            | ContextQuery::Callers { budget, .. }
            | ContextQuery::Callees { budget, .. }
            | ContextQuery::Effects { budget, .. }
            | ContextQuery::Contracts { budget, .. }
            | ContextQuery::History { budget, .. }
            | ContextQuery::Proofs { budget, .. }
            | ContextQuery::Resources { budget, .. }
            | ContextQuery::Boundaries { budget, .. }
            | ContextQuery::Why { budget, .. }
            | ContextQuery::RefactorContext { budget, .. }
            | ContextQuery::Runtime { budget, .. }
            | ContextQuery::Diff { budget, .. }
            | ContextQuery::Risks { budget, .. }
            | ContextQuery::Todo { budget, .. }
            | ContextQuery::Capabilities { budget, .. }
            | ContextQuery::Handlers { budget, .. }
            | ContextQuery::Concurrency { budget, .. }
            | ContextQuery::Tasks { budget, .. }
            | ContextQuery::Assumptions { budget, .. }
            | ContextQuery::ExtractCandidates { budget, .. }
            | ContextQuery::MoveSafety { budget, .. } => budget,
        }
    }

    /// Return the primary target `NodeRef`, if this query is node-scoped.
    ///
    /// Returns `None` for `Graph` and `Diff` queries.
    pub fn target(&self) -> Option<NodeRef> {
        match self {
            ContextQuery::Node { target, .. }
            | ContextQuery::Impact { target, .. }
            | ContextQuery::Callers { target, .. }
            | ContextQuery::Callees { target, .. }
            | ContextQuery::Effects { target, .. }
            | ContextQuery::Contracts { target, .. }
            | ContextQuery::History { target, .. }
            | ContextQuery::Proofs { target, .. }
            | ContextQuery::Resources { target, .. }
            | ContextQuery::Boundaries { target, .. }
            | ContextQuery::Why { target, .. }
            | ContextQuery::RefactorContext { target, .. }
            | ContextQuery::Runtime { target, .. }
            | ContextQuery::Risks { target, .. }
            | ContextQuery::Todo { target, .. }
            | ContextQuery::Capabilities { target, .. }
            | ContextQuery::Handlers { target, .. }
            | ContextQuery::Concurrency { target, .. }
            | ContextQuery::Tasks { target, .. }
            | ContextQuery::Assumptions { target, .. }
            | ContextQuery::ExtractCandidates { target, .. }
            | ContextQuery::MoveSafety { target, .. } => Some(*target),
            ContextQuery::Graph { .. } | ContextQuery::Diff { .. } => None,
        }
    }
}
