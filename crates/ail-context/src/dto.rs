// ── ail-context::dto ──────────────────────────────────────────────────────
//
// Query/response/selector data-transfer objects for the context API.
//
// # Determinism contract
//
// All DTOs use `Vec` and `BTreeMap` only — never `HashMap` — to satisfy
// the CBOR determinism contract inherited from `ail-core` and `ail-storage`.
// No floating-point values; timestamps are `u64` Unix milliseconds.

use ail_core::semantic_graph::{GraphNode, NodeRef};
use ail_storage::graph::SnapshotEnvelope;
use ail_storage::object::ObjectId;
use serde::{Deserialize, Serialize};

// ── ImpactInfo ────────────────────────────────────────────────────────────

/// Structured impact classification attached to `Impact` query responses.
///
/// Provides a breakdown of the affected-node set into semantic categories
/// (tests, capabilities, public APIs) and a computed `risk_level` string so
/// consumers can make risk-aware decisions without re-classifying every node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ImpactInfo {
    /// `NodeRef`s of nodes with `NodeKind::Test` in the affected set.
    pub affected_tests: Vec<NodeRef>,
    /// `NodeRef`s of nodes with `NodeKind::Capability` in the affected set.
    pub affected_capabilities: Vec<NodeRef>,
    /// `NodeRef`s of nodes with `Visibility::Public` in the affected set.
    pub affected_public_apis: Vec<NodeRef>,
    /// Count of Contract/Invariant nodes that need re-verification.
    pub required_reverification: usize,
    /// Overall risk classification: `"none"`, `"low"`, `"medium"`, or `"high"`.
    ///
    /// Derived from `affected_public_apis.len() + required_reverification`:
    /// 0 → `"none"`, 1–3 → `"low"`, 4–10 → `"medium"`, >10 → `"high"`.
    pub risk_level: String,
}

// ── RefactorInfo ──────────────────────────────────────────────────────────

/// Structured refactoring support information for `RefactorContext`,
/// `ExtractCandidates`, and `MoveSafety` query responses.
///
/// Provides a pre-classified breakdown of nodes that must be considered when
/// performing a safe refactoring — what's locked, what must be preserved, and
/// what must be updated.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RefactorInfo {
    /// `NodeRef`s of nodes with `WorkflowState::Locked` in the context set.
    ///
    /// These represent behavior locks that constrain the refactoring.
    pub behavior_locks_needed: Vec<NodeRef>,
    /// `NodeRef`s of Contract/Invariant nodes that must be preserved.
    pub contracts_to_preserve: Vec<NodeRef>,
    /// `NodeRef`s of Effect/EffectAlias nodes that must be preserved.
    pub effects_to_preserve: Vec<NodeRef>,
    /// `NodeRef`s of nodes that call the refactoring target (must be updated).
    pub callers_to_update: Vec<NodeRef>,
    /// `NodeRef`s of proof nodes (via `Proves` edges) that need re-running.
    pub proofs_to_rerun: Vec<NodeRef>,
    /// `NodeRef`s of nodes that may conflict with this refactoring.
    ///
    /// Nodes with `BreaksIfChanged` edges in the context set.
    pub possible_conflicts: Vec<NodeRef>,
    /// Human-readable suggestions for performing the refactoring safely.
    pub suggested_refactor_ops: Vec<String>,
}

// ── RepairOption ──────────────────────────────────────────────────────────

/// A structured suggestion for recovering from a context error.
///
/// Mirrors the `repair_options` block in the context-server protocol doc.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairOption {
    /// Short identifier for the repair option (e.g., `"query_latest"`, `"narrow_scope"`).
    pub option_id: String,
    /// Human-readable description of the repair action.
    pub description: String,
    /// Suggested follow-up query in text form (e.g., `"context fn.checkout snapshot=latest"`).
    pub suggested_query: Option<String>,
}

// ── IndexInfo ─────────────────────────────────────────────────────────────

/// Metadata about a derived index used when building this response.
///
/// Each response lists index versions/hashes used so consumers can detect
/// stale indexes without re-querying.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexInfo {
    /// Index kind (e.g., `"call_graph"`, `"effect_graph"`, `"proof_obligation"`).
    pub kind: String,
    /// Content hash of the index at the time of this response.
    pub hash: [u8; 32],
    /// `true` when the index is behind the current snapshot.
    pub stale: bool,
}

// ── ProvenanceBlock ───────────────────────────────────────────────────────

/// Provenance information attached to every `ContextResponse`.
///
/// Mirrors the `provenance` block in the context-server protocol doc.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProvenanceBlock {
    /// Sources consulted (e.g., `"semantic_graph"`, `"verification_reports"`, `"runtime_profiles"`).
    pub sources: Vec<String>,
    /// Derived indexes used, with their versions/hashes.
    pub indexes: Vec<IndexInfo>,
    /// Verification/audit report hashes incorporated into this response.
    pub reports: Vec<[u8; 32]>,
}

// ── Schema constant ───────────────────────────────────────────────────────

/// Schema version string for `ContextResponse`, stable for the lifetime of
/// this wire-format generation.
pub const CONTEXT_SCHEMA_V1: &str = "context/1.0";

// ── FreshnessStatus ───────────────────────────────────────────────────────

/// The freshness state of a context response relative to the current graph.
///
/// Mirrors the `freshness fresh | stale | unknown` field in the protocol doc.
/// `Unknown` is returned when the server cannot determine whether the snapshot
/// is current (e.g., the index version is unavailable).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FreshnessStatus {
    #[default]
    /// The snapshot is the current graph root — no invalidation has occurred.
    Fresh,
    /// A newer snapshot exists; this response may describe stale state.
    Stale,
    /// Freshness cannot be determined (e.g., index not available).
    Unknown,
}

// ── RedactionState ────────────────────────────────────────────────────────

/// Explicit redaction state for a context response.
///
/// Mirrors the `redaction none | partial | restricted` field in the
/// context-server protocol doc.  This replaces the old `redacted: bool` flag
/// and carries richer semantic intent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RedactionState {
    #[default]
    /// No nodes were withheld; full structured content is present.
    None,
    /// One or more nodes were withheld; redacted fields are marked.
    Partial,
    /// The entire response is restricted; access requires approval.
    Restricted,
}

// ── RedactionPolicy ───────────────────────────────────────────────────────

/// Caller-supplied redaction policy that describes which node categories
/// are withheld and why.
///
/// The `RedactionPolicy` accompanies the `redacted_refs` set passed to
/// `ResponseBuilder`: it documents intent rather than driving the filter
/// (the `BTreeSet<NodeRef>` is still the operative list).  Responses include
/// this policy in their audit trail so consumers understand what was omitted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactionPolicy {
    /// Human-readable label for this policy (e.g., `"PII"`, `"restricted"`).
    pub label: String,
    /// Categories withheld (e.g., `["secrets", "PII", "audit_logs"]`).
    pub categories: Vec<String>,
    /// Whether a session capability is required to lift this policy.
    pub requires_approval: bool,
}

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

// ── QueryBudget ───────────────────────────────────────────────────────────

/// Fine-grained budget and scoping dimensions for a context query.
///
/// Replaces the single `budget: usize` byte limit with the full set of
/// dimensions specified in the context-server protocol doc:
///
/// ```txt
/// Budget fields:
///   max_depth            — limit BFS traversal depth
///   max_nodes            — limit total nodes returned
///   max_tokens           — byte limit for the structured layer (primary budget)
///   include_private      — whether to include private nodes
///   include_transitive   — whether to include transitive relationships
///   include_runtime_logs — whether to include runtime log data
///   profile              — runtime profile for capability/handler queries
/// ```
///
/// # Constructors
///
/// - `QueryBudget::bytes(n)` — set `max_tokens = n`, leave other fields as defaults.
/// - `QueryBudget::default()` — `max_tokens = usize::MAX` (unlimited byte budget).
///
/// # Effective byte limit
///
/// `QueryBudget::effective_bytes()` returns `max_tokens` — the value used
/// by `ResponseBuilder` as the byte limit for the structured layer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryBudget {
    /// Maximum BFS traversal depth from the query target.
    ///
    /// `None` means unlimited depth. When set, BFS stops at this hop count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_depth: Option<usize>,
    /// Maximum number of nodes to include in the structured layer.
    ///
    /// `None` means unlimited nodes (bounded only by `max_tokens`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_nodes: Option<usize>,
    /// Maximum total bytes for the structured layer.
    ///
    /// `0` is invalid and will be rejected with `E_INVALID_BUDGET`.
    /// This is the primary budget dimension and replaces the former
    /// `budget: usize` field.
    pub max_tokens: usize,
    /// When `true`, private nodes are included in the response.
    ///
    /// Private nodes are those with `Visibility::Private` or no visibility
    /// annotation.  Default is `false` (private nodes are omitted).
    #[serde(default)]
    pub include_private: bool,
    /// When `true`, transitive relationships are followed during BFS.
    ///
    /// For `Callers`/`Callees` queries this overrides the per-variant
    /// `transitive` flag.  Default is `true`.
    #[serde(default = "default_true")]
    pub include_transitive: bool,
    /// When `true`, runtime log data is included in the response.
    ///
    /// Runtime logs can be large; default is `false` (logs omitted).
    #[serde(default)]
    pub include_runtime_logs: bool,
    /// Runtime profile identifier for capability/handler/runtime queries.
    ///
    /// `None` means all profiles or the default profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Default for QueryBudget {
    fn default() -> Self {
        Self {
            max_depth: None,
            max_nodes: None,
            max_tokens: usize::MAX,
            include_private: false,
            include_transitive: true,
            include_runtime_logs: false,
            profile: None,
        }
    }
}

impl QueryBudget {
    /// Create a `QueryBudget` with `max_tokens = bytes` and all other fields
    /// at their defaults.
    ///
    /// This is the recommended constructor for callers that only care about
    /// the byte limit (equivalent to the former `budget: usize` usage).
    pub fn bytes(bytes: usize) -> Self {
        Self {
            max_tokens: bytes,
            ..Self::default()
        }
    }

    /// The effective byte limit used by `ResponseBuilder`.
    ///
    /// Returns `max_tokens` — the primary budget dimension.
    pub fn effective_bytes(&self) -> usize {
        self.max_tokens
    }
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

// ── ResponseLimits ────────────────────────────────────────────────────────

/// Budget accounting block attached to every `ContextResponse`.
///
/// Mirrors the `limits` block described in the context-server protocol doc.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseLimits {
    /// The byte budget that was in effect for this query.
    pub budget_bytes: usize,
    /// The total CBOR bytes consumed by the `structured` slice.
    pub bytes_used: usize,
    /// `true` when `bytes_used` reached `budget_bytes` before all candidate
    /// nodes were included.
    pub truncated: bool,
    /// Names of sections omitted due to budget exhaustion.
    ///
    /// Empty when `truncated` is `false`.  Example entries:
    /// `"transitive_callers"`, `"history_chain"`.
    pub omitted_sections: Vec<String>,
}

// ── ContextResponse ───────────────────────────────────────────────────────

/// The response envelope produced by resolving a `ContextQuery`.
///
/// `context_hash` is `blake3(CBOR(structured))` — byte-stable for identical
/// `structured` inputs regardless of other field values.
///
/// # Serialization
///
/// `ContextResponse` satisfies the determinism contract: `Vec`/`BTreeMap`
/// only, no `HashMap`, no floats.  `SnapshotEnvelope` has `PartialEq` only
/// (no `Eq`), so this struct follows suit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContextResponse {
    /// Schema version tag (always `"context/1.0"` for this generation).
    pub schema: String,
    /// The snapshot from which this slice was built.
    pub snapshot: SnapshotEnvelope,
    /// Content-addressed graph root; equals `snapshot.graph_root_hash`.
    pub graph_root_hash: ObjectId,
    /// `blake3(CBOR(query_bytes))` where `query_bytes = CBOR(ContextQuery)`.
    ///
    /// Stable identifier for the query that produced this response.
    /// Can be used by ChangeSets to assert which query they are based on.
    pub query_hash: [u8; 32],
    /// `blake3(CBOR(structured))` — stable for identical structured layers.
    pub context_hash: [u8; 32],
    /// Nodes matching the query, ordered by `NodeRef`.
    pub structured: Vec<GraphNode>,
    /// Text rendered from `structured` only (post-redaction/truncation).
    pub summary: String,
    /// Unix milliseconds: equals `snapshot.created_at`.
    pub freshness: u64,
    /// Unix milliseconds when this response was generated.
    ///
    /// Mirrors the `generated_at` field in the protocol doc.
    /// Set to the current wall-clock time at response build time.
    pub generated_at: u64,
    /// `true` when at least one node was withheld by the redaction policy.
    ///
    /// Legacy boolean kept for backward compatibility.  Prefer `redaction_state`.
    pub redacted: bool,
    /// Explicit redaction state for this response.
    ///
    /// Mirrors `redaction none | partial | restricted` in the protocol doc.
    #[serde(default, skip_serializing_if = "is_redaction_none")]
    pub redaction_state: RedactionState,
    /// The redaction policy applied to this response, if any.
    ///
    /// Populated when `redaction_state` is `Partial` or `Restricted`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redaction_policy: Option<RedactionPolicy>,
    /// `true` when the byte budget was exhausted before all matching nodes
    /// were included.
    ///
    /// Mirrors `limits.truncated`; kept for backward compatibility.
    pub truncated: bool,
    /// Budget accounting for this response.
    pub limits: ResponseLimits,
    /// Snapshot provenance chain for `History` and `Why` queries.
    ///
    /// Empty for all other query kinds.  Ordered oldest-first.
    pub history_entries: Vec<SnapshotEnvelope>,
    /// Freshness status relative to the current graph.
    ///
    /// `Fresh` when this response was built from the current snapshot.
    /// `Stale` when a newer snapshot exists.  `Unknown` when the server
    /// cannot determine currency (e.g., index unavailable).
    ///
    /// Serialized only when not `Fresh` (additive; pre-G27 decoders see
    /// `None` and may treat absence as fresh).
    #[serde(default, skip_serializing_if = "is_fresh")]
    pub freshness_status: FreshnessStatus,
    /// Provenance block listing sources, index versions, and report hashes.
    ///
    /// Mirrors the `provenance` block in the protocol doc.
    #[serde(default, skip_serializing_if = "is_provenance_empty")]
    pub provenance: ProvenanceBlock,
    /// Structured repair options when errors or staleness occurred.
    ///
    /// Empty for successful fresh responses.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repair_options: Vec<RepairOption>,
    /// Impact classification for `Impact` queries.
    ///
    /// `None` for all other query kinds.  Populated by `ResponseBuilder`
    /// when building responses for `ContextQuery::Impact`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impact_info: Option<ImpactInfo>,
    /// Refactoring support information for `RefactorContext`, `ExtractCandidates`,
    /// and `MoveSafety` queries.
    ///
    /// `None` for all other query kinds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refactor_info: Option<RefactorInfo>,
}

fn is_fresh(s: &FreshnessStatus) -> bool {
    *s == FreshnessStatus::Fresh
}

fn is_redaction_none(s: &RedactionState) -> bool {
    *s == RedactionState::None
}

fn is_provenance_empty(p: &ProvenanceBlock) -> bool {
    p.sources.is_empty() && p.indexes.is_empty() && p.reports.is_empty()
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use crate::dto::{ProvenanceBlock, RedactionState};
    use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef};
    use ail_storage::codec::{CborCodec, ContentCodec};
    use ail_storage::graph::SnapshotEnvelope;
    use ail_storage::object::ObjectId;

    fn make_snapshot() -> SnapshotEnvelope {
        let id = ObjectId::from_bytes(b"test-snap");
        SnapshotEnvelope {
            id,
            graph_root_hash: id,
            parent_id: None,
            applied_change_id: None,
            created_at: 42_000,
            verification_report_hash: None,
            ..Default::default()
        }
    }

    fn make_limits(budget: usize, used: usize) -> ResponseLimits {
        ResponseLimits {
            budget_bytes: budget,
            bytes_used: used,
            truncated: false,
            omitted_sections: Vec::new(),
        }
    }

    fn make_response(snapshot: SnapshotEnvelope, structured: Vec<GraphNode>) -> ContextResponse {
        let codec = CborCodec;
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: QueryBudget::default(),
        };
        let query_bytes = codec.encode(&query).expect("encode query");
        let query_hash = *blake3::hash(&query_bytes).as_bytes();
        let structured_bytes = codec.encode(&structured).expect("encode structured");
        let context_hash = *blake3::hash(&structured_bytes).as_bytes();
        let bytes_used = structured_bytes.len();
        ContextResponse {
            schema: CONTEXT_SCHEMA_V1.to_string(),
            graph_root_hash: snapshot.graph_root_hash,
            query_hash,
            context_hash,
            freshness: snapshot.created_at,
            generated_at: 0,
            snapshot,
            structured,
            summary: String::new(),
            redacted: false,
            redaction_state: RedactionState::None,
            redaction_policy: None,
            truncated: false,
            limits: make_limits(usize::MAX, bytes_used),
            history_entries: Vec::new(),
            freshness_status: FreshnessStatus::Fresh,
            provenance: ProvenanceBlock::default(),
            repair_options: Vec::new(),
            impact_info: None,
            refactor_info: None,
        }
    }

    // ── context_query_node_cbor_roundtrip ─────────────────────────────────
    // Spec: DTOs MUST use Vec/BTreeMap for deterministic CBOR.
    //
    // RED: `ContextQuery::Node` did not exist → compile error.
    // GREEN: enum + serde derive makes it compile and roundtrip cleanly.
    #[test]
    fn context_query_node_cbor_roundtrip() {
        let codec = CborCodec;
        let query = ContextQuery::Node {
            target: NodeRef(5),
            scope: QueryScope::Full,
            budget: QueryBudget::bytes(4096),
        };
        let bytes = codec.encode(&query).expect("encode must succeed");
        let decoded: ContextQuery = codec.decode(&bytes).expect("decode must succeed");
        assert_eq!(decoded, query, "ContextQuery must survive CBOR roundtrip");
    }

    // ── context_query_graph_cbor_roundtrip ────────────────────────────────
    // TRIANGULATE: Graph variant must also roundtrip.
    #[test]
    fn context_query_graph_cbor_roundtrip() {
        let codec = CborCodec;
        let query = ContextQuery::Graph {
            scope: QueryScope::Local,
            budget: QueryBudget::bytes(2048),
        };
        let bytes = codec.encode(&query).expect("encode must succeed");
        let decoded: ContextQuery = codec.decode(&bytes).expect("decode must succeed");
        assert_eq!(
            decoded, query,
            "ContextQuery::Graph must survive CBOR roundtrip"
        );
    }

    // ── new_query_variants_cbor_roundtrip ─────────────────────────────────
    // Spec: all new ContextQuery variants must survive CBOR roundtrip.
    #[test]
    fn new_query_variants_cbor_roundtrip() {
        let codec = CborCodec;
        let variants: Vec<ContextQuery> = vec![
            ContextQuery::Impact {
                target: NodeRef(1),
                budget: QueryBudget::bytes(1024),
            },
            ContextQuery::Callers {
                target: NodeRef(2),
                transitive: true,
                budget: QueryBudget::bytes(512),
            },
            ContextQuery::Callees {
                target: NodeRef(3),
                transitive: false,
                budget: QueryBudget::bytes(256),
            },
            ContextQuery::Effects {
                target: NodeRef(4),
                budget: QueryBudget::bytes(2048),
            },
            ContextQuery::Contracts {
                target: NodeRef(5),
                budget: QueryBudget::bytes(4096),
            },
            ContextQuery::History {
                target: NodeRef(6),
                budget: QueryBudget::bytes(8192),
            },
        ];
        for q in &variants {
            let bytes = codec.encode(q).expect("encode must succeed");
            let decoded: ContextQuery = codec.decode(&bytes).expect("decode must succeed");
            assert_eq!(decoded, *q, "{q:?} must survive CBOR roundtrip");
        }
    }

    // ── context_query_budget_accessor ─────────────────────────────────────
    // All variants expose .budget().
    #[test]
    fn context_query_budget_accessor() {
        let node_q = ContextQuery::Node {
            target: NodeRef(0),
            scope: QueryScope::Local,
            budget: QueryBudget::bytes(1024),
        };
        let graph_q = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: QueryBudget::bytes(512),
        };
        let impact_q = ContextQuery::Impact {
            target: NodeRef(0),
            budget: QueryBudget::bytes(333),
        };
        let callers_q = ContextQuery::Callers {
            target: NodeRef(0),
            transitive: false,
            budget: QueryBudget::bytes(444),
        };
        assert_eq!(node_q.budget(), 1024);
        assert_eq!(graph_q.budget(), 512);
        assert_eq!(impact_q.budget(), 333);
        assert_eq!(callers_q.budget(), 444);
    }

    // ── context_query_target_accessor ────────────────────────────────────
    // Node-scoped queries expose a target; Graph does not.
    #[test]
    fn context_query_target_accessor() {
        let node_q = ContextQuery::Node {
            target: NodeRef(7),
            scope: QueryScope::Local,
            budget: QueryBudget::bytes(1),
        };
        let graph_q = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: QueryBudget::bytes(1),
        };
        let impact_q = ContextQuery::Impact {
            target: NodeRef(9),
            budget: QueryBudget::bytes(1),
        };
        assert_eq!(node_q.target(), Some(NodeRef(7)));
        assert_eq!(graph_q.target(), None);
        assert_eq!(impact_q.target(), Some(NodeRef(9)));
    }

    // ── context_response_cbor_roundtrip ───────────────────────────────────
    // Spec scenario: "Re-serialization produces identical bytes" for ContextResponse.
    //
    // RED: `ContextResponse` struct did not exist → compile error.
    // GREEN: struct + serde derive enables roundtrip.
    #[test]
    fn context_response_cbor_roundtrip() {
        let codec = CborCodec;
        let snapshot = make_snapshot();
        let node = GraphNode::new(NodeRef(0), NodeKind::Module, "core");
        let structured = vec![node];
        let resp = make_response(snapshot, structured);
        let resp = ContextResponse {
            summary: "Module: core".to_string(),
            ..resp
        };

        let bytes = codec.encode(&resp).expect("encode must succeed");
        let decoded: ContextResponse = codec.decode(&bytes).expect("decode must succeed");
        assert_eq!(decoded, resp, "ContextResponse must survive CBOR roundtrip");
    }

    // ── context_response_deterministic_encoding ───────────────────────────
    // Spec scenario: "context_hash is stable for identical inputs".
    // TRIANGULATE: encoding the same ContextResponse twice produces identical bytes.
    #[test]
    fn context_response_deterministic_encoding() {
        let codec = CborCodec;
        let snapshot = make_snapshot();
        let resp = make_response(snapshot, Vec::new());

        let bytes_a = codec.encode(&resp).expect("first encode");
        let bytes_b = codec.encode(&resp).expect("second encode");
        assert_eq!(
            bytes_a, bytes_b,
            "identical ContextResponse must produce identical CBOR bytes"
        );
    }

    // ── context_response_has_schema_field ────────────────────────────────
    // Spec: schema field must equal CONTEXT_SCHEMA_V1 on every response.
    #[test]
    fn context_response_has_schema_field() {
        let snapshot = make_snapshot();
        let resp = make_response(snapshot, Vec::new());
        assert_eq!(
            resp.schema, CONTEXT_SCHEMA_V1,
            "schema must equal CONTEXT_SCHEMA_V1"
        );
    }

    // ── response_limits_roundtrip ─────────────────────────────────────────
    // Spec: ResponseLimits must survive CBOR roundtrip.
    #[test]
    fn response_limits_roundtrip() {
        let codec = CborCodec;
        let limits = ResponseLimits {
            budget_bytes: 4096,
            bytes_used: 1234,
            truncated: true,
            omitted_sections: vec!["transitive_callers".to_string()],
        };
        let bytes = codec.encode(&limits).expect("encode must succeed");
        let decoded: ResponseLimits = codec.decode(&bytes).expect("decode must succeed");
        assert_eq!(
            decoded, limits,
            "ResponseLimits must survive CBOR roundtrip"
        );
    }

    // ── g27_new_query_variants_cbor_roundtrip ────────────────────────────
    // Spec: all G27 ContextQuery variants must survive CBOR roundtrip.
    //
    // RED: Proofs/Resources/Boundaries/Why/RefactorContext/Runtime did not exist.
    // GREEN: enum variants + serde derive makes them compile and roundtrip.
    #[test]
    fn g27_new_query_variants_cbor_roundtrip() {
        let codec = CborCodec;
        let variants: Vec<ContextQuery> = vec![
            ContextQuery::Proofs {
                target: NodeRef(10),
                budget: QueryBudget::bytes(1024),
            },
            ContextQuery::Resources {
                target: NodeRef(11),
                budget: QueryBudget::bytes(2048),
            },
            ContextQuery::Boundaries {
                target: NodeRef(12),
                budget: QueryBudget::bytes(4096),
            },
            ContextQuery::Why {
                target: NodeRef(13),
                budget: QueryBudget::bytes(512),
            },
            ContextQuery::RefactorContext {
                target: NodeRef(14),
                budget: QueryBudget::bytes(8192),
            },
            ContextQuery::Runtime {
                target: NodeRef(15),
                profile: "prod".to_string(),
                budget: QueryBudget::bytes(16384),
            },
        ];
        for q in &variants {
            let bytes = codec.encode(q).expect("encode must succeed");
            let decoded: ContextQuery = codec.decode(&bytes).expect("decode must succeed");
            assert_eq!(decoded, *q, "{q:?} must survive CBOR roundtrip");
        }
    }

    // ── g27_budget_accessor_for_new_variants ─────────────────────────────
    // All new G27 variants expose .budget().
    #[test]
    fn g27_budget_accessor_for_new_variants() {
        assert_eq!(
            ContextQuery::Proofs {
                target: NodeRef(0),
                budget: QueryBudget::bytes(111)
            }
            .budget(),
            111
        );
        assert_eq!(
            ContextQuery::Resources {
                target: NodeRef(0),
                budget: QueryBudget::bytes(222)
            }
            .budget(),
            222
        );
        assert_eq!(
            ContextQuery::Boundaries {
                target: NodeRef(0),
                budget: QueryBudget::bytes(333)
            }
            .budget(),
            333
        );
        assert_eq!(
            ContextQuery::Why {
                target: NodeRef(0),
                budget: QueryBudget::bytes(444)
            }
            .budget(),
            444
        );
        assert_eq!(
            ContextQuery::RefactorContext {
                target: NodeRef(0),
                budget: QueryBudget::bytes(555)
            }
            .budget(),
            555
        );
        assert_eq!(
            ContextQuery::Runtime {
                target: NodeRef(0),
                profile: "dev".to_string(),
                budget: QueryBudget::bytes(666)
            }
            .budget(),
            666
        );
    }

    // ── g27_target_accessor_for_new_variants ─────────────────────────────
    // All new G27 variants expose .target() → Some(NodeRef).
    #[test]
    fn g27_target_accessor_for_new_variants() {
        assert_eq!(
            ContextQuery::Proofs {
                target: NodeRef(10),
                budget: QueryBudget::bytes(1)
            }
            .target(),
            Some(NodeRef(10))
        );
        assert_eq!(
            ContextQuery::Resources {
                target: NodeRef(11),
                budget: QueryBudget::bytes(1)
            }
            .target(),
            Some(NodeRef(11))
        );
        assert_eq!(
            ContextQuery::Runtime {
                target: NodeRef(15),
                profile: "test".to_string(),
                budget: QueryBudget::bytes(1)
            }
            .target(),
            Some(NodeRef(15))
        );
    }

    // ── freshness_status_cbor_roundtrip ──────────────────────────────────
    // Spec: FreshnessStatus must survive CBOR roundtrip.
    //
    // RED: FreshnessStatus did not exist → compile error.
    // GREEN: enum + serde derive makes it compile and roundtrip.
    #[test]
    fn freshness_status_cbor_roundtrip() {
        let codec = CborCodec;
        for status in [
            FreshnessStatus::Fresh,
            FreshnessStatus::Stale,
            FreshnessStatus::Unknown,
        ] {
            let bytes = codec.encode(&status).expect("encode must succeed");
            let decoded: FreshnessStatus = codec.decode(&bytes).expect("decode must succeed");
            assert_eq!(decoded, status, "{status:?} must survive CBOR roundtrip");
        }
    }

    // ── freshness_status_fresh_is_default ────────────────────────────────
    // Fresh is the default — not serialized (additive wire compat).
    #[test]
    fn freshness_status_fresh_is_default() {
        let snapshot = make_snapshot();
        let resp = make_response(snapshot, Vec::new());
        assert_eq!(
            resp.freshness_status,
            FreshnessStatus::Fresh,
            "default response must have FreshnessStatus::Fresh"
        );
    }

    // ── redaction_policy_cbor_roundtrip ──────────────────────────────────
    // Spec: RedactionPolicy must survive CBOR roundtrip.
    //
    // RED: RedactionPolicy did not exist → compile error.
    // GREEN: struct + serde derive makes it compile and roundtrip.
    #[test]
    fn redaction_policy_cbor_roundtrip() {
        let codec = CborCodec;
        let policy = RedactionPolicy {
            label: "PII".to_string(),
            categories: vec!["secrets".to_string(), "audit_logs".to_string()],
            requires_approval: true,
        };
        let bytes = codec.encode(&policy).expect("encode must succeed");
        let decoded: RedactionPolicy = codec.decode(&bytes).expect("decode must succeed");
        assert_eq!(
            decoded, policy,
            "RedactionPolicy must survive CBOR roundtrip"
        );
    }

    // ── context_response_with_stale_status_roundtrip ─────────────────────
    // TRIANGULATE: ContextResponse with Stale freshness_status must roundtrip.
    #[test]
    fn context_response_with_stale_status_roundtrip() {
        let codec = CborCodec;
        let snapshot = make_snapshot();
        let resp = ContextResponse {
            freshness_status: FreshnessStatus::Stale,
            ..make_response(snapshot, Vec::new())
        };
        let bytes = codec.encode(&resp).expect("encode must succeed");
        let decoded: ContextResponse = codec.decode(&bytes).expect("decode must succeed");
        assert_eq!(
            decoded.freshness_status,
            FreshnessStatus::Stale,
            "Stale freshness_status must survive CBOR roundtrip"
        );
    }
}
