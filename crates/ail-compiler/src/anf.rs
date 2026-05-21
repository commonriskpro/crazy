// ── ail-compiler::anf ─────────────────────────────────────────────────────
//
// ANF (Administrative Normal Form) IR value types — the second lowering
// stage output.
//
// # Design constraints
//
// - `Vec` only (no `HashMap`) — workspace determinism contract.
// - All types `#[derive(Serialize)]` for CBOR hash sealing.
// - Every `AnfBinding` carries a `source_ref: NodeRef` that traces back to
//   the original `SemanticGraph` node; this provenance must survive lowering.
//
// # G3 scope (anf-real)
//
// Promotes `AnfBinding` from a flat placeholder to a real ANF IR node.
//
// `AnfExpr` mirrors `CoreExpr` but enforces A-Normal Form: every intermediate
// result is named.  Specifically:
//   - `Call.args` and `FieldGet.record` and `If.cond` are atomic names
//     (`String`), never nested expressions.
//   - Nested sub-expressions are let-bound before use (handled in lowering).
//
// `AnfBinding.expr` holds the normalised expression for this binding.
// Nodes without a `CoreExpr` body default to `AnfExpr::Literal(Unit)`.

use ail_core::semantic_graph::NodeRef;
use serde::{Deserialize, Serialize};

use crate::core_ir::{LiteralValue, StageHashes};

// ── AnfExpr ───────────────────────────────────────────────────────────────

/// A-Normal Form expression — all intermediate values are let-bound.
///
/// Corresponds to the ANF IR layer described in `docs/core-ir.md`:
/// > ANF IR: compiler IR principal; orden explícito de efectos.
///
/// Key ANF invariant: call arguments, field-access records, and if-conditions
/// are ALWAYS atomic (variable names), never nested expressions.  The lowering
/// stage ensures this by introducing fresh let-bindings for any non-atomic
/// sub-expression.
///
/// All variants must be serializable for CBOR determinism.  Recursive
/// sub-expressions use `Box<AnfExpr>`.  Collections use `Vec` (never
/// `HashMap`) per the workspace determinism contract.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AnfExpr {
    /// A typed constant — already atomic.
    Literal(LiteralValue),
    /// A reference to a local variable by name — atomic.
    Var(String),
    /// An immutable let-binding: `let <name> = <value> in <body>`.
    ///
    /// Used both for user-written lets and for synthetic temporaries
    /// introduced during ANF flattening of nested expressions.
    Let {
        name: String,
        value: Box<AnfExpr>,
        body: Box<AnfExpr>,
    },
    /// A boolean branch.
    ///
    /// `cond` is an atomic variable name — guaranteed by the lowering pass.
    If {
        cond: String,
        then_branch: Box<AnfExpr>,
        else_branch: Box<AnfExpr>,
    },
    /// A function call.
    ///
    /// `args` are atomic variable names — guaranteed by the lowering pass.
    Call { func: String, args: Vec<String> },
    /// Read one field from a named record.
    ///
    /// `record` is an atomic variable name — guaranteed by the lowering pass.
    FieldGet { record: String, field: String },
    /// Explicit return — wraps the return expression.
    Return(Box<AnfExpr>),
    /// Effect-ordered sequence of expressions.
    ///
    /// Used for sequential effect calls where the individual results are
    /// discarded (or each step produces a unit).
    Seq(Vec<AnfExpr>),
    /// Placeholder for `CoreExpr` variants not yet lowered to ANF.
    ///
    /// Represents unhandled variants (Match, Lambda, RecordNew, etc.).
    /// Backends treat this as a `trap`/`unreachable` stub.
    Placeholder,
}

// Manual Eq impl: required because `LiteralValue::Float` contains `f64`,
// which does not implement `Eq`.  We compare floats by bit pattern (NaN ==
// NaN for IR identity purposes — same bit pattern = same literal).
impl Eq for AnfExpr {}

// ── AnfBinding ────────────────────────────────────────────────────────────

/// One binding in the ANF IR — lowered from a `CoreNode`.
///
/// `source_ref` is the provenance chain back to the originating
/// `SemanticGraph` node.  It MUST equal the `CoreNode::source_ref` that
/// this binding was produced from.
///
/// `expr` holds the ANF expression for this binding.  Nodes without a
/// `CoreExpr` body default to `AnfExpr::Literal(LiteralValue::Unit)`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnfBinding {
    /// Original `NodeRef` from the `SemanticGraph` — preserved through
    /// Core IR and into ANF for full end-to-end provenance.
    pub source_ref: NodeRef,
    /// Binding name, copied from the `CoreNode`.
    pub name: String,
    /// ANF expression body for this binding.
    ///
    /// For top-level definitions without an expression body (modules, types,
    /// capabilities, etc.), this defaults to `AnfExpr::Literal(LiteralValue::Unit)`.
    pub expr: AnfExpr,
}

// ── AnfIr ─────────────────────────────────────────────────────────────────

/// Output of the second pipeline stage: a flat list of ANF bindings with
/// full provenance and an extended hash chain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnfIr {
    /// ANF bindings in source traversal order.
    ///
    /// May contain more bindings than the originating `CoreIr.nodes` because
    /// the ANF flattening pass introduces synthetic let-bindings for nested
    /// sub-expressions.
    pub bindings: Vec<AnfBinding>,
    /// Hash chain extended through the ANF stage.
    /// `stage_hashes.anf_ir_hash` is `Some(...)` after this stage completes.
    pub stage_hashes: StageHashes,
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core_ir::{LiteralValue, StageHashes};
    use crate::hash::stable_cbor_bytes;
    #[allow(unused_imports)]
    use ciborium;

    // ── AnfExpr construction ──────────────────────────────────────────────

    // All AnfExpr variants are constructible without panic.
    #[test]
    fn all_anf_expr_variants_are_constructible() {
        let _lit = AnfExpr::Literal(LiteralValue::Int(42));
        let _var = AnfExpr::Var("x".to_string());
        let _let = AnfExpr::Let {
            name: "y".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
            body: Box::new(AnfExpr::Var("y".to_string())),
        };
        let _if = AnfExpr::If {
            cond: "flag".to_string(),
            then_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            else_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
        };
        let _call = AnfExpr::Call {
            func: "fn.add".to_string(),
            args: vec!["a".to_string(), "b".to_string()],
        };
        let _fg = AnfExpr::FieldGet {
            record: "order".to_string(),
            field: "total".to_string(),
        };
        let _ret = AnfExpr::Return(Box::new(AnfExpr::Var("result".to_string())));
        let _seq = AnfExpr::Seq(vec![
            AnfExpr::Call {
                func: "db.write".to_string(),
                args: vec!["order".to_string()],
            },
            AnfExpr::Literal(LiteralValue::Unit),
        ]);
        let _placeholder = AnfExpr::Placeholder;
    }

    // If.cond is a String (atomic), not a nested AnfExpr.
    #[test]
    fn anf_if_cond_is_atomic_string() {
        let expr = AnfExpr::If {
            cond: "my_flag".to_string(),
            then_branch: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
            else_branch: Box::new(AnfExpr::Literal(LiteralValue::Bool(false))),
        };
        if let AnfExpr::If { cond, .. } = expr {
            assert_eq!(cond, "my_flag");
        } else {
            panic!("expected If variant");
        }
    }

    // Call.args are Vec<String> (atomic names), not nested expressions.
    #[test]
    fn anf_call_args_are_atomic_strings() {
        let expr = AnfExpr::Call {
            func: "fn.checkout".to_string(),
            args: vec!["cart_id".to_string(), "user_id".to_string()],
        };
        if let AnfExpr::Call { func, args } = expr {
            assert_eq!(func, "fn.checkout");
            assert_eq!(args, vec!["cart_id", "user_id"]);
        } else {
            panic!("expected Call variant");
        }
    }

    // ── AnfBinding ────────────────────────────────────────────────────────

    // Scenario: AnfBinding preserves its source_ref provenance.
    // Spec: "every AnfBinding.source_ref matches origin NodeRef"
    #[test]
    fn anf_binding_preserves_source_ref() {
        let binding = AnfBinding {
            source_ref: NodeRef(7),
            name: "fn_x".to_string(),
            expr: AnfExpr::Literal(LiteralValue::Unit),
        };
        assert_eq!(
            binding.source_ref,
            NodeRef(7),
            "source_ref must be preserved verbatim"
        );
    }

    // Scenario: AnfBinding with Let expr is constructible.
    #[test]
    fn anf_binding_with_let_expr() {
        let binding = AnfBinding {
            source_ref: NodeRef(5),
            name: "fn_checkout".to_string(),
            expr: AnfExpr::Let {
                name: "cart".to_string(),
                value: Box::new(AnfExpr::Call {
                    func: "db.read".to_string(),
                    args: vec!["cart_id".to_string()],
                }),
                body: Box::new(AnfExpr::Var("cart".to_string())),
            },
        };
        assert_eq!(binding.source_ref, NodeRef(5));
        assert_eq!(binding.name, "fn_checkout");
    }

    // Scenario: AnfIr is constructible with bindings and stage hashes.
    #[test]
    fn anf_ir_is_constructible() {
        let ir = AnfIr {
            bindings: vec![
                AnfBinding {
                    source_ref: NodeRef(0),
                    name: "mod_root".to_string(),
                    expr: AnfExpr::Literal(LiteralValue::Unit),
                },
                AnfBinding {
                    source_ref: NodeRef(1),
                    name: "fn_main".to_string(),
                    expr: AnfExpr::Placeholder,
                },
            ],
            stage_hashes: StageHashes {
                graph_snapshot_hash: [0u8; 32],
                verification_report_hash: [0u8; 32],
                core_ir_hash: [1u8; 32],
                anf_ir_hash: Some([2u8; 32]),
                wasm_hash: None,
                native_hash: None,
            },
        };
        assert_eq!(ir.bindings.len(), 2);
        assert!(ir.stage_hashes.anf_ir_hash.is_some());
    }

    // TRIANGULATE: stable_cbor_bytes on Vec<AnfBinding> is deterministic.
    #[test]
    fn anf_binding_list_cbor_is_deterministic() {
        let bindings = vec![
            AnfBinding {
                source_ref: NodeRef(0),
                name: "a".to_string(),
                expr: AnfExpr::Literal(LiteralValue::Int(1)),
            },
            AnfBinding {
                source_ref: NodeRef(1),
                name: "b".to_string(),
                expr: AnfExpr::Var("a".to_string()),
            },
            AnfBinding {
                source_ref: NodeRef(2),
                name: "c".to_string(),
                expr: AnfExpr::Placeholder,
            },
        ];
        let b1 = stable_cbor_bytes(&bindings).expect("first encode");
        let b2 = stable_cbor_bytes(&bindings).expect("second encode");
        assert_eq!(b1, b2, "Vec<AnfBinding> must produce identical CBOR bytes");
    }

    // TRIANGULATE: different binding lists produce different CBOR bytes.
    #[test]
    fn different_anf_binding_lists_produce_different_cbor() {
        let list_a = vec![AnfBinding {
            source_ref: NodeRef(0),
            name: "x".to_string(),
            expr: AnfExpr::Literal(LiteralValue::Int(1)),
        }];
        let list_b = vec![AnfBinding {
            source_ref: NodeRef(0),
            name: "x".to_string(),
            expr: AnfExpr::Literal(LiteralValue::Int(2)),
        }];
        let b_a = stable_cbor_bytes(&list_a).expect("encode a");
        let b_b = stable_cbor_bytes(&list_b).expect("encode b");
        assert_ne!(
            b_a, b_b,
            "different AnfBinding lists must produce different CBOR"
        );
    }

    // Scenario: source_ref is not dropped when name is the same.
    #[test]
    fn anf_binding_distinct_refs_are_not_equal() {
        let b1 = AnfBinding {
            source_ref: NodeRef(3),
            name: "shared_name".to_string(),
            expr: AnfExpr::Placeholder,
        };
        let b2 = AnfBinding {
            source_ref: NodeRef(4),
            name: "shared_name".to_string(),
            expr: AnfExpr::Placeholder,
        };
        assert_ne!(b1, b2, "bindings with different NodeRefs must not be equal");
    }

    // S11: CBOR round-trip for AnfBinding with Let expr is lossless.
    #[test]
    fn anf_binding_cbor_round_trip_with_let_expr() {
        let binding = AnfBinding {
            source_ref: NodeRef(9),
            name: "fn_round_trip".to_string(),
            expr: AnfExpr::Let {
                name: "tmp".to_string(),
                value: Box::new(AnfExpr::Call {
                    func: "fn.add".to_string(),
                    args: vec!["x".to_string(), "y".to_string()],
                }),
                body: Box::new(AnfExpr::Var("tmp".to_string())),
            },
        };
        let bytes = stable_cbor_bytes(&binding).expect("encode must succeed");
        let decoded: AnfBinding =
            ciborium::from_reader(bytes.as_slice()).expect("decode must succeed");
        assert_eq!(
            decoded, binding,
            "AnfBinding with Let expr must survive CBOR round-trip"
        );
    }
}
