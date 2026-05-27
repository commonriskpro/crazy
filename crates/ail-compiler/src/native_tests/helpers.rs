pub(super) use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
pub(super) use ail_verify::report::VerificationReport;

pub(super) use super::super::*;
pub(super) use crate::core_ir::StageHashes;
pub(super) use crate::lower::{lower_to_anf, lower_to_core_ir};

pub(super) fn proven_report() -> VerificationReport {
    VerificationReport {
        entries: vec![],
        ..Default::default()
    }
}

pub(super) fn anf_for_n(n: usize) -> AnfIr {
    let graph = SemanticGraph {
        nodes: (0..n)
            .map(|i| GraphNode::new(NodeRef(i as u32), NodeKind::Function, format!("fn_{i}")))
            .collect(),
        edges: vec![],
    };
    let core = lower_to_core_ir(&graph, &proven_report()).unwrap();
    lower_to_anf(&core).unwrap()
}

pub(super) fn anf_with_call2(func: &str, lhs: i64, rhs: i64) -> AnfIr {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "x".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(lhs))),
            body: Box::new(AnfExpr::Let {
                name: "y".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(rhs))),
                body: Box::new(AnfExpr::Call {
                    func: func.to_string(),
                    args: vec!["x".to_string(), "y".to_string()],
                }),
            }),
        },
    })
}

pub(super) fn anf_with_call1(func: &str, operand: i64) -> AnfIr {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "x".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(operand))),
            body: Box::new(AnfExpr::Call {
                func: func.to_string(),
                args: vec!["x".to_string()],
            }),
        },
    })
}

pub(super) fn placeholder_anf() -> AnfIr {
    use crate::anf::{AnfBinding, AnfExpr};
    anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Placeholder,
    })
}

pub(super) fn anf_with_if(cond_val: bool, then_val: i64, else_val: i64) -> AnfIr {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "c".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Bool(cond_val))),
            body: Box::new(AnfExpr::If {
                cond: "c".to_string(),
                then_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(then_val))),
                else_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(else_val))),
            }),
        },
    })
}

pub(super) fn anf_for_binding(binding: crate::anf::AnfBinding) -> AnfIr {
    use crate::anf::SourceMap;
    AnfIr {
        schema_version: crate::anf::ANF_SCHEMA_VERSION,
        source_map: SourceMap::from_bindings(std::slice::from_ref(&binding)),
        bindings: vec![binding],
        stage_hashes: StageHashes {
            graph_snapshot_hash: [0u8; 32],
            verification_report_hash: [0u8; 32],
            core_ir_hash: [1u8; 32],
            anf_ir_hash: Some([2u8; 32]),
            wasm_hash: None,
            native_hash: None,
            source_map_hash: None,
            artifact_manifest_hash: None,
        },
    }
}

// Helper: emit native for a single Int literal binding with a FIXED name
// so that two calls with different values produce identical symbol tables
// and any byte difference is purely from code content.
pub(super) fn anf_with_int_literal(n: i64) -> AnfIr {
    use crate::anf::AnfBinding;
    use crate::core_ir::LiteralValue;
    anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_lit".to_string(), // fixed name — code difference is the only variable
        expr: crate::anf::AnfExpr::Literal(LiteralValue::Int(n)),
    })
}

pub(super) fn anf_with_record(fields: Vec<(&str, i64)>) -> AnfIr {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let field_exprs: Vec<(String, AnfExpr)> = fields
        .into_iter()
        .map(|(f, v)| (f.to_string(), AnfExpr::Literal(LiteralValue::Int(v))))
        .collect();
    anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::RecordNew {
            fields: field_exprs,
        },
    })
}

pub(super) fn anf_lambda_no_captures(body_val: i64) -> AnfIr {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Lambda {
            params: vec!["p".to_string()],
            captures: vec![],
            body: Box::new(AnfExpr::Literal(LiteralValue::Int(body_val))),
        },
    })
}

pub(super) fn anf_lambda_one_capture(cap_val: i64) -> AnfIr {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    // let x = cap_val in (lambda captures=[x] params=[p] body=Var("p"))
    // body=Var("p") keeps the inner function compilable; the closure env
    // carries x's value by value via the outer ctx.
    anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "x".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(cap_val))),
            body: Box::new(AnfExpr::Lambda {
                params: vec!["p".to_string()],
                captures: vec!["x".to_string()],
                body: Box::new(AnfExpr::Var("p".to_string())),
            }),
        },
    })
}

pub(super) fn anf_lambda_returning_param_body(body: crate::anf::AnfExpr) -> AnfIr {
    use crate::anf::{AnfBinding, AnfExpr};
    anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Lambda {
            params: vec!["p".to_string()],
            captures: vec![],
            body: Box::new(body),
        },
    })
}

pub(super) fn anf_with_bytes(data: Vec<u8>) -> AnfIr {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Literal(LiteralValue::Bytes(data)),
    })
}
