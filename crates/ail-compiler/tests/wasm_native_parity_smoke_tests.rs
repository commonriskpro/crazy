// ── ail-compiler::wasm_native_parity_smoke_tests ─────────────────────────
//
// WASM / native backend parity smoke.
//
// # Purpose
//
// These tests verify that both backend paths (`emit_wasm` and `emit_native`)
// produce equivalent structural artifacts for the same `AnfIr` input:
//
// - Both succeed (neither panics or errors).
// - Both produce non-empty binary output.
// - Both produce provenance maps covering the same set of NodeRefs.
// - Both produce source maps with the same number of entries.
// - Both seal their respective backend hashes (`wasm_hash` / `native_hash`).
// - Neither backend's hash chain leaks into the other's output.
//
// # What parity means (and does not mean)
//
// Parity here is STRUCTURAL, not semantic.  We do not execute the generated
// artifacts and compare runtime outputs.  Full semantic equivalence verification
// is a future "translation validation" deliverable (see docs/compiler.md —
// "Long-term option: translation validation").
//
// Structural parity is sufficient to prove:
//   1. The same ANF IR is accepted by both backends without error.
//   2. Both backends track provenance for every binding.
//   3. The hash chain is extended independently per backend (no cross-contamination).
//
// # Scope
//
// Simple expressions only: integer literals, bool literals, arithmetic ops
// (`i64.add`), and an If branch.  These are the expressions for which both
// backends currently lower to real code (not trap stubs).

use ail_compiler::{
    AnfBinding, AnfExpr, AnfIr, SourceMap,
    anf::ANF_SCHEMA_VERSION,
    core_ir::{LiteralValue, StageHashes},
    emit_native, emit_wasm,
    lower::{lower_to_anf, lower_to_core_ir},
};
use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_verify::report::VerificationReport;

// ── helpers ──────────────────────────────────────────────────────────────

fn proven_report() -> VerificationReport {
    VerificationReport {
        entries: vec![],
        ..Default::default()
    }
}

fn graph_with_n_functions(n: usize) -> SemanticGraph {
    SemanticGraph {
        nodes: (0..n)
            .map(|i| GraphNode::new(NodeRef(i as u32), NodeKind::Function, format!("fn_{i}")))
            .collect(),
        edges: vec![],
    }
}

fn anf_for_n(n: usize) -> AnfIr {
    let graph = graph_with_n_functions(n);
    let core = lower_to_core_ir(&graph, &proven_report()).expect("lower_to_core_ir");
    lower_to_anf(&core).expect("lower_to_anf")
}

fn sealed_anf_single(binding: AnfBinding) -> AnfIr {
    AnfIr {
        schema_version: ANF_SCHEMA_VERSION,
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

// ── Parity: both backends accept the same AnfIr ───────────────────────────

/// Parity smoke: 1-binding ANF succeeds in both WASM and native backends.
#[test]
fn parity_smoke_one_binding_anf_succeeds_in_both_backends() {
    let anf = anf_for_n(1);
    let wasm = emit_wasm(&anf).expect("emit_wasm");
    let native = emit_native(&anf).expect("emit_native");

    assert!(
        !wasm.wasm.is_empty(),
        "parity smoke: WASM output must be non-empty"
    );
    assert!(
        !native.native_bytes.is_empty(),
        "parity smoke: native output must be non-empty"
    );
}

/// Parity smoke: 3-binding ANF succeeds in both backends.
#[test]
fn parity_smoke_three_binding_anf_succeeds_in_both_backends() {
    let anf = anf_for_n(3);
    assert!(
        emit_wasm(&anf).is_ok(),
        "parity smoke: emit_wasm must succeed for 3 bindings"
    );
    assert!(
        emit_native(&anf).is_ok(),
        "parity smoke: emit_native must succeed for 3 bindings"
    );
}

/// Parity smoke: empty ANF succeeds in both backends.
#[test]
fn parity_smoke_empty_anf_succeeds_in_both_backends() {
    let anf = anf_for_n(0);
    let wasm = emit_wasm(&anf).expect("emit_wasm");
    let native = emit_native(&anf).expect("emit_native");

    // Both produce at minimum a valid module/object header.
    assert!(
        !wasm.wasm.is_empty(),
        "parity smoke: WASM must emit non-empty module header"
    );
    assert!(
        !native.native_bytes.is_empty(),
        "parity smoke: native must emit non-empty object header"
    );
}

// ── Parity: provenance map covers the same NodeRefs ───────────────────────

/// Parity smoke: both backends produce provenance maps covering the same NodeRefs.
///
/// The WASM provenance map (BTreeMap<NodeRef, u32>) and the native provenance
/// map (BTreeMap<NodeRef, u64>) must cover exactly the same set of NodeRef keys.
#[test]
fn parity_smoke_provenance_maps_cover_same_node_refs() {
    let n = 4;
    let anf = anf_for_n(n);
    let wasm = emit_wasm(&anf).expect("emit_wasm");
    let native = emit_native(&anf).expect("emit_native");

    let wasm_refs: Vec<NodeRef> = wasm.provenance.keys().copied().collect();
    let native_refs: Vec<NodeRef> = native.provenance.keys().copied().collect();

    assert_eq!(
        wasm_refs, native_refs,
        "parity smoke: WASM and native provenance maps must cover the same NodeRefs"
    );
    assert_eq!(
        wasm_refs.len(),
        n,
        "parity smoke: provenance must have {n} entries"
    );
}

/// Parity smoke: empty ANF produces empty provenance maps in both backends.
#[test]
fn parity_smoke_empty_anf_produces_empty_provenance_maps_in_both() {
    let anf = anf_for_n(0);
    let wasm = emit_wasm(&anf).expect("emit_wasm");
    let native = emit_native(&anf).expect("emit_native");

    assert!(
        wasm.provenance.is_empty(),
        "parity smoke: WASM provenance must be empty for zero-binding ANF"
    );
    assert!(
        native.provenance.is_empty(),
        "parity smoke: native provenance must be empty for zero-binding ANF"
    );
}

// ── Parity: source map entry count matches bindings ───────────────────────

/// Parity smoke: WASM and native source maps have the same entry count.
#[test]
fn parity_smoke_source_map_entry_count_matches_in_both_backends() {
    let n = 3;
    let anf = anf_for_n(n);
    let wasm = emit_wasm(&anf).expect("emit_wasm");
    let native = emit_native(&anf).expect("emit_native");

    assert_eq!(
        wasm.source_map.entries.len(),
        n,
        "parity smoke: WASM source map must have {n} entries"
    );
    assert_eq!(
        native.source_map.entries.len(),
        n,
        "parity smoke: native source map must have {n} entries"
    );
}

// ── Parity: hash chain independence ───────────────────────────────────────

/// Parity smoke: WASM backend does not contaminate native_hash.
///
/// After `emit_wasm`, `hash_chain.native_hash` must be None.
/// This proves the two backends do not share hash chain entries.
#[test]
fn parity_smoke_wasm_does_not_set_native_hash() {
    let anf = anf_for_n(2);
    let wasm = emit_wasm(&anf).expect("emit_wasm");

    assert!(
        wasm.hash_chain.native_hash.is_none(),
        "parity smoke: emit_wasm must not populate native_hash"
    );
    assert!(
        wasm.hash_chain.wasm_hash.is_some(),
        "parity smoke: emit_wasm must populate wasm_hash"
    );
}

/// Parity smoke: native backend does not contaminate wasm_hash.
///
/// After `emit_native`, `hash_chain.wasm_hash` must be None.
#[test]
fn parity_smoke_native_does_not_set_wasm_hash() {
    let anf = anf_for_n(2);
    let native = emit_native(&anf).expect("emit_native");

    assert!(
        native.hash_chain.wasm_hash.is_none(),
        "parity smoke: emit_native must not populate wasm_hash"
    );
    assert!(
        native.hash_chain.native_hash.is_some(),
        "parity smoke: emit_native must populate native_hash"
    );
}

/// Parity smoke: wasm_hash and native_hash differ for the same AnfIr input.
///
/// Different hash chain formulas produce different digests even for the same
/// ANF.  This ensures hash chain entries are content-specific.
#[test]
fn parity_smoke_wasm_hash_and_native_hash_differ() {
    let anf = anf_for_n(2);
    let wasm = emit_wasm(&anf).expect("emit_wasm");
    let native = emit_native(&anf).expect("emit_native");

    let wasm_hash = wasm.hash_chain.wasm_hash.expect("wasm_hash must be Some");
    let native_hash = native
        .hash_chain
        .native_hash
        .expect("native_hash must be Some");

    assert_ne!(
        wasm_hash, native_hash,
        "parity smoke: wasm_hash and native_hash must differ (different content, different formula)"
    );
}

// ── Parity: simple expressions ────────────────────────────────────────────

/// Parity smoke: integer literal compiles in both backends.
#[test]
fn parity_smoke_int_literal_compiles_in_both() {
    let anf = sealed_anf_single(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_const".to_string(),
        expr: AnfExpr::Literal(LiteralValue::Int(42)),
    });

    assert!(
        emit_wasm(&anf).is_ok(),
        "parity smoke: int literal must succeed in WASM"
    );
    assert!(
        emit_native(&anf).is_ok(),
        "parity smoke: int literal must succeed in native"
    );
}

/// Parity smoke: bool literal compiles in both backends.
#[test]
fn parity_smoke_bool_literal_compiles_in_both() {
    let anf = sealed_anf_single(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_bool".to_string(),
        expr: AnfExpr::Literal(LiteralValue::Bool(true)),
    });

    assert!(
        emit_wasm(&anf).is_ok(),
        "parity smoke: bool literal must succeed in WASM"
    );
    assert!(
        emit_native(&anf).is_ok(),
        "parity smoke: bool literal must succeed in native"
    );
}

/// Parity smoke: i64.add compiles in both backends.
///
/// `i64.add` is the canonical arithmetic operation.  Both backends lower it
/// to real machine instructions (not trap stubs), so this is the primary
/// executable-subset parity assertion.
#[test]
fn parity_smoke_i64_add_compiles_in_both() {
    let anf = sealed_anf_single(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_add".to_string(),
        expr: AnfExpr::Let {
            name: "x".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::Let {
                name: "y".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(2))),
                body: Box::new(AnfExpr::Call {
                    func: "i64.add".to_string(),
                    args: vec!["x".to_string(), "y".to_string()],
                }),
            }),
        },
    });

    let wasm = emit_wasm(&anf).expect("parity smoke: i64.add must succeed in WASM");
    let native = emit_native(&anf).expect("parity smoke: i64.add must succeed in native");

    // Both produce non-empty binary output.
    assert!(
        !wasm.wasm.is_empty(),
        "parity smoke: i64.add WASM must be non-empty"
    );
    assert!(
        !native.native_bytes.is_empty(),
        "parity smoke: i64.add native must be non-empty"
    );

    // Both seal their respective hashes.
    assert!(
        wasm.hash_chain.wasm_hash.is_some(),
        "parity smoke: i64.add WASM hash must be sealed"
    );
    assert!(
        native.hash_chain.native_hash.is_some(),
        "parity smoke: i64.add native hash must be sealed"
    );
}

/// Parity smoke: If expression compiles in both backends.
#[test]
fn parity_smoke_if_expression_compiles_in_both() {
    let anf = sealed_anf_single(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_if".to_string(),
        expr: AnfExpr::Let {
            name: "cond".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
            body: Box::new(AnfExpr::If {
                cond: "cond".to_string(),
                then_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
                else_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            }),
        },
    });

    assert!(
        emit_wasm(&anf).is_ok(),
        "parity smoke: If expression must succeed in WASM"
    );
    assert!(
        emit_native(&anf).is_ok(),
        "parity smoke: If expression must succeed in native"
    );
}

/// Parity smoke: i64.add produces different output than Placeholder in both backends.
///
/// Both backends must produce substantively different artifacts for a real
/// arithmetic expression vs. a Placeholder trap stub.  This confirms neither
/// backend degrades all expressions to a single stub.
#[test]
fn parity_smoke_arithmetic_differs_from_placeholder_in_both_backends() {
    let arithmetic = sealed_anf_single(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "a".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(10))),
            body: Box::new(AnfExpr::Let {
                name: "b".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(3))),
                body: Box::new(AnfExpr::Call {
                    func: "i64.sub".to_string(),
                    args: vec!["a".to_string(), "b".to_string()],
                }),
            }),
        },
    });

    let placeholder = sealed_anf_single(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Placeholder,
    });

    let wasm_arith = emit_wasm(&arithmetic).expect("WASM arithmetic");
    let wasm_placeholder = emit_wasm(&placeholder).expect("WASM placeholder");
    assert_ne!(
        wasm_arith.wasm, wasm_placeholder.wasm,
        "parity smoke: i64.sub must differ from Placeholder in WASM"
    );

    let native_arith = emit_native(&arithmetic).expect("native arithmetic");
    let native_placeholder = emit_native(&placeholder).expect("native placeholder");
    assert_ne!(
        native_arith.native_bytes, native_placeholder.native_bytes,
        "parity smoke: i64.sub must differ from Placeholder in native"
    );
}

// ── Parity: source map entry node_ids match ───────────────────────────────

/// Parity smoke: source map node_ids in WASM and native outputs match.
///
/// Both backends clone the ANF source map and add backend-specific offsets.
/// The node_id in each entry must be identical across both backends.
#[test]
fn parity_smoke_source_map_node_ids_match_between_backends() {
    let n = 3;
    let anf = anf_for_n(n);
    let wasm = emit_wasm(&anf).expect("emit_wasm");
    let native = emit_native(&anf).expect("emit_native");

    let wasm_node_ids: Vec<NodeRef> = wasm.source_map.entries.iter().map(|e| e.node_id).collect();
    let native_node_ids: Vec<NodeRef> = native
        .source_map
        .entries
        .iter()
        .map(|e| e.node_id)
        .collect();

    assert_eq!(
        wasm_node_ids, native_node_ids,
        "parity smoke: source map node_ids must match between WASM and native"
    );
}

/// Parity smoke: WASM source map has wasm_offset; native has native_offset.
///
/// Each backend populates only its own offset field.  Cross-contamination
/// would break debugging tools that rely on offset specificity.
#[test]
fn parity_smoke_backends_populate_their_own_offset_field_only() {
    let n = 2;
    let anf = anf_for_n(n);
    let wasm = emit_wasm(&anf).expect("emit_wasm");
    let native = emit_native(&anf).expect("emit_native");

    for (i, entry) in wasm.source_map.entries.iter().enumerate() {
        assert!(
            entry.wasm_offset.is_some(),
            "parity smoke: WASM source map entry {i} must have wasm_offset"
        );
        assert!(
            entry.native_offset.is_none(),
            "parity smoke: WASM source map entry {i} must NOT have native_offset"
        );
    }

    for (i, entry) in native.source_map.entries.iter().enumerate() {
        assert!(
            entry.native_offset.is_some(),
            "parity smoke: native source map entry {i} must have native_offset"
        );
        assert!(
            entry.wasm_offset.is_none(),
            "parity smoke: native source map entry {i} must NOT have wasm_offset"
        );
    }
}
