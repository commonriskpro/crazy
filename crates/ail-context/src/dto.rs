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
}

impl ContextQuery {
    /// The byte budget for the structured layer.
    pub fn budget(&self) -> usize {
        match self {
            ContextQuery::Node { budget, .. } | ContextQuery::Graph { budget, .. } => *budget,
        }
    }
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
    /// The snapshot from which this slice was built.
    pub snapshot: SnapshotEnvelope,
    /// Content-addressed graph root; equals `snapshot.graph_root_hash`.
    pub graph_root_hash: ObjectId,
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
    pub truncated: bool,
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
        assert_eq!(decoded, query, "ContextQuery::Graph must survive CBOR roundtrip");
    }

    // ── context_query_budget_accessor ─────────────────────────────────────
    // Both variants expose .budget().
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
        assert_eq!(node_q.budget(), 1024);
        assert_eq!(graph_q.budget(), 512);
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
        let structured = vec![node.clone()];
        let structured_bytes = codec.encode(&structured).expect("encode structured");
        let context_hash = *blake3::hash(&structured_bytes).as_bytes();

        let resp = ContextResponse {
            graph_root_hash: snapshot.graph_root_hash,
            context_hash,
            freshness: snapshot.created_at,
            snapshot,
            structured,
            summary: "Module: core".to_string(),
            redacted: false,
            truncated: false,
        };

        let bytes = codec.encode(&resp).expect("encode must succeed");
        let decoded: ContextResponse = codec.decode(&bytes).expect("decode must succeed");
        assert_eq!(
            decoded, resp,
            "ContextResponse must survive CBOR roundtrip"
        );
    }

    // ── context_response_deterministic_encoding ───────────────────────────
    // Spec scenario: "context_hash is stable for identical inputs".
    // TRIANGULATE: encoding the same ContextResponse twice produces identical bytes.
    #[test]
    fn context_response_deterministic_encoding() {
        let codec = CborCodec;
        let snapshot = make_snapshot();
        let structured: Vec<GraphNode> = Vec::new();
        let structured_bytes = codec.encode(&structured).expect("encode structured");
        let context_hash = *blake3::hash(&structured_bytes).as_bytes();

        let resp = ContextResponse {
            graph_root_hash: snapshot.graph_root_hash,
            context_hash,
            freshness: snapshot.created_at,
            snapshot,
            structured,
            summary: String::new(),
            redacted: false,
            truncated: false,
        };

        let bytes_a = codec.encode(&resp).expect("first encode");
        let bytes_b = codec.encode(&resp).expect("second encode");
        assert_eq!(
            bytes_a, bytes_b,
            "identical ContextResponse must produce identical CBOR bytes"
        );
    }
}
