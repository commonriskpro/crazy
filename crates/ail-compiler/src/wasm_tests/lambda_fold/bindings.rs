use super::*;

// Scenario: emit_wasm succeeds for a top-level Lambda binding with no captures.
// Proves the pipeline is end-to-end correct for the no-capture case.
#[test]
fn emit_wasm_lambda_binding_no_capture_succeeds() {
    use ail_core::semantic_graph::NodeRef;

    // fn(x) -> x  (identity Lambda, no captures)
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "id".to_string(),
        expr: AnfExpr::Lambda {
            params: vec!["x".to_string()],
            captures: vec![],
            body: Box::new(AnfExpr::Var("x".to_string())),
        },
    };
    let anf = sealed_anf(vec![binding]);
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for identity Lambda");
    assert!(
        !artifact.wasm.is_empty(),
        "WASM binary must be non-empty for Lambda binding"
    );
    assert!(
        artifact.hash_chain.wasm_hash.is_some(),
        "wasm_hash must be sealed after emit_wasm"
    );
}

// Scenario: emit_wasm succeeds for a top-level Lambda binding with one capture.
// Proves the pipeline handles captures as additional WASM function params.
#[test]
fn emit_wasm_lambda_binding_with_capture_succeeds() {
    use ail_core::semantic_graph::NodeRef;

    // fn(x) -> add(outer, x)  — outer is a captured variable
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "add_to_outer".to_string(),
        expr: AnfExpr::Lambda {
            params: vec!["x".to_string()],
            captures: vec!["outer".to_string()],
            body: Box::new(AnfExpr::Call {
                func: "+".to_string(),
                args: vec!["outer".to_string(), "x".to_string()],
            }),
        },
    };
    let anf = sealed_anf(vec![binding]);
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for Lambda with captures");
    assert!(
        !artifact.wasm.is_empty(),
        "WASM binary must be non-empty for Lambda binding with captures"
    );
    // The function is exported because binding_result returns Some(I64).
    assert!(
        artifact.export_types.contains_key("add_to_outer"),
        "Lambda binding with I64 body must appear in export_types; got: {:?}",
        artifact.export_types.keys().collect::<Vec<_>>()
    );
}

// Scenario: a binding whose body contains a nested Lambda with captures emits
// a closure env in linear memory.  The WASM module must include memory and the
// global bump-allocator section required by emit_alloc.
//
// The test verifies structural properties: emit_wasm succeeds, the binary
// contains a memory section (needs_memory = true due to captures), and the
// hash is sealed.
#[test]
fn emit_wasm_nested_lambda_with_captures_allocates_memory() {
    use ail_core::semantic_graph::NodeRef;

    // let result = (fn(x) { x + outer })  — nested Lambda in a Let body
    // The outer binding does not itself have params; it wraps a nested Lambda.
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "make_closure".to_string(),
        expr: AnfExpr::Let {
            name: "closure".to_string(),
            value: Box::new(AnfExpr::Lambda {
                params: vec!["x".to_string()],
                captures: vec!["outer".to_string()],
                body: Box::new(AnfExpr::Call {
                    func: "+".to_string(),
                    args: vec!["outer".to_string(), "x".to_string()],
                }),
            }),
            body: Box::new(AnfExpr::Var("closure".to_string())),
        },
    };
    let anf = sealed_anf(vec![binding]);
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for binding with nested Lambda");
    assert!(!artifact.wasm.is_empty(), "WASM binary must be non-empty");
    // A memory section must be present (confirmed by the global bump-allocator
    // section, which is only emitted when needs_memory = true).  We verify
    // indirectly: the WASM binary must be larger than a module with no memory.
    let no_mem_anf = sealed_anf(vec![AnfBinding {
        source_ref: ail_core::semantic_graph::NodeRef(1),
        name: "lit".to_string(),
        expr: AnfExpr::Literal(LiteralValue::Int(42)),
    }]);
    let no_mem_artifact = emit_wasm(&no_mem_anf).unwrap();
    assert!(
        artifact.wasm.len() > no_mem_artifact.wasm.len(),
        "module with nested Lambda + captures must be larger than a literal-only module \
         (memory + global sections are required for the closure env)"
    );
}

// TRIANGULATE: two Lambda bindings with different capture counts produce different
// WASM hashes (cap_count field in closure env header changes the binary).
#[test]
fn lambda_bindings_with_different_capture_counts_produce_different_hashes() {
    use ail_core::semantic_graph::NodeRef;

    let make_lambda = |captures: Vec<String>| {
        sealed_anf(vec![AnfBinding {
            source_ref: NodeRef(0),
            name: "f".to_string(),
            expr: AnfExpr::Lambda {
                params: vec!["x".to_string()],
                captures,
                body: Box::new(AnfExpr::Var("x".to_string())),
            },
        }])
    };

    let a = emit_wasm(&make_lambda(vec![])).unwrap();
    let b = emit_wasm(&make_lambda(vec!["outer".to_string()])).unwrap();
    assert_ne!(
        a.hash_chain.wasm_hash, b.hash_chain.wasm_hash,
        "Lambda with captures must produce a different wasm_hash than one without"
    );
}

// ── End WASM closure capture tests ───────────────────────────────────────

// ── Wave 7C: CellNew / CellGet / CellSet / MapNew / SetNew / IndexGet ─────
//
// Proves that the six collection/cell primitives no longer emit unconditional
// Unreachable and instead produce valid, executable WASM that uses linear
// memory correctly.

// Scenario: CellNew allocates 8 bytes and stores the initial value.
// Expects: memory section present, I64Store emitted, WASM validates.

