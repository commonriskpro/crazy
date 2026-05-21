// ── ail-compiler::lower — TDD tests (Task 2.1 RED + Task 2.3 RED + verify fixes) ──
//
// Tests for `lower_to_core_ir` and `lower_to_anf`.
// Written BEFORE the production code exists (strict TDD).

use ail_compiler::hash::{hash_with_parent, stable_cbor_bytes};
use ail_compiler::{CompileError, lower_to_anf, lower_to_core_ir};
use ail_core::semantic_graph::{EdgeKind, GraphEdge, GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_verify::report::{VerificationEntry, VerificationReport, VerificationState};

// ── Helpers ───────────────────────────────────────────────────────────────

/// Build a `VerificationReport` whose summary equals `state`.
/// Uses a single entry whose state is `state`.
fn report_with_state(state: VerificationState) -> VerificationReport {
    VerificationReport::new(vec![VerificationEntry {
        claim: "test claim".to_string(),
        state,
        scope: "test".to_string(),
        evidence: None,
    }])
}

/// Build an empty `VerificationReport` — summary is vacuous `Proven`.
fn proven_report() -> VerificationReport {
    VerificationReport::new(vec![])
}

/// Build a `SemanticGraph` with exactly N nodes (Module, Function, Effect,
/// Capability, Contract, Invariant, Test, Boundary — cycling through kinds).
fn make_n_node_graph(n: usize) -> SemanticGraph {
    let kinds = [
        NodeKind::Module,
        NodeKind::Function,
        NodeKind::Effect,
        NodeKind::Capability,
        NodeKind::Contract,
        NodeKind::Invariant,
        NodeKind::Test,
        NodeKind::Boundary,
    ];
    let nodes: Vec<GraphNode> = (0..n)
        .map(|i| {
            GraphNode::new(
                NodeRef(i as u32),
                kinds[i % kinds.len()],
                format!("node_{i}"),
            )
        })
        .collect();
    SemanticGraph {
        nodes,
        edges: vec![],
    }
}

// ── lower_to_core_ir: report gate ─────────────────────────────────────────

// Spec: "Rejected report is refused"
// GIVEN a VerificationReport with summary Failed
// WHEN lower_to_core_ir is called
// THEN Err(CompileError::RejectedReport) is returned
#[test]
fn rejected_report_failed_returns_rejected_error() {
    let graph = ail_testkit::make_semantic_graph();
    let report = report_with_state(VerificationState::Failed);
    let result = lower_to_core_ir(&graph, &report);
    assert_eq!(
        result,
        Err(CompileError::RejectedReport),
        "Failed summary must be refused"
    );
}

// TRIANGULATE: Assumed summary is also rejected
#[test]
fn rejected_report_assumed_returns_rejected_error() {
    let graph = make_n_node_graph(1);
    let report = report_with_state(VerificationState::Assumed);
    let result = lower_to_core_ir(&graph, &report);
    assert_eq!(result, Err(CompileError::RejectedReport));
}

// TRIANGULATE: Unverified summary is rejected
#[test]
fn rejected_report_unverified_returns_rejected_error() {
    let graph = make_n_node_graph(1);
    let report = report_with_state(VerificationState::Unverified);
    let result = lower_to_core_ir(&graph, &report);
    assert_eq!(result, Err(CompileError::RejectedReport));
}

// TRIANGULATE: Unsafe summary is rejected
#[test]
fn rejected_report_unsafe_returns_rejected_error() {
    let graph = make_n_node_graph(1);
    let report = report_with_state(VerificationState::Unsafe);
    let result = lower_to_core_ir(&graph, &report);
    assert_eq!(result, Err(CompileError::RejectedReport));
}

// Spec: "Accepted report proceeds" (Proven)
// GIVEN a VerificationReport with vacuous Proven summary (empty)
// WHEN lower_to_core_ir is called
// THEN Ok(CoreIr) is returned
#[test]
fn accepted_report_proven_succeeds() {
    let graph = ail_testkit::make_semantic_graph();
    let report = proven_report();
    let result = lower_to_core_ir(&graph, &report);
    assert!(result.is_ok(), "Proven report must be accepted: {result:?}");
}

// TRIANGULATE: RuntimeChecked summary is also accepted
#[test]
fn accepted_report_runtime_checked_succeeds() {
    let graph = make_n_node_graph(2);
    let report = report_with_state(VerificationState::RuntimeChecked);
    let result = lower_to_core_ir(&graph, &report);
    assert!(
        result.is_ok(),
        "RuntimeChecked report must be accepted: {result:?}"
    );
}

// ── lower_to_core_ir: total NodeRef coverage ──────────────────────────────

// Spec: "All nodes covered"
// GIVEN a SemanticGraph with N NodeRef entries
// WHEN lowering succeeds
// THEN the CoreIr contains exactly N nodes, each referencing its source NodeRef
#[test]
fn n_node_graph_produces_n_core_nodes() {
    let graph = make_n_node_graph(5);
    let report = proven_report();
    let core = lower_to_core_ir(&graph, &report).expect("must succeed");
    assert_eq!(
        core.nodes.len(),
        5,
        "must produce exactly one CoreNode per graph NodeRef"
    );
}

// TRIANGULATE: 1-node graph produces 1 CoreNode
#[test]
fn one_node_graph_produces_one_core_node() {
    let graph = make_n_node_graph(1);
    let report = proven_report();
    let core = lower_to_core_ir(&graph, &report).expect("must succeed");
    assert_eq!(core.nodes.len(), 1);
    assert_eq!(core.nodes[0].source_ref, NodeRef(0));
}

// TRIANGULATE: 3-node fixture — each CoreNode carries the correct source_ref
#[test]
fn core_nodes_carry_correct_source_refs() {
    let graph = ail_testkit::make_semantic_graph(); // NodeRef(0), (1), (2)
    let report = proven_report();
    let core = lower_to_core_ir(&graph, &report).expect("must succeed");
    assert_eq!(core.nodes.len(), 3);
    assert_eq!(core.nodes[0].source_ref, NodeRef(0));
    assert_eq!(core.nodes[1].source_ref, NodeRef(1));
    assert_eq!(core.nodes[2].source_ref, NodeRef(2));
}

// ── lower_to_core_ir: deterministic hash seal ─────────────────────────────

// Spec: "Same inputs produce same hash"
// GIVEN identical SemanticGraph and VerificationReport across two independent runs
// WHEN both runs complete
// THEN core_ir_hash is byte-identical in both outputs
#[test]
fn same_inputs_produce_identical_core_ir_hash() {
    let graph = ail_testkit::make_semantic_graph();
    let report = proven_report();
    let run1 = lower_to_core_ir(&graph, &report).expect("run1 must succeed");
    let run2 = lower_to_core_ir(&graph, &report).expect("run2 must succeed");
    assert_eq!(
        run1.stage_hashes.core_ir_hash, run2.stage_hashes.core_ir_hash,
        "core_ir_hash must be deterministic across runs"
    );
}

// TRIANGULATE: different graphs produce different core_ir_hash
#[test]
fn different_graphs_produce_different_core_ir_hash() {
    let graph_a = make_n_node_graph(2);
    let graph_b = make_n_node_graph(3);
    let report = proven_report();
    let core_a = lower_to_core_ir(&graph_a, &report).expect("a must succeed");
    let core_b = lower_to_core_ir(&graph_b, &report).expect("b must succeed");
    assert_ne!(
        core_a.stage_hashes.core_ir_hash, core_b.stage_hashes.core_ir_hash,
        "different graph inputs must produce different core_ir_hash"
    );
}

// ── lower_to_anf: deterministic output (Task 2.3 RED) ────────────────────

// Spec: "Deterministic output"
// GIVEN the same CoreIr input on two separate invocations
// WHEN lower_to_anf is called
// THEN both invocations return byte-identical AnfIr outputs (same anf_ir_hash)
#[test]
fn same_core_ir_produces_identical_anf_ir_hash() {
    let graph = ail_testkit::make_semantic_graph();
    let report = proven_report();
    let core = lower_to_core_ir(&graph, &report).expect("core must succeed");
    let anf1 = lower_to_anf(&core).expect("anf1 must succeed");
    let anf2 = lower_to_anf(&core).expect("anf2 must succeed");
    assert_eq!(
        anf1.stage_hashes.anf_ir_hash, anf2.stage_hashes.anf_ir_hash,
        "anf_ir_hash must be deterministic"
    );
}

// Spec: "Provenance carried through"
// GIVEN a CoreIr node with provenance NodeRef(id)
// WHEN ANF lowering completes
// THEN the corresponding AnfIr node carries that NodeRef in its provenance field
#[test]
fn anf_bindings_carry_correct_source_refs() {
    let graph = ail_testkit::make_semantic_graph(); // NodeRef(0), (1), (2)
    let report = proven_report();
    let core = lower_to_core_ir(&graph, &report).expect("core must succeed");
    let anf = lower_to_anf(&core).expect("anf must succeed");
    // Every AnfBinding source_ref must match the originating NodeRef
    let anf_refs: Vec<NodeRef> = anf.bindings.iter().map(|b| b.source_ref).collect();
    let core_refs: Vec<NodeRef> = core.nodes.iter().map(|n| n.source_ref).collect();
    assert_eq!(
        anf_refs, core_refs,
        "each AnfBinding.source_ref must match the originating CoreNode.source_ref"
    );
}

// Spec: "No node dropped"
// GIVEN a CoreIr with N nodes
// WHEN ANF lowering completes
// THEN every NodeRef from the CoreIr has at least one AnfIr node referencing it
#[test]
fn all_core_node_refs_appear_in_anf() {
    let graph = make_n_node_graph(4);
    let report = proven_report();
    let core = lower_to_core_ir(&graph, &report).expect("core must succeed");
    let anf = lower_to_anf(&core).expect("anf must succeed");
    assert_eq!(
        anf.bindings.len(),
        core.nodes.len(),
        "every CoreNode must produce an AnfBinding"
    );
    for node in &core.nodes {
        let found = anf.bindings.iter().any(|b| b.source_ref == node.source_ref);
        assert!(
            found,
            "NodeRef({}) must appear in AnfIr bindings",
            node.source_ref.0
        );
    }
}

// TRIANGULATE: 1-node CoreIr → 1 AnfBinding with matching source_ref
#[test]
fn one_core_node_produces_one_anf_binding() {
    let graph = make_n_node_graph(1);
    let report = proven_report();
    let core = lower_to_core_ir(&graph, &report).expect("core must succeed");
    let anf = lower_to_anf(&core).expect("anf must succeed");
    assert_eq!(anf.bindings.len(), 1);
    assert_eq!(anf.bindings[0].source_ref, NodeRef(0));
}

// Spec: "Hash includes predecessor"
// GIVEN core_ir_hash H
// WHEN ANF lowering seals its artifact
// THEN anf_ir_hash is produced by hashing H concatenated with the ANF bytes
// (verified indirectly: different core_ir → different anf_ir_hash)
#[test]
fn different_core_ir_produces_different_anf_ir_hash() {
    let graph_a = make_n_node_graph(2);
    let graph_b = make_n_node_graph(3);
    let report = proven_report();
    let core_a = lower_to_core_ir(&graph_a, &report).expect("a must succeed");
    let core_b = lower_to_core_ir(&graph_b, &report).expect("b must succeed");
    let anf_a = lower_to_anf(&core_a).expect("anf a must succeed");
    let anf_b = lower_to_anf(&core_b).expect("anf b must succeed");
    assert_ne!(
        anf_a.stage_hashes.anf_ir_hash, anf_b.stage_hashes.anf_ir_hash,
        "different CoreIr inputs must produce different anf_ir_hash"
    );
}

// ── Fix: graph validation (verify-fixes RED) ─────────────────────────────

// Spec: "Unresolvable / invalid graph is refused"
// GIVEN a SemanticGraph with two nodes sharing the same NodeRef
// WHEN lower_to_core_ir is called
// THEN Err(CompileError::InvalidGraph(_)) is returned
#[test]
fn invalid_graph_duplicate_node_refs_returns_invalid_graph_error() {
    let graph = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Function, "fn_a"),
            GraphNode::new(NodeRef(0), NodeKind::Function, "fn_b"), // duplicate ref
        ],
        edges: vec![],
    };
    let report = proven_report();
    let result = lower_to_core_ir(&graph, &report);
    assert!(
        matches!(result, Err(CompileError::InvalidGraph(_))),
        "duplicate NodeRef must produce InvalidGraph, got {result:?}"
    );
}

// TRIANGULATE: dangling edge endpoint → MissingNode (the NodeRef is absent)
#[test]
fn invalid_graph_dangling_edge_returns_missing_node_error() {
    let graph = SemanticGraph {
        nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Function, "fn_a")],
        edges: vec![GraphEdge {
            source: NodeRef(0),
            target: NodeRef(99), // NodeRef(99) is not in the node list
            kind: EdgeKind::Calls,
        }],
    };
    let report = proven_report();
    let result = lower_to_core_ir(&graph, &report);
    assert!(
        matches!(result, Err(CompileError::MissingNode(NodeRef(99)))),
        "dangling edge target must produce MissingNode(NodeRef(99)), got {result:?}"
    );
}

// ── Fix: explicit hash-chain recomputation (verify-fixes RED) ─────────────

// Spec: "Hash chain continuation — anf_ir_hash = blake3(core_ir_hash || anf_bytes)"
// GIVEN a CoreIr produced from a known graph
// WHEN we recompute blake3(core_ir_hash || cbor(bindings)) manually
// THEN the value equals anf.stage_hashes.anf_ir_hash exactly
#[test]
fn anf_hash_chain_matches_explicit_recomputation() {
    let graph = make_n_node_graph(3);
    let report = proven_report();
    let core = lower_to_core_ir(&graph, &report).expect("core must succeed");
    let anf = lower_to_anf(&core).expect("anf must succeed");

    // Recompute: anf_ir_hash = blake3(core_ir_hash || cbor(anf_bindings))
    let anf_bindings_bytes =
        stable_cbor_bytes(&anf.bindings).expect("stable_cbor_bytes for bindings must succeed");
    let expected_hash = hash_with_parent(&core.stage_hashes.core_ir_hash, &anf_bindings_bytes);

    assert_eq!(
        anf.stage_hashes.anf_ir_hash,
        Some(expected_hash),
        "anf_ir_hash must equal blake3(core_ir_hash || cbor(bindings))"
    );
}
