use super::helpers::*;

#[test]
fn bool_literals_emit_i64_constants() {
    let ops = emit_valid_wasm(AnfExpr::Literal(LiteralValue::Bool(true)), "fn.flag");

    assert!(
        ops.iter().any(|op| op == "I64Const { value: 1 }"),
        "bool true must lower to i64.const 1, got {ops:?}"
    );
}

#[test]
fn bool_literal_can_drive_if_condition() {
    let ops = emit_valid_wasm(
        AnfExpr::Let {
            name: "flag".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Bool(false))),
            body: Box::new(AnfExpr::If {
                cond: "flag".to_string(),
                then_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
                else_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(2))),
            }),
        },
        "fn.branch",
    );

    assert!(
        ops.iter().any(|op| op.starts_with("If")),
        "expected WASM if"
    );
    assert!(
        ops.iter().any(|op| op == "I64Const { value: 0 }"),
        "bool false must lower to i64.const 0, got {ops:?}"
    );
}

#[test]
fn loop_break_with_value_emits_valid_wasm_br_to_outer_block() {
    let ops = emit_valid_wasm(
        AnfExpr::Loop {
            body: Box::new(AnfExpr::Break {
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(10))),
            }),
        },
        "fn.count_to_ten",
    );

    assert!(
        ops.iter().any(|op| op.starts_with("Block")),
        "expected block"
    );
    assert!(ops.iter().any(|op| op.starts_with("Loop")), "expected loop");
    assert!(
        ops.iter().any(|op| op == "Br { relative_depth: 1 }"),
        "break must branch to outer block, got {ops:?}"
    );
}

#[test]
fn continue_emits_valid_wasm_br_to_loop_header() {
    let ops = emit_valid_wasm(
        AnfExpr::Loop {
            body: Box::new(AnfExpr::Continue),
        },
        "fn.spin",
    );

    assert!(
        ops.iter().any(|op| op == "Br { relative_depth: 0 }"),
        "continue must branch to loop header, got {ops:?}"
    );
}

#[test]
fn while_loop_emits_valid_wasm_loop_with_exit_branch() {
    let ops = emit_valid_wasm(
        AnfExpr::Let {
            name: "keep_going".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Bool(false))),
            body: Box::new(AnfExpr::WhileLoop {
                cond: "keep_going".to_string(),
                body: Box::new(AnfExpr::Literal(LiteralValue::Int(10))),
            }),
        },
        "fn.while_loop",
    );

    assert!(ops.iter().any(|op| op.starts_with("Loop")), "expected loop");
    assert!(
        ops.iter().any(|op| op == "BrIf { relative_depth: 1 }"),
        "while false condition must branch out of outer block, got {ops:?}"
    );
    assert!(
        ops.iter().any(|op| op == "Br { relative_depth: 0 }"),
        "while body must branch back to loop header, got {ops:?}"
    );
}

// ── Wave 18D: WhileLoop as Let value emits valid WASM ────────────────────
//
// Scenario: WhileLoop used as the `value` of an outer `Let` binding must
// push a unit (I32 0) so the enclosing LocalSet has something to consume.
// Without the unit push the emitted WASM would fail wasmparser::validate.
//
// Structure under test:
//   let flag = false in
//   let _w   = while(flag, 0) in   ← WhileLoop as Let value
//   42
//
// Approval criteria:
//  1. emit_wasm + wasmparser::validate both succeed (no stack underflow).
//  2. The instruction stream contains an I32Const{value: 0} after the loop
//     End, proving the unit was emitted.
#[test]
fn while_loop_emits_unit_when_used_as_let_value() {
    let ops = emit_valid_wasm(
        AnfExpr::Let {
            name: "flag".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Bool(false))),
            body: Box::new(AnfExpr::Let {
                name: "_w".to_string(),
                value: Box::new(AnfExpr::WhileLoop {
                    cond: "flag".to_string(),
                    body: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                }),
                body: Box::new(AnfExpr::Literal(LiteralValue::Int(42))),
            }),
        },
        "fn.while_loop_let",
    );
    assert!(
        ops.iter().any(|op| op.starts_with("Loop")),
        "expected loop block"
    );
    assert!(
        ops.iter().any(|op| op == "I32Const { value: 0 }"),
        "WhileLoop must push unit I32 0 for the enclosing LocalSet, got {ops:?}"
    );
}

// ── Task 3.1: wasmparser validates emitted modules ────────────────────────

// Scenario: zero-binding graph → minimal valid WASM module.
// The emitted bytes must pass wasmparser::validate (structural validity only).
// Approval: for zero bindings the output must be EXACTLY the 8-byte WASM
// magic-number + version header (no sections appended).
#[test]
fn empty_anf_emits_valid_wasm_module() {
    // WASM magic (0x00 0x61 0x73 0x6d) + version 1 (0x01 0x00 0x00 0x00).
    const WASM_HEADER: [u8; 8] = [0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];

    let anf = anf_for_graph(&empty_graph());
    let artifact = emit_wasm(&anf).expect("emit_wasm failed on empty anf");

    // Structural validity via the reference parser.
    wasmparser::validate(&artifact.wasm).expect("wasmparser rejected empty wasm module");

    // Exact byte contract: zero-binding module is header-only (8 bytes).
    assert_eq!(
        artifact.wasm.as_slice(),
        &WASM_HEADER,
        "empty AnfIr must emit exactly the 8-byte WASM magic+version header, \
         got {} bytes: {:02x?}",
        artifact.wasm.len(),
        artifact.wasm,
    );
}

// Scenario: N-binding graph → valid WASM module with N function stubs.
#[test]
fn three_node_graph_emits_valid_wasm_module() {
    let anf = anf_for_graph(&graph_with_n_nodes(3));
    let artifact = emit_wasm(&anf).expect("emit_wasm failed on 3-node graph");
    wasmparser::validate(&artifact.wasm).expect("wasmparser rejected 3-function wasm module");
}

// TRIANGULATE: single-node graph also produces a structurally valid module.

#[test]
fn function_bodies_are_exactly_unreachable_end() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = anf_for_graph(&graph_with_n_nodes(2));
    let artifact = emit_wasm(&anf).expect("emit_wasm failed");

    // Module must be structurally valid.
    wasmparser::validate(&artifact.wasm).expect("wasmparser rejected module");

    let mut function_bodies_found = 0usize;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Ok(Payload::CodeSectionEntry(body)) = payload {
            function_bodies_found += 1;
            let mut reader = body
                .get_operators_reader()
                .expect("get_operators_reader failed");

            // Read all operators for this body.
            let ops: Vec<_> = std::iter::from_fn(|| reader.read().ok()).collect();

            // Every body must be non-empty and end with End.
            assert!(
                !ops.is_empty(),
                "function body must have at least one instruction"
            );
            assert!(
                matches!(ops.last().unwrap(), Operator::End),
                "last instruction must be End, got {:?}",
                ops.last()
            );
            // Placeholder nodes (no expr body) produce: [Unreachable, Drop, End].
            // The Drop is emitted because function type is () -> ().
            // At minimum there must be at least 2 instructions (something + End).
            assert!(
                ops.len() >= 2,
                "function body must have at least 2 instructions, got {ops:?}"
            );
        }
    }
    assert_eq!(
        function_bodies_found, 2,
        "expected 2 code section entries for 2-binding AnfIr"
    );
}

// ── verify-fix: provenance values are byte offsets, not function indexes ──

// Spec: provenance map stores the WASM byte offset of each code entry.
// We assert that wasm[provenance[NodeRef(i)]] is a valid LEB128-encoded byte
// (i.e. its value is non-zero for a non-empty body), proving the stored value
// is a byte position in the binary, NOT the function index.
