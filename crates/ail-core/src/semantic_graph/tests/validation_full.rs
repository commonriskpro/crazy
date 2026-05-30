use super::*;

// ── validate_full: valid graph returns empty errors ───────────────────
// Spec: validate_full on a clean graph returns zero errors.
//
// RED: validate_full() did not exist → compile error.
// GREEN: method added with all checks → returns empty vec.
#[test]
fn validate_full_valid_graph_returns_no_errors() {
    let graph = SemanticGraph {
        nodes: vec![
            node(0, NodeKind::Module, "core"),
            node(1, NodeKind::Function, "run"),
            node(2, NodeKind::Effect, "io"),
        ],
        edges: vec![edge(0, 1, EdgeKind::DependsOn), edge(1, 2, EdgeKind::Emits)],
    };
    let errors = graph.validate_full();
    assert!(
        errors.is_empty(),
        "clean graph must produce zero errors; got: {errors:?}"
    );
}

// ── validate_full: duplicate ref detected ────────────────────────────
// Spec: validate_full returns DuplicateRef for duplicate NodeRef(0).
#[test]
fn validate_full_detects_duplicate_ref() {
    let graph = SemanticGraph {
        nodes: vec![
            node(0, NodeKind::Module, "a"),
            node(0, NodeKind::Function, "b"), // duplicate
        ],
        edges: vec![],
    };
    let errors = graph.validate_full();
    assert!(
        errors.contains(&GraphValidationError::DuplicateRef(NodeRef(0))),
        "must detect duplicate NodeRef(0); got: {errors:?}"
    );
}

// ── validate_full: dangling edge detected ─────────────────────────────
// TRIANGULATE: different error kind from duplicate.
// Spec: validate_full returns DanglingEdge for missing edge endpoint.
#[test]
fn validate_full_detects_dangling_edge() {
    let graph = SemanticGraph {
        nodes: vec![node(0, NodeKind::Module, "src")],
        edges: vec![edge(0, 99, EdgeKind::DependsOn)], // target 99 missing
    };
    let errors = graph.validate_full();
    assert!(
        errors.contains(&GraphValidationError::DanglingEdge {
            r#ref: NodeRef(99),
            role: DanglingRole::Target,
        }),
        "must detect dangling target NodeRef(99); got: {errors:?}"
    );
}

// ── validate_full: effect_row without Emits edge is rejected ─────────
// Spec: A node with non-empty effect_row but no Emits edge is incoherent.
//
// RED: EffectRowNoEmitsEdge variant did not exist → compile error.
// GREEN: Pass 3 in validate_full() detects the missing edge.
#[test]
fn validate_full_detects_effect_row_without_emits_edge() {
    let mut fn_node = node(0, NodeKind::Function, "pay");
    fn_node.effect_row = Some(EffectRow {
        effects: vec!["IO".to_string()],
    });
    let graph = SemanticGraph {
        nodes: vec![fn_node],
        edges: vec![], // no Emits edge!
    };
    let errors = graph.validate_full();
    assert!(
        errors.contains(&GraphValidationError::EffectRowNoEmitsEdge(NodeRef(0))),
        "must detect effect_row without Emits edge; got: {errors:?}"
    );
}

// ── validate_full: effect_row WITH Emits edge passes ─────────────────
// TRIANGULATE: coherent effect_row must not produce an error.
#[test]
fn validate_full_effect_row_with_emits_edge_passes() {
    let mut fn_node = node(0, NodeKind::Function, "pay");
    fn_node.effect_row = Some(EffectRow {
        effects: vec!["IO".to_string()],
    });
    let io_node = node(1, NodeKind::Effect, "io");
    let graph = SemanticGraph {
        nodes: vec![fn_node, io_node],
        edges: vec![edge(0, 1, EdgeKind::Emits)],
    };
    let errors = graph.validate_full();
    let effect_row_errors: Vec<_> = errors
        .iter()
        .filter(|e| matches!(e, GraphValidationError::EffectRowNoEmitsEdge(_)))
        .collect();
    assert!(
        effect_row_errors.is_empty(),
        "coherent effect_row+Emits must not produce EffectRowNoEmitsEdge; got: {errors:?}"
    );
}

// ── validate_full: capability_reqs missing Capability node ───────────
// Spec: A capability requirement that names a non-existent Capability node
// is incoherent.
//
// RED: CapabilityReqsMissingNode variant did not exist → compile error.
// GREEN: Pass 4 in validate_full() detects the missing node.
#[test]
fn validate_full_detects_capability_req_missing_node() {
    let mut fn_node = node(0, NodeKind::Function, "transfer");
    fn_node.capability_reqs = Some(CapabilityReqs {
        caps: vec!["net:read".to_string()],
    });
    let graph = SemanticGraph {
        nodes: vec![fn_node],
        edges: vec![], // no Capability node named "net:read"
    };
    let errors = graph.validate_full();
    assert!(
        errors.contains(&GraphValidationError::CapabilityReqsMissingNode {
            owner_ref: NodeRef(0),
            cap_name: "net:read".to_string(),
        }),
        "must detect missing Capability node 'net:read'; got: {errors:?}"
    );
}

// ── validate_full: capability_reqs WITH matching Capability node passes
// TRIANGULATE: satisfied capability_reqs must not produce an error.
#[test]
fn validate_full_capability_reqs_with_matching_node_passes() {
    let mut fn_node = node(0, NodeKind::Function, "transfer");
    fn_node.capability_reqs = Some(CapabilityReqs {
        caps: vec!["net:read".to_string()],
    });
    let cap_node = node(1, NodeKind::Capability, "net:read");
    let graph = SemanticGraph {
        nodes: vec![fn_node, cap_node],
        edges: vec![],
    };
    let errors = graph.validate_full();
    let cap_errors: Vec<_> = errors
        .iter()
        .filter(|e| matches!(e, GraphValidationError::CapabilityReqsMissingNode { .. }))
        .collect();
    assert!(
        cap_errors.is_empty(),
        "satisfied capability_reqs must not produce errors; got: {errors:?}"
    );
}

// ── validate_full: multiple errors returned at once ───────────────────
// Spec: validate_full returns ALL errors, not just the first one.
#[test]
fn validate_full_returns_all_errors() {
    // Two duplicate refs AND a dangling edge
    let graph = SemanticGraph {
        nodes: vec![
            node(0, NodeKind::Module, "a"),
            node(0, NodeKind::Function, "b"), // duplicate NodeRef(0)
        ],
        edges: vec![edge(0, 99, EdgeKind::DependsOn)], // dangling target 99
    };
    let errors = graph.validate_full();
    // Must contain at least DuplicateRef and DanglingEdge
    let has_dup = errors
        .iter()
        .any(|e| matches!(e, GraphValidationError::DuplicateRef(NodeRef(0))));
    let has_dangling = errors.iter().any(|e| {
        matches!(
            e,
            GraphValidationError::DanglingEdge {
                r#ref: NodeRef(99),
                role: DanglingRole::Target,
            }
        )
    });
    assert!(
        has_dup,
        "validate_full must include DuplicateRef error; got: {errors:?}"
    );
    assert!(
        has_dangling,
        "validate_full must include DanglingEdge error; got: {errors:?}"
    );
    assert!(
        errors.len() >= 2,
        "validate_full must return all errors, not just one; got: {errors:?}"
    );
}
