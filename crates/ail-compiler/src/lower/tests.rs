use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_verify::report::{VerificationEntry, VerificationReport, VerificationState};

use super::*;

// ── Helpers ───────────────────────────────────────────────────────────

fn report_with_state(state: VerificationState) -> VerificationReport {
    VerificationReport {
        entries: vec![VerificationEntry {
            claim: "claim".to_string(),
            state,
            scope: "s".to_string(),
            evidence: None,
            blocking: matches!(state, VerificationState::Failed | VerificationState::Unsafe),
            repair_options: vec![],
        }],
        ..Default::default()
    }
}

fn proven_report() -> VerificationReport {
    VerificationReport {
        entries: vec![],
        ..Default::default()
    }
}

fn one_node_graph() -> SemanticGraph {
    SemanticGraph {
        nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Module, "m")],
        edges: vec![],
    }
}

fn lower_single(
    expr: &crate::core_ir::CoreExpr,
) -> (crate::anf::AnfExpr, Vec<crate::anf::AnfBinding>) {
    let mut fresh = 0u32;
    let mut out: Vec<crate::anf::AnfBinding> = Vec::new();
    let result = lower_core_expr_to_anf(expr, &mut fresh, NodeRef(0), &mut out);
    (result, out)
}

mod concurrency;
mod core_pipeline;
mod expression_shapes;
mod nominal_types;
mod reports;
