use super::*;

// ─────────────────────────────────────────────────────────────────────
// R2 FEATURE TESTS: generated_at, provenance, redaction state, security,
//                   freshness detection, repair options, index reporting.
// ─────────────────────────────────────────────────────────────────────

// ── r2_generated_at_is_populated ─────────────────────────────────────
// Spec: generated_at is set in the response envelope.
#[test]
fn r2_generated_at_is_populated() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };
    let opts = BuildOptions {
        generated_at: 99_000,
        authorized: true,
        ..Default::default()
    };
    let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &no_redactions(), &opts)
        .expect("build_full must succeed");
    assert_eq!(
        resp.generated_at, 99_000,
        "generated_at must match opts value"
    );
}

// ── r2_freshness_stale_when_latest_differs ────────────────────────────
// Spec: freshness_status is Stale when latest_snapshot_id != snapshot.id
#[test]
fn r2_freshness_stale_when_latest_differs() {
    use crate::dto::FreshnessStatus;
    let graph = make_graph();
    let snapshot = make_snapshot(); // id = "builder-snap"
    let other_id = ObjectId::from_bytes(b"other-snap");
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };
    let opts = BuildOptions {
        latest_snapshot_id: Some(&other_id),
        authorized: true,
        ..Default::default()
    };
    let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &no_redactions(), &opts)
        .expect("build_full must succeed");
    assert_eq!(
        resp.freshness_status,
        FreshnessStatus::Stale,
        "freshness_status must be Stale when latest differs; got: {:?}",
        resp.freshness_status
    );
}

// ── r2_freshness_fresh_when_latest_matches ────────────────────────────
// TRIANGULATE: freshness_status is Fresh when latest == snapshot.id
#[test]
fn r2_freshness_fresh_when_latest_matches() {
    use crate::dto::FreshnessStatus;
    let graph = make_graph();
    let snapshot = make_snapshot();
    let snap_id = snapshot.id;
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };
    let opts = BuildOptions {
        latest_snapshot_id: Some(&snap_id),
        authorized: true,
        ..Default::default()
    };
    let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &no_redactions(), &opts)
        .expect("build_full must succeed");
    assert_eq!(
        resp.freshness_status,
        FreshnessStatus::Fresh,
        "freshness_status must be Fresh when latest matches; got: {:?}",
        resp.freshness_status
    );
}

// ── r2_stale_response_has_repair_option ───────────────────────────────
// Spec: Stale response must include a query_latest repair option.
#[test]
fn r2_stale_response_has_repair_option() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let other_id = ObjectId::from_bytes(b"newer-snap");
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };
    let opts = BuildOptions {
        latest_snapshot_id: Some(&other_id),
        authorized: true,
        ..Default::default()
    };
    let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &no_redactions(), &opts)
        .expect("build_full must succeed");
    assert!(
        resp.repair_options
            .iter()
            .any(|r| r.option_id == "query_latest"),
        "stale response must contain query_latest repair option; got: {:?}",
        resp.repair_options
    );
}

// ── r2_truncated_response_has_narrow_scope_repair_option ─────────────
// Spec: Truncated response must include a narrow_scope repair option.
#[test]
fn r2_truncated_response_has_narrow_scope_repair_option() {
    let graph = make_graph(); // 3 nodes
    let snapshot = make_snapshot();
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::bytes(1),
    }; // too small
    let opts = BuildOptions {
        authorized: true,
        ..Default::default()
    };
    let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &no_redactions(), &opts)
        .expect("build_full must succeed even when truncated");
    assert!(resp.truncated, "response must be truncated with budget=1");
    assert!(
        resp.repair_options
            .iter()
            .any(|r| r.option_id == "narrow_scope"),
        "truncated response must contain narrow_scope repair option; got: {:?}",
        resp.repair_options
    );
}

// ── r2_access_denied_for_unauthorized_redacted_target ─────────────────
// Spec: E_ACCESS_DENIED when unauthorized and target is redacted.
#[test]
fn r2_access_denied_for_unauthorized_redacted_target() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let mut redacted = BTreeSet::new();
    redacted.insert(NodeRef(0)); // redact the target
    let query = ContextQuery::Node {
        target: NodeRef(0),
        scope: QueryScope::Local,
        budget: QueryBudget::default(),
    };
    let opts = BuildOptions {
        authorized: false,
        ..Default::default()
    };
    let result = ResponseBuilder::build_full(&query, &graph, &snapshot, &redacted, &opts);
    assert_eq!(
        result,
        Err(ContextError::AccessDenied),
        "unauthorized access to redacted target must return E_ACCESS_DENIED"
    );
}

// ── r2_authorized_caller_can_access_redacted_target ───────────────────
// TRIANGULATE: authorized caller succeeds even when target is redacted
// (the node is removed from structured, but E_ACCESS_DENIED is not raised).
#[test]
fn r2_authorized_caller_can_access_redacted_target() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let mut redacted = BTreeSet::new();
    redacted.insert(NodeRef(0));
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };
    let opts = BuildOptions {
        authorized: true,
        ..Default::default()
    };
    let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &redacted, &opts)
        .expect("authorized call must succeed");
    assert!(resp.redacted, "redacted flag must be set");
    // Node 0 must be absent from structured.
    assert!(
        !resp.structured.iter().any(|n| n.id == NodeRef(0)),
        "redacted node must not appear in structured"
    );
}

// ── r2_redaction_policy_wired_into_response ───────────────────────────
// Spec: RedactionPolicy is attached to the response when supplied.
#[test]
fn r2_redaction_policy_wired_into_response() {
    use crate::dto::{RedactionPolicy, RedactionState};
    let graph = make_graph();
    let snapshot = make_snapshot();
    let mut redacted = BTreeSet::new();
    redacted.insert(NodeRef(1)); // redact node 1
    let policy = RedactionPolicy {
        label: "PII".to_string(),
        categories: vec!["secrets".to_string()],
        requires_approval: false,
    };
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };
    let opts = BuildOptions {
        authorized: true,
        redaction_policy: Some(&policy),
        ..Default::default()
    };
    let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &redacted, &opts)
        .expect("build must succeed");
    assert_eq!(
        resp.redaction_state,
        RedactionState::Partial,
        "redaction_state must be Partial when nodes are withheld; got: {:?}",
        resp.redaction_state
    );
    assert_eq!(
        resp.redaction_policy,
        Some(policy),
        "redaction_policy must be wired into the response"
    );
}

// ── r2_restricted_policy_sets_restricted_state ────────────────────────
// Spec: requires_approval=true → RedactionState::Restricted
#[test]
fn r2_restricted_policy_sets_restricted_state() {
    use crate::dto::{RedactionPolicy, RedactionState};
    let graph = make_graph();
    let snapshot = make_snapshot();
    let policy = RedactionPolicy {
        label: "internal".to_string(),
        categories: vec!["audit_logs".to_string()],
        requires_approval: true,
    };
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };
    let opts = BuildOptions {
        authorized: true,
        redaction_policy: Some(&policy),
        ..Default::default()
    };
    let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &no_redactions(), &opts)
        .expect("build must succeed");
    assert_eq!(
        resp.redaction_state,
        RedactionState::Restricted,
        "requires_approval policy must produce Restricted state; got: {:?}",
        resp.redaction_state
    );
}

// ── r2_provenance_block_includes_semantic_graph_source ────────────────
// Spec: provenance.sources always includes "semantic_graph".
#[test]
fn r2_provenance_block_includes_semantic_graph_source() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };
    let opts = BuildOptions {
        authorized: true,
        ..Default::default()
    };
    let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &no_redactions(), &opts)
        .expect("build must succeed");
    assert!(
        resp.provenance
            .sources
            .iter()
            .any(|s| s == "semantic_graph"),
        "provenance must contain semantic_graph source; got: {:?}",
        resp.provenance.sources
    );
}

// ── r2_provenance_block_includes_extra_sources ────────────────────────
// TRIANGULATE: extra sources supplied in opts are preserved.
#[test]
fn r2_provenance_block_includes_extra_sources() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let extra_sources = vec![
        "verification_reports".to_string(),
        "runtime_profiles".to_string(),
    ];
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };
    let opts = BuildOptions {
        authorized: true,
        provenance_sources: &extra_sources,
        ..Default::default()
    };
    let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &no_redactions(), &opts)
        .expect("build must succeed");
    assert!(
        resp.provenance
            .sources
            .iter()
            .any(|s| s == "verification_reports"),
        "provenance must contain verification_reports; got: {:?}",
        resp.provenance.sources
    );
}

// ── r2_index_info_attached_to_provenance ──────────────────────────────
// Spec: index versions/hashes are listed in provenance.indexes.
#[test]
fn r2_index_info_attached_to_provenance() {
    use crate::dto::IndexInfo;
    let graph = make_graph();
    let snapshot = make_snapshot();
    let indexes = vec![IndexInfo {
        kind: "call_graph".to_string(),
        hash: [0u8; 32],
        stale: false,
    }];
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };
    let opts = BuildOptions {
        authorized: true,
        index_info: &indexes,
        ..Default::default()
    };
    let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &no_redactions(), &opts)
        .expect("build must succeed");
    assert_eq!(
        resp.provenance.indexes.len(),
        1,
        "provenance.indexes must contain the supplied index info"
    );
    assert_eq!(resp.provenance.indexes[0].kind, "call_graph");
}

// ── r2_stale_index_triggers_rebuild_repair_option ─────────────────────
// Spec: stale index should trigger rebuild_index repair option.
#[test]
fn r2_stale_index_triggers_rebuild_repair_option() {
    use crate::dto::IndexInfo;
    let graph = make_graph();
    let snapshot = make_snapshot();
    let indexes = vec![IndexInfo {
        kind: "call_graph".to_string(),
        hash: [0u8; 32],
        stale: true, // stale!
    }];
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };
    let opts = BuildOptions {
        authorized: true,
        index_info: &indexes,
        ..Default::default()
    };
    let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &no_redactions(), &opts)
        .expect("build must succeed");
    assert!(
        resp.repair_options
            .iter()
            .any(|r| r.option_id == "rebuild_index"),
        "stale index must generate rebuild_index repair option; got: {:?}",
        resp.repair_options
    );
}

// ── r2_new_query_variants_cbor_roundtrip ──────────────────────────────
// All R2 query variants must survive CBOR roundtrip.
#[test]
fn r2_new_query_variants_cbor_roundtrip() {
    use ail_storage::codec::{CborCodec, ContentCodec};
    let codec = CborCodec;
    let variants: Vec<ContextQuery> = vec![
        ContextQuery::Diff {
            snapshot_a: None,
            snapshot_b: None,
            budget: QueryBudget::bytes(1024),
        },
        ContextQuery::Risks {
            target: NodeRef(1),
            budget: QueryBudget::bytes(512),
        },
        ContextQuery::Todo {
            target: NodeRef(2),
            budget: QueryBudget::bytes(256),
        },
        ContextQuery::Capabilities {
            target: NodeRef(3),
            profile: "prod".to_string(),
            budget: QueryBudget::bytes(2048),
        },
        ContextQuery::Handlers {
            target: NodeRef(4),
            profile: "dev".to_string(),
            budget: QueryBudget::bytes(4096),
        },
        ContextQuery::Concurrency {
            target: NodeRef(5),
            budget: QueryBudget::bytes(512),
        },
        ContextQuery::Tasks {
            target: NodeRef(6),
            budget: QueryBudget::bytes(1024),
        },
        ContextQuery::Assumptions {
            target: NodeRef(7),
            budget: QueryBudget::bytes(2048),
        },
        ContextQuery::ExtractCandidates {
            target: NodeRef(8),
            budget: QueryBudget::bytes(4096),
        },
        ContextQuery::MoveSafety {
            target: NodeRef(9),
            destination: NodeRef(10),
            budget: QueryBudget::bytes(8192),
        },
    ];
    for q in &variants {
        let bytes = codec.encode(q).expect("encode must succeed");
        let decoded: ContextQuery = codec.decode(&bytes).expect("decode must succeed");
        assert_eq!(decoded, *q, "{q:?} must survive CBOR roundtrip");
    }
}

// ── r2_redaction_state_cbor_roundtrip ─────────────────────────────────
// RedactionState enum must survive CBOR roundtrip.
#[test]
fn r2_redaction_state_cbor_roundtrip() {
    use crate::dto::RedactionState;
    use ail_storage::codec::{CborCodec, ContentCodec};
    let codec = CborCodec;
    for state in [
        RedactionState::None,
        RedactionState::Partial,
        RedactionState::Restricted,
    ] {
        let bytes = codec.encode(&state).expect("encode must succeed");
        let decoded: RedactionState = codec.decode(&bytes).expect("decode must succeed");
        assert_eq!(decoded, state, "{state:?} must survive CBOR roundtrip");
    }
}

// ── r2_provenance_block_cbor_roundtrip ────────────────────────────────
// ProvenanceBlock must survive CBOR roundtrip.
#[test]
fn r2_provenance_block_cbor_roundtrip() {
    use crate::dto::{IndexInfo, ProvenanceBlock};
    use ail_storage::codec::{CborCodec, ContentCodec};
    let codec = CborCodec;
    let prov = ProvenanceBlock {
        sources: vec![
            "semantic_graph".to_string(),
            "verification_reports".to_string(),
        ],
        indexes: vec![IndexInfo {
            kind: "call_graph".to_string(),
            hash: [1u8; 32],
            stale: false,
        }],
        reports: vec![[2u8; 32]],
    };
    let bytes = codec.encode(&prov).expect("encode must succeed");
    let decoded: ProvenanceBlock = codec.decode(&bytes).expect("decode must succeed");
    assert_eq!(decoded, prov, "ProvenanceBlock must survive CBOR roundtrip");
}

// ── r2_repair_option_cbor_roundtrip ───────────────────────────────────
// RepairOption must survive CBOR roundtrip.
#[test]
fn r2_repair_option_cbor_roundtrip() {
    use crate::dto::RepairOption;
    use ail_storage::codec::{CborCodec, ContentCodec};
    let codec = CborCodec;
    let opt = RepairOption {
        option_id: "query_latest".to_string(),
        description: "Re-issue at latest snapshot".to_string(),
        suggested_query: Some("context fn.checkout snapshot=latest".to_string()),
    };
    let bytes = codec.encode(&opt).expect("encode must succeed");
    let decoded: RepairOption = codec.decode(&bytes).expect("decode must succeed");
    assert_eq!(decoded, opt, "RepairOption must survive CBOR roundtrip");
}

// ── r2_full_response_cbor_roundtrip ───────────────────────────────────
// ContextResponse with all new R2 fields must survive CBOR roundtrip.
#[test]
fn r2_full_response_cbor_roundtrip() {
    use crate::dto::{FreshnessStatus, IndexInfo};
    use ail_storage::codec::{CborCodec, ContentCodec};
    let codec = CborCodec;
    let graph = make_graph();
    let snapshot = make_snapshot();
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };
    let other_id = ObjectId::from_bytes(b"other");
    let sources = vec!["semantic_graph".to_string()];
    let indexes = vec![IndexInfo {
        kind: "call_graph".to_string(),
        hash: [0u8; 32],
        stale: false,
    }];
    let opts = BuildOptions {
        authorized: true,
        latest_snapshot_id: Some(&other_id), // force Stale
        generated_at: 12345,
        provenance_sources: &sources,
        index_info: &indexes,
        ..Default::default()
    };
    let resp = ResponseBuilder::build_full(&query, &graph, &snapshot, &no_redactions(), &opts)
        .expect("build must succeed");
    assert_eq!(resp.freshness_status, FreshnessStatus::Stale);

    let bytes = codec.encode(&resp).expect("encode must succeed");
    let decoded: crate::dto::ContextResponse = codec.decode(&bytes).expect("decode must succeed");
    assert_eq!(
        decoded, resp,
        "full R2 ContextResponse must survive CBOR roundtrip"
    );
}
