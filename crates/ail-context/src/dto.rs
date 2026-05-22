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
/// Only `ById` is supported by `StoreContextSource`; `InMemoryContextSource`
/// also supports `ById` for test predictability.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SnapshotSelector {
    /// Look up a specific snapshot by its `SnapshotEnvelope.id`.
    ById(ObjectId),
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
/// `budget` is a byte limit for the structured layer; zero is invalid and
/// will be rejected with `ContextError::InvalidBudget`.
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
        /// Maximum total bytes for the structured layer (must be > 0).
        budget: usize,
    },
    /// Context spanning the whole graph.
    Graph {
        /// Traversal scope.
        scope: QueryScope,
        /// Maximum total bytes for the structured layer (must be > 0).
        budget: usize,
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
        /// Maximum total bytes for the structured layer (must be > 0).
        budget: usize,
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
        /// Maximum total bytes for the structured layer (must be > 0).
        budget: usize,
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
        /// Maximum total bytes for the structured layer (must be > 0).
        budget: usize,
    },
    /// Effects query: returns declared effects and capabilities for `target`.
    ///
    /// The response `structured` slice contains only the target node (with
    /// its `effect_row` and `capability_reqs` fields populated if present).
    /// Nodes reachable via `EdgeKind::Emits` are also included.
    Effects {
        /// The node whose effects and capabilities are requested.
        target: NodeRef,
        /// Maximum total bytes for the structured layer (must be > 0).
        budget: usize,
    },
    /// Contracts query: returns contract clauses (requires/ensures) for `target`.
    ///
    /// The response `structured` slice contains only the target node (with
    /// its `contract_clauses` field populated if present).
    Contracts {
        /// The node whose contracts are requested.
        target: NodeRef,
        /// Maximum total bytes for the structured layer (must be > 0).
        budget: usize,
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
        /// Maximum total bytes for the structured layer (must be > 0).
        budget: usize,
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
        /// Maximum total bytes for the structured layer (must be > 0).
        budget: usize,
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
        /// Maximum total bytes for the structured layer (must be > 0).
        budget: usize,
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
        /// Maximum total bytes for the structured layer (must be > 0).
        budget: usize,
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
        /// Maximum total bytes for the structured layer (must be > 0).
        budget: usize,
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
        /// Maximum total bytes for the structured layer (must be > 0).
        budget: usize,
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
        /// Maximum total bytes for the structured layer (must be > 0).
        budget: usize,
    },
}

impl ContextQuery {
    /// The byte budget for the structured layer.
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
            | ContextQuery::Runtime { budget, .. } => *budget,
        }
    }

    /// Return the primary target `NodeRef`, if this query is node-scoped.
    ///
    /// Returns `None` for `Graph` queries.
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
            | ContextQuery::Runtime { target, .. } => Some(*target),
            ContextQuery::Graph { .. } => None,
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
    /// `true` when at least one node was withheld by the redaction policy.
    pub redacted: bool,
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
}

fn is_fresh(s: &FreshnessStatus) -> bool {
    *s == FreshnessStatus::Fresh
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
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
            budget: usize::MAX,
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
            snapshot,
            structured,
            summary: String::new(),
            redacted: false,
            truncated: false,
            limits: make_limits(usize::MAX, bytes_used),
            history_entries: Vec::new(),
            freshness_status: FreshnessStatus::Fresh,
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
            budget: 4096,
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
            budget: 2048,
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
                budget: 1024,
            },
            ContextQuery::Callers {
                target: NodeRef(2),
                transitive: true,
                budget: 512,
            },
            ContextQuery::Callees {
                target: NodeRef(3),
                transitive: false,
                budget: 256,
            },
            ContextQuery::Effects {
                target: NodeRef(4),
                budget: 2048,
            },
            ContextQuery::Contracts {
                target: NodeRef(5),
                budget: 4096,
            },
            ContextQuery::History {
                target: NodeRef(6),
                budget: 8192,
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
            budget: 1024,
        };
        let graph_q = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: 512,
        };
        let impact_q = ContextQuery::Impact {
            target: NodeRef(0),
            budget: 333,
        };
        let callers_q = ContextQuery::Callers {
            target: NodeRef(0),
            transitive: false,
            budget: 444,
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
            budget: 1,
        };
        let graph_q = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: 1,
        };
        let impact_q = ContextQuery::Impact {
            target: NodeRef(9),
            budget: 1,
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
                budget: 1024,
            },
            ContextQuery::Resources {
                target: NodeRef(11),
                budget: 2048,
            },
            ContextQuery::Boundaries {
                target: NodeRef(12),
                budget: 4096,
            },
            ContextQuery::Why {
                target: NodeRef(13),
                budget: 512,
            },
            ContextQuery::RefactorContext {
                target: NodeRef(14),
                budget: 8192,
            },
            ContextQuery::Runtime {
                target: NodeRef(15),
                profile: "prod".to_string(),
                budget: 16384,
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
            ContextQuery::Proofs { target: NodeRef(0), budget: 111 }.budget(),
            111
        );
        assert_eq!(
            ContextQuery::Resources { target: NodeRef(0), budget: 222 }.budget(),
            222
        );
        assert_eq!(
            ContextQuery::Boundaries { target: NodeRef(0), budget: 333 }.budget(),
            333
        );
        assert_eq!(
            ContextQuery::Why { target: NodeRef(0), budget: 444 }.budget(),
            444
        );
        assert_eq!(
            ContextQuery::RefactorContext { target: NodeRef(0), budget: 555 }.budget(),
            555
        );
        assert_eq!(
            ContextQuery::Runtime { target: NodeRef(0), profile: "dev".to_string(), budget: 666 }.budget(),
            666
        );
    }

    // ── g27_target_accessor_for_new_variants ─────────────────────────────
    // All new G27 variants expose .target() → Some(NodeRef).
    #[test]
    fn g27_target_accessor_for_new_variants() {
        assert_eq!(
            ContextQuery::Proofs { target: NodeRef(10), budget: 1 }.target(),
            Some(NodeRef(10))
        );
        assert_eq!(
            ContextQuery::Resources { target: NodeRef(11), budget: 1 }.target(),
            Some(NodeRef(11))
        );
        assert_eq!(
            ContextQuery::Runtime { target: NodeRef(15), profile: "test".to_string(), budget: 1 }.target(),
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
        for status in [FreshnessStatus::Fresh, FreshnessStatus::Stale, FreshnessStatus::Unknown] {
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
        assert_eq!(decoded, policy, "RedactionPolicy must survive CBOR roundtrip");
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
