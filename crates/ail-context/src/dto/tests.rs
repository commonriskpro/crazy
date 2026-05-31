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
        redacted_descriptors: Vec::new(),
        diagnostics: Vec::new(),
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
