use super::*;

// ── lower_to_core_ir ──────────────────────────────────────────────────

#[test]
fn rejected_report_returns_rejected_error() {
    let graph = one_node_graph();
    let report = report_with_state(VerificationState::Failed);
    assert_eq!(
        lower_to_core_ir(&graph, &report),
        Err(CompileError::RejectedReport)
    );
}

#[test]
fn accepted_report_returns_core_ir() {
    let graph = one_node_graph();
    let report = proven_report();
    let result = lower_to_core_ir(&graph, &report);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(result.unwrap().nodes.len(), 1);
}

// ── lower_to_anf ─────────────────────────────────────────────────────

#[test]
fn lower_to_anf_produces_one_binding_per_core_node() {
    let graph = one_node_graph();
    let report = proven_report();
    let core = lower_to_core_ir(&graph, &report).unwrap();
    let anf = lower_to_anf(&core).unwrap();
    assert_eq!(anf.bindings.len(), 1);
    assert_eq!(anf.bindings[0].source_ref, NodeRef(0));
}

#[test]
fn anf_ir_hash_is_set_after_lowering() {
    let graph = one_node_graph();
    let report = proven_report();
    let core = lower_to_core_ir(&graph, &report).unwrap();
    let anf = lower_to_anf(&core).unwrap();
    assert!(anf.stage_hashes.anf_ir_hash.is_some());
}
