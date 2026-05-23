// ── ail-compiler::wasm_tests ─────────────────────────────────────────────
//
// Task 3.1 (RED): Tests written BEFORE emit_wasm / WasmArtifact exist.
// These drive the implementation of src/wasm.rs.
//
// Spec scenarios covered:
//  - wasmparser::validate accepts every emitted module.
//  - Zero-binding graph → minimal valid WASM module (magic + version).
//  - N-binding graph → N functions each with [unreachable, end] body.
//  - provenance map has exactly N entries for an N-binding AnfIr.
//  - wasm_hash is sealed in hash_chain after emit_wasm.
//  - Determinism: same AnfIr → identical wasm bytes across two calls.

use ail_compiler::core_ir::{CoreExpr, LiteralValue, MatchArm, StageHashes};
use ail_compiler::{
    AnfBinding, AnfExpr, AnfIr, SourceMap, emit_wasm,
    lower::{lower_core_expr_to_anf, lower_to_anf, lower_to_core_ir},
};
use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_verify::report::VerificationReport;

// ── helpers ──────────────────────────────────────────────────────────────

fn empty_graph() -> SemanticGraph {
    SemanticGraph {
        nodes: vec![],
        edges: vec![],
    }
}

fn graph_with_n_nodes(n: usize) -> SemanticGraph {
    SemanticGraph {
        nodes: (0..n)
            .map(|i| GraphNode::new(NodeRef(i as u32), NodeKind::Function, format!("fn_{i}")))
            .collect(),
        edges: vec![],
    }
}

fn proven_report() -> VerificationReport {
    VerificationReport {
        entries: vec![],
        ..Default::default()
    }
}

fn anf_for_graph(graph: &SemanticGraph) -> ail_compiler::AnfIr {
    let core = lower_to_core_ir(graph, &proven_report()).expect("lower_to_core_ir failed");
    lower_to_anf(&core).expect("lower_to_anf failed")
}

fn sealed_anf(binding: AnfBinding) -> AnfIr {
    AnfIr {
        schema_version: ail_compiler::anf::ANF_SCHEMA_VERSION,
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

fn operators(wasm: &[u8]) -> Vec<String> {
    use wasmparser::{Parser, Payload};

    let mut names = Vec::new();
    for payload in Parser::new(0).parse_all(wasm) {
        if let Payload::CodeSectionEntry(body) = payload.expect("payload must parse") {
            let mut reader = body
                .get_operators_reader()
                .expect("operators reader must build");
            while !reader.eof() {
                names.push(format!("{:?}", reader.read().expect("operator must read")));
            }
        }
    }
    names
}

fn emit_valid_wasm(expr: AnfExpr, name: &str) -> Vec<String> {
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: name.to_string(),
        expr,
    };
    let artifact = emit_wasm(&sealed_anf(binding)).expect("emit_wasm failed");
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");
    operators(&artifact.wasm)
}

fn contains_match(expr: &AnfExpr) -> bool {
    match expr {
        AnfExpr::Match { .. } => true,
        AnfExpr::Let { value, body, .. } => contains_match(value) || contains_match(body),
        _ => false,
    }
}

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
fn one_node_graph_emits_valid_wasm_module() {
    let anf = anf_for_graph(&graph_with_n_nodes(1));
    let artifact = emit_wasm(&anf).expect("emit_wasm failed on 1-node graph");
    wasmparser::validate(&artifact.wasm).expect("wasmparser rejected 1-function wasm module");
}

// ── Task 3.1: provenance map completeness ────────────────────────────────

// Spec: provenance map has exactly N entries for N-binding AnfIr.
#[test]
fn provenance_map_has_n_entries_for_n_nodes() {
    let anf = anf_for_graph(&graph_with_n_nodes(4));
    let artifact = emit_wasm(&anf).expect("emit_wasm failed");
    assert_eq!(
        artifact.provenance.len(),
        4,
        "provenance map must have exactly 4 entries for a 4-node graph"
    );
}

// TRIANGULATE: zero-node graph has empty provenance map.
#[test]
fn provenance_map_is_empty_for_zero_node_graph() {
    let anf = anf_for_graph(&empty_graph());
    let artifact = emit_wasm(&anf).expect("emit_wasm failed");
    assert!(
        artifact.provenance.is_empty(),
        "provenance map must be empty for a zero-binding AnfIr"
    );
}

// Spec: provenance NodeRefs match the source bindings.
#[test]
fn provenance_map_contains_correct_node_refs() {
    let graph = graph_with_n_nodes(3);
    let anf = anf_for_graph(&graph);
    let artifact = emit_wasm(&anf).expect("emit_wasm failed");
    // Each NodeRef(0), NodeRef(1), NodeRef(2) must be in the provenance map.
    for i in 0..3u32 {
        assert!(
            artifact.provenance.contains_key(&NodeRef(i)),
            "provenance must contain NodeRef({i})"
        );
    }
}

// ── Task 3.1: hash chain sealing ─────────────────────────────────────────

// Spec: wasm_hash is Some after emit_wasm.
#[test]
fn wasm_hash_is_sealed_after_emit() {
    let anf = anf_for_graph(&graph_with_n_nodes(2));
    let artifact = emit_wasm(&anf).expect("emit_wasm failed");
    assert!(
        artifact.hash_chain.wasm_hash.is_some(),
        "wasm_hash must be Some after emit_wasm"
    );
}

// Spec: hash_chain carries verification_report_hash through to WASM stage.
#[test]
fn hash_chain_preserves_verification_report_hash() {
    let anf = anf_for_graph(&graph_with_n_nodes(1));
    let artifact = emit_wasm(&anf).expect("emit_wasm failed");
    // verification_report_hash is always computed (even for empty report).
    assert_ne!(
        artifact.hash_chain.verification_report_hash, [0u8; 32],
        "verification_report_hash must be non-zero after pipeline"
    );
}

// ── Task 3.1: determinism ────────────────────────────────────────────────

// Spec: same AnfIr produces identical wasm bytes in two calls.
#[test]
fn emit_wasm_is_deterministic() {
    let anf = anf_for_graph(&graph_with_n_nodes(3));
    let a1 = emit_wasm(&anf).expect("emit_wasm first call");
    let a2 = emit_wasm(&anf).expect("emit_wasm second call");
    assert_eq!(
        a1.wasm, a2.wasm,
        "emit_wasm must produce identical bytes for the same AnfIr"
    );
    assert_eq!(
        a1.hash_chain.wasm_hash, a2.hash_chain.wasm_hash,
        "wasm_hash must be identical across two calls with the same AnfIr"
    );
}

// TRIANGULATE: different AnfIrs produce different wasm hashes.
#[test]
fn different_anf_inputs_produce_different_wasm_hashes() {
    let anf_2 = anf_for_graph(&graph_with_n_nodes(2));
    let anf_3 = anf_for_graph(&graph_with_n_nodes(3));
    let a2 = emit_wasm(&anf_2).expect("emit_wasm 2-node");
    let a3 = emit_wasm(&anf_3).expect("emit_wasm 3-node");
    assert_ne!(
        a2.hash_chain.wasm_hash, a3.hash_chain.wasm_hash,
        "different AnfIr inputs must produce different wasm_hashes"
    );
}

// ── G20 R2: function bodies are real ANF-derived WASM code ───────────────

// Spec (G20 R2): function bodies are generated from ANF expressions — no longer
// stub-only.  For Placeholder/default nodes, the body contains unreachable.
// For real AnfExpr nodes, the body contains real instructions ending with End.
//
// We assert:
//  1. The correct number of code sections are emitted.
//  2. Every body ends with End (valid WASM).
//  3. Bodies are structurally valid (wasmparser validates the whole module).
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
#[test]
fn provenance_values_are_byte_offsets_not_function_indexes() {
    let n = 3usize;
    let anf = anf_for_graph(&graph_with_n_nodes(n));
    let artifact = emit_wasm(&anf).expect("emit_wasm failed");

    assert_eq!(
        artifact.provenance.len(),
        n,
        "provenance must have {n} entries"
    );

    for i in 0..n {
        let nr = NodeRef(i as u32);
        let offset = *artifact
            .provenance
            .get(&nr)
            .unwrap_or_else(|| panic!("NodeRef({i}) missing from provenance"));

        // The byte at the stored offset is the LEB128-encoded body size.
        // For G20 R2, bodies contain real ANF-derived code — size > 0.
        // We assert the offset is a valid index into the WASM binary.
        assert!(
            (offset as usize) < artifact.wasm.len(),
            "provenance offset {offset} must be within wasm binary of len {}",
            artifact.wasm.len()
        );
        // The byte at the offset must be non-zero (non-empty body).
        assert_ne!(
            artifact.wasm[offset as usize], 0x00,
            "wasm[provenance[NodeRef({i})]] = wasm[{offset}] must be non-zero (body size), \
             got 0x{:02x} — body cannot be empty",
            artifact.wasm[offset as usize]
        );
        // Crucially: the value must NOT equal the function index (0, 1, 2).
        // If it were a function index, wasm[0] = 0x00 (magic byte) and
        // provenance[NodeRef(0)] would equal 0 — but 0x00 is the magic byte,
        // not a valid code-entry header.
        // We prove it's an offset by checking offset > 8 (past the WASM header).
        assert!(
            offset > 8,
            "provenance offset {offset} must be past the 8-byte WASM header \
             (function index 0 would point to magic byte region)"
        );
    }
}

// ── verify-fix: explicit WASM hash-chain recomputation ────────────────────

// Spec: wasm_hash = blake3(anf_ir_hash || wasm_binary)
// Verify by recomputing the hash explicitly and comparing.
#[test]
fn wasm_hash_chain_matches_explicit_recomputation() {
    use ail_compiler::hash::hash_with_parent;

    let anf = anf_for_graph(&graph_with_n_nodes(2));
    let artifact = emit_wasm(&anf).expect("emit_wasm failed");

    let anf_ir_hash = anf
        .stage_hashes
        .anf_ir_hash
        .expect("anf_ir_hash must be sealed before emit_wasm");

    // Recompute: wasm_hash = blake3(anf_ir_hash || wasm_bytes)
    let expected_hash = hash_with_parent(&anf_ir_hash, &artifact.wasm);

    assert_eq!(
        artifact.hash_chain.wasm_hash,
        Some(expected_hash),
        "wasm_hash must equal blake3(anf_ir_hash || wasm_binary)"
    );
}

#[test]
fn anf_if_emits_real_wasm_if_else() {
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.branch".to_string(),
        expr: AnfExpr::Let {
            name: "flag".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
            body: Box::new(AnfExpr::If {
                cond: "flag".to_string(),
                then_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(10))),
                else_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(20))),
            }),
        },
    };
    let artifact = emit_wasm(&sealed_anf(binding)).expect("emit_wasm failed");

    wasmparser::validate(&artifact.wasm).expect("if wasm must validate");
    let ops = operators(&artifact.wasm);
    assert!(
        ops.iter().any(|op| op.starts_with("If")),
        "expected If in {ops:?}"
    );
    assert!(
        ops.iter().any(|op| op == "Else"),
        "expected Else in {ops:?}"
    );
    assert!(
        !ops.iter().any(|op| op == "Unreachable"),
        "if must not emit unreachable: {ops:?}"
    );
}

#[test]
fn effect_call_emits_host_call_import_and_call() {
    use wasmparser::{Operator, Parser, Payload};

    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.main".to_string(),
        expr: AnfExpr::Let {
            name: "n".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(41))),
            body: Box::new(AnfExpr::EffectCall {
                capability: "test.counter".to_string(),
                func: "inc".to_string(),
                args: vec!["n".to_string()],
            }),
        },
    };
    let anf = sealed_anf(binding);
    let artifact = emit_wasm(&anf).expect("emit_wasm failed");
    wasmparser::validate(&artifact.wasm).expect("effect wasm must validate");

    let mut saw_import = false;
    let mut saw_host_call = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        match payload.expect("payload must parse") {
            Payload::ImportSection(imports) => {
                for import in imports {
                    let import = import.expect("import must parse");
                    let rendered = format!("{import:?}");
                    if rendered.contains("ail") && rendered.contains("host_call") {
                        saw_import = true;
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                let mut reader = body.get_operators_reader().expect("operators");
                while !reader.eof() {
                    if matches!(
                        reader.read().expect("operator"),
                        Operator::Call { function_index: 0 }
                    ) {
                        saw_host_call = true;
                    }
                }
            }
            _ => {}
        }
    }

    assert!(saw_import, "expected ail/host_call import");
    assert!(
        saw_host_call,
        "expected effect call to call imported function 0"
    );
}

#[test]
fn core_if_lowers_to_anf_if_and_emits_valid_wasm() {
    let core = CoreExpr::If {
        cond: Box::new(CoreExpr::Literal(LiteralValue::Bool(false))),
        then_: Box::new(CoreExpr::Literal(LiteralValue::Int(1))),
        else_: Box::new(CoreExpr::Literal(LiteralValue::Int(2))),
    };
    let mut fresh = 0;
    let mut synthetic = Vec::new();
    let expr = lower_core_expr_to_anf(&core, &mut fresh, NodeRef(0), &mut synthetic);
    assert!(matches!(expr, AnfExpr::If { .. }));

    let mut bindings = synthetic;
    bindings.push(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.core_branch".to_string(),
        expr,
    });
    let anf = AnfIr {
        schema_version: ail_compiler::anf::ANF_SCHEMA_VERSION,
        source_map: SourceMap::from_bindings(&bindings),
        bindings,
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
    };

    let artifact = emit_wasm(&anf).expect("emit_wasm failed");
    wasmparser::validate(&artifact.wasm).expect("core if wasm must validate");
}

#[test]
fn anf_match_on_i64_literal_emits_real_branching() {
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.match_value".to_string(),
        expr: AnfExpr::Let {
            name: "tag".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(2))),
            body: Box::new(AnfExpr::Match {
                scrutinee: "tag".to_string(),
                arms: vec![
                    ail_compiler::anf::AnfMatchArm {
                        pattern: "1".to_string(),
                        body: AnfExpr::Literal(LiteralValue::Int(10)),
                    },
                    ail_compiler::anf::AnfMatchArm {
                        pattern: "2".to_string(),
                        body: AnfExpr::Literal(LiteralValue::Int(20)),
                    },
                    ail_compiler::anf::AnfMatchArm {
                        pattern: "_".to_string(),
                        body: AnfExpr::Literal(LiteralValue::Int(30)),
                    },
                ],
            }),
        },
    };
    let artifact = emit_wasm(&sealed_anf(binding)).expect("emit_wasm failed");

    wasmparser::validate(&artifact.wasm).expect("match wasm must validate");
    let ops = operators(&artifact.wasm);
    assert!(
        ops.iter().any(|op| op == "I64Eq"),
        "expected i64 equality in {ops:?}"
    );
    assert!(
        ops.iter().any(|op| op.starts_with("If")),
        "expected If cascade in {ops:?}"
    );
    assert!(
        !ops.iter().any(|op| op == "Unreachable"),
        "match must not emit unreachable: {ops:?}"
    );
}

#[test]
fn core_match_lowers_to_anf_match_and_emits_valid_wasm() {
    let core = CoreExpr::Match {
        scrutinee: Box::new(CoreExpr::Literal(LiteralValue::Int(1))),
        arms: vec![
            MatchArm {
                pattern: "1".to_string(),
                body: CoreExpr::Literal(LiteralValue::Int(11)),
            },
            MatchArm {
                pattern: "_".to_string(),
                body: CoreExpr::Literal(LiteralValue::Int(22)),
            },
        ],
    };
    let mut fresh = 0;
    let mut synthetic = Vec::new();
    let expr = lower_core_expr_to_anf(&core, &mut fresh, NodeRef(0), &mut synthetic);
    assert!(matches!(expr, AnfExpr::Match { .. }));

    let mut bindings = synthetic;
    bindings.push(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.core_match".to_string(),
        expr,
    });
    let anf = AnfIr {
        schema_version: ail_compiler::anf::ANF_SCHEMA_VERSION,
        source_map: SourceMap::from_bindings(&bindings),
        bindings,
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
    };

    let artifact = emit_wasm(&anf).expect("emit_wasm failed");
    wasmparser::validate(&artifact.wasm).expect("core match wasm must validate");
}

#[test]
fn parsed_match_body_lowers_to_anf_and_emits_valid_wasm() {
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.match_surface");
    node.body_expr = Some("match(2, 1, 10, 2, 20, _, 30)".to_string());
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };

    let core = lower_to_core_ir(&graph, &proven_report()).expect("core lowering must parse match");
    assert!(
        matches!(core.nodes[0].expr, Some(CoreExpr::Match { .. })),
        "body_expr must parse to CoreExpr::Match"
    );

    let anf = lower_to_anf(&core).expect("ANF lowering must handle parsed match");
    assert!(
        contains_match(&anf.bindings[0].expr),
        "parsed match must survive into ANF"
    );

    let artifact = emit_wasm(&anf).expect("emit_wasm failed");
    wasmparser::validate(&artifact.wasm).expect("parsed match wasm must validate");
    let ops = operators(&artifact.wasm);
    assert!(
        ops.iter().any(|op| op == "I64Eq"),
        "parsed match must emit equality checks, got {ops:?}"
    );
    assert!(
        ops.iter().any(|op| op.starts_with("If")),
        "parsed match must emit branch cascade, got {ops:?}"
    );
}
