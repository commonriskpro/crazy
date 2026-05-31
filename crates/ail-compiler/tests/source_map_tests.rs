// ── ail-compiler::source_map_tests ───────────────────────────────────────
//
// G32: Semantic source map integration tests.
//
// Spec scenarios covered:
//  - emit_wasm populates wasm_offset for every emitted binding.
//  - emit_native populates native_offset for every emitted binding.
//  - Source maps have one entry per binding (including synthetic duplicates).
//  - Empty input yields empty source map.
//  - Any source-map change changes source_map_hash downstream.
//  - Duplicate NodeRefs are preserved (not collapsed) in source maps.
//  - lower_to_anf_with_graph threads provenance (change_set, block_ref,
//    contract_ref, effect_ref, runtime_check_ref) from SemanticGraph nodes.

use ail_compiler::{
    AnfIr, CompileError, SourceMap, SourceMapSpan, emit_native, emit_native_with_profile,
    emit_wasm, emit_wasm_with_profile,
    lower::{lower_to_anf, lower_to_anf_with_graph, lower_to_core_ir},
};
use ail_core::semantic_graph::{
    ContractClauses, EdgeKind, EffectRow, GraphEdge, GraphNode, NodeKind, NodeRef, Provenance,
    RefinementRef, RefinementStatus, RuntimeCheckMeta, SemanticGraph, Span,
};
use ail_verify::report::VerificationReport;

// ── helpers ──────────────────────────────────────────────────────────────

fn proven_report() -> VerificationReport {
    VerificationReport {
        entries: vec![],
        ..Default::default()
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

fn anf_for_n(n: usize) -> ail_compiler::AnfIr {
    let graph = graph_with_n_nodes(n);
    let core = lower_to_core_ir(&graph, &proven_report()).expect("lower_to_core_ir");
    lower_to_anf(&core).expect("lower_to_anf")
}

fn graph_with_n_provenance_nodes(n: usize) -> SemanticGraph {
    SemanticGraph {
        nodes: (0..n)
            .map(|i| {
                let mut node =
                    GraphNode::new(NodeRef(i as u32), NodeKind::Function, format!("fn_{i}"));
                node.provenance = Some(Provenance {
                    change_id: format!("change.fn_{i}"),
                });
                node
            })
            .collect(),
        edges: vec![],
    }
}

fn proven_anf_for_n(n: usize) -> AnfIr {
    let graph = graph_with_n_provenance_nodes(n);
    let core = lower_to_core_ir(&graph, &proven_report()).expect("lower_to_core_ir");
    lower_to_anf_with_graph(&core, &graph).expect("lower_to_anf_with_graph")
}

// ── Task 4: WASM backend populates wasm_offset ────────────────────────────

// Spec: emit_wasm must return a semantic source map with wasm_offset populated
// for every emitted binding.
// RED → GREEN: WasmArtifact.source_map is populated with non-None wasm_offset.
#[test]
fn emit_wasm_populates_wasm_offset_for_every_binding() {
    let n = 3usize;
    let anf = anf_for_n(n);
    let artifact = emit_wasm(&anf).expect("emit_wasm");

    assert_eq!(
        artifact.source_map.entries.len(),
        n,
        "source map must have one entry per binding"
    );
    for (i, entry) in artifact.source_map.entries.iter().enumerate() {
        assert!(
            entry.wasm_offset.is_some(),
            "entry {i} must have wasm_offset set after emit_wasm, got None"
        );
        assert!(
            entry.generated_span.is_some(),
            "entry {i} must have generated_span set after emit_wasm"
        );
        assert!(
            entry.native_offset.is_none(),
            "entry {i} must NOT have native_offset set by emit_wasm"
        );
    }
}

// TRIANGULATE: wasm_offset values are actual byte offsets (past WASM header).
#[test]
fn emit_wasm_offsets_are_past_wasm_header() {
    let anf = anf_for_n(2);
    let artifact = emit_wasm(&anf).expect("emit_wasm");
    for (i, entry) in artifact.source_map.entries.iter().enumerate() {
        let offset = entry.wasm_offset.expect("wasm_offset must be Some");
        assert!(
            offset > 8,
            "entry {i}: wasm_offset {offset} must be past the 8-byte WASM header"
        );
    }
}

// Spec: empty input yields empty source map after emit_wasm.
#[test]
fn emit_wasm_empty_input_yields_empty_source_map() {
    let anf = anf_for_n(0);
    let artifact = emit_wasm(&anf).expect("emit_wasm");
    assert!(
        artifact.source_map.entries.is_empty(),
        "empty AnfIr must produce empty source map in WasmArtifact"
    );
}

// Spec: source map entry node_id matches the binding's source_ref.
#[test]
fn emit_wasm_source_map_node_ids_match_bindings() {
    let n = 3;
    let anf = anf_for_n(n);
    let artifact = emit_wasm(&anf).expect("emit_wasm");
    for (binding, entry) in anf.bindings.iter().zip(artifact.source_map.entries.iter()) {
        assert_eq!(
            entry.node_id, binding.source_ref,
            "source map entry node_id must match binding.source_ref"
        );
        assert_eq!(
            entry.binding_name, binding.name,
            "source map entry binding_name must match binding.name"
        );
    }
}

// ── Task 4: Native backend populates native_offset ────────────────────────

// Spec: emit_native must return a semantic source map with native_offset
// populated for every emitted binding.
#[test]
fn emit_native_populates_native_offset_for_every_binding() {
    let n = 3usize;
    let anf = anf_for_n(n);
    let artifact = emit_native(&anf).expect("emit_native");

    assert_eq!(
        artifact.source_map.entries.len(),
        n,
        "source map must have one entry per binding"
    );
    for (i, entry) in artifact.source_map.entries.iter().enumerate() {
        assert!(
            entry.native_offset.is_some(),
            "entry {i} must have native_offset set after emit_native, got None"
        );
        assert!(
            entry.generated_span.is_some(),
            "entry {i} must have generated_span set after emit_native"
        );
        assert!(
            entry.wasm_offset.is_none(),
            "entry {i} must NOT have wasm_offset set by emit_native"
        );
    }
}

// Spec: empty input yields empty source map after emit_native.
#[test]
fn emit_native_empty_input_yields_empty_source_map() {
    let anf = anf_for_n(0);
    let artifact = emit_native(&anf).expect("emit_native");
    assert!(
        artifact.source_map.entries.is_empty(),
        "empty AnfIr must produce empty source map in NativeArtifact"
    );
}

// Spec: source map entry node_id matches the binding's source_ref.
#[test]
fn emit_native_source_map_node_ids_match_bindings() {
    let n = 3;
    let anf = anf_for_n(n);
    let artifact = emit_native(&anf).expect("emit_native");
    for (binding, entry) in anf.bindings.iter().zip(artifact.source_map.entries.iter()) {
        assert_eq!(
            entry.node_id, binding.source_ref,
            "source map entry node_id must match binding.source_ref"
        );
    }
}

// ── Task 3: source_map_hash ────────────────────────────────────────────────

// Spec: any source-map change changes source_map_hash.
// We verify that source_map_hash is Some and that different inputs produce
// different hashes.
#[test]
fn emit_wasm_sets_source_map_hash() {
    let anf = anf_for_n(2);
    let artifact = emit_wasm(&anf).expect("emit_wasm");
    assert!(
        artifact.hash_chain.source_map_hash.is_some(),
        "source_map_hash must be Some after emit_wasm"
    );
}

// TRIANGULATE: different AnfIr inputs produce different source_map_hash.
#[test]
fn different_inputs_produce_different_source_map_hashes_wasm() {
    let a1 = emit_wasm(&anf_for_n(1)).expect("emit_wasm 1");
    let a2 = emit_wasm(&anf_for_n(2)).expect("emit_wasm 2");
    assert_ne!(
        a1.hash_chain.source_map_hash, a2.hash_chain.source_map_hash,
        "different AnfIr inputs must produce different source_map_hashes"
    );
}

// Spec: emit_native also sets source_map_hash.
#[test]
fn emit_native_sets_source_map_hash() {
    let anf = anf_for_n(2);
    let artifact = emit_native(&anf).expect("emit_native");
    assert!(
        artifact.hash_chain.source_map_hash.is_some(),
        "source_map_hash must be Some after emit_native"
    );
}

// TRIANGULATE: different native inputs produce different source_map_hash.
#[test]
fn different_inputs_produce_different_source_map_hashes_native() {
    let a1 = emit_native(&anf_for_n(1)).expect("emit_native 1");
    let a2 = emit_native(&anf_for_n(2)).expect("emit_native 2");
    assert_ne!(
        a1.hash_chain.source_map_hash, a2.hash_chain.source_map_hash,
        "different AnfIr inputs must produce different source_map_hashes for native"
    );
}

// Spec: source_map_hash changes when source map content changes.
// We verify by computing a deterministic hash and comparing across runs.
#[test]
fn source_map_hash_is_deterministic_across_runs() {
    let anf = anf_for_n(3);
    let a1 = emit_wasm(&anf).expect("first emit_wasm");
    let a2 = emit_wasm(&anf).expect("second emit_wasm");
    assert_eq!(
        a1.hash_chain.source_map_hash, a2.hash_chain.source_map_hash,
        "source_map_hash must be identical across two calls with the same AnfIr"
    );
}

// ── G32 Round 2: Provenance threading via lower_to_anf_with_graph ─────────

/// Build a `SemanticGraph` with one Function node that has full provenance
/// metadata: provenance.change_id, contract_clauses, effect_row, and
/// runtime_checks.
fn graph_with_rich_provenance() -> SemanticGraph {
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn_checkout");
    node.provenance = Some(Provenance {
        change_id: "change.add_checkout".to_string(),
    });
    node.contract_clauses = Some(ContractClauses {
        requires: vec!["amount > 0".to_string()],
        ensures: vec![],
    });
    node.effect_row = Some(EffectRow {
        effects: vec!["database.read".to_string()],
    });
    node.runtime_checks = Some(vec![RuntimeCheckMeta {
        predicate: "cart_id != null".to_string(),
        hash: "rtcheck_hash_abc123".to_string(),
    }]);
    SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    }
}

/// Build a `SemanticGraph` with one Module node (which should receive a
/// block_ref) and one Function node (which should NOT receive a block_ref).
fn graph_with_module_and_function() -> SemanticGraph {
    SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Module, "mod_core"),
            GraphNode::new(NodeRef(1), NodeKind::Function, "fn_run"),
        ],
        edges: vec![],
    }
}

fn graph_with_boundary_provenance() -> SemanticGraph {
    let mut boundary = GraphNode::new(NodeRef(10), NodeKind::Boundary, "public_api");
    boundary.provenance = Some(Provenance {
        change_id: "change.public_boundary".to_string(),
    });
    boundary.contract_clauses = Some(ContractClauses {
        requires: vec!["authenticated".to_string()],
        ensures: vec!["audited".to_string()],
    });

    SemanticGraph {
        nodes: vec![boundary],
        edges: vec![],
    }
}

#[test]
fn lower_to_anf_with_graph_threads_source_span() {
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn_with_span");
    node.span = Some(Span {
        source: "src/private_checkout.ail".to_string(),
        start: 12,
        end: 44,
    });
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let core = lower_to_core_ir(&graph, &proven_report()).expect("lower_to_core_ir");
    let anf = lower_to_anf_with_graph(&core, &graph).expect("lower_to_anf_with_graph");
    let span = anf.source_map.entries[0]
        .source_span
        .as_ref()
        .expect("source span must be threaded into source map");

    assert_eq!(span.file_id, "src/private_checkout.ail");
    assert_eq!(span.start, 12);
    assert_eq!(span.end, 44);
}

#[test]
fn source_map_validation_reports_stable_redacted_diagnostics_in_order() {
    let mut map = anf_for_n(2).source_map;
    map.entries[0].source_span = Some(SourceMapSpan::new("/secret/customer_a.ail", 20, 10));
    map.entries[0].generated_span = Some(SourceMapSpan::new("program.wasm", 0, 12));
    map.entries[1].source_span = Some(SourceMapSpan::new("", 1, 5));
    map.entries[1].generated_span = Some(SourceMapSpan::new("program.wasm", 8, 20));

    let issues = map.validation_issues();
    let codes: Vec<&str> = issues.iter().map(|issue| issue.code).collect();
    assert_eq!(codes, vec!["AIL-SM-001", "AIL-SM-002", "AIL-SM-003"]);
    assert_eq!(issues[0].category, "span.range");
    assert_eq!(issues[1].category, "span.file_id");
    assert_eq!(issues[2].category, "generated.overlap");

    let rendered = CompileError::InvalidSourceMap { issues }.to_string();
    assert!(rendered.contains("file-id=present"));
    assert!(rendered.contains("file-id=missing"));
    assert!(
        !rendered.contains("customer_a"),
        "diagnostic descriptor must not expose raw source file ids: {rendered}"
    );
}

// RED → GREEN: lower_to_anf_with_graph threads change_set from
// GraphNode.provenance.change_id into each SourceMapEntry.
#[test]
fn lower_to_anf_with_graph_threads_change_set_from_provenance() {
    let graph = graph_with_rich_provenance();
    let core = lower_to_core_ir(&graph, &proven_report()).expect("lower_to_core_ir");
    let anf = lower_to_anf_with_graph(&core, &graph).expect("lower_to_anf_with_graph");
    assert_eq!(anf.source_map.entries.len(), 1);
    assert_eq!(
        anf.source_map.entries[0].change_set.as_deref(),
        Some("change.add_checkout"),
        "change_set must be threaded from GraphNode.provenance.change_id"
    );
}

// RED → GREEN: lower_to_anf_with_graph threads contract_ref from
// GraphNode.contract_clauses (derived as `contract.<node_name>`).
#[test]
fn lower_to_anf_with_graph_threads_contract_ref_from_clauses() {
    let graph = graph_with_rich_provenance();
    let core = lower_to_core_ir(&graph, &proven_report()).expect("lower_to_core_ir");
    let anf = lower_to_anf_with_graph(&core, &graph).expect("lower_to_anf_with_graph");
    let contract_ref = anf.source_map.entries[0].contract_ref.as_ref();
    assert!(
        contract_ref.is_some(),
        "contract_ref must be Some when GraphNode has contract_clauses"
    );
    assert!(
        contract_ref.unwrap().0.contains("fn_checkout"),
        "contract_ref must be derived from node name, got {:?}",
        contract_ref
    );
}

// RED → GREEN: lower_to_anf_with_graph threads effect_ref from
// the first effect in GraphNode.effect_row.effects.
#[test]
fn lower_to_anf_with_graph_threads_effect_ref_from_effect_row() {
    let graph = graph_with_rich_provenance();
    let core = lower_to_core_ir(&graph, &proven_report()).expect("lower_to_core_ir");
    let anf = lower_to_anf_with_graph(&core, &graph).expect("lower_to_anf_with_graph");
    let effect_ref = &anf.source_map.entries[0].effect_ref;
    assert!(
        effect_ref.is_some(),
        "effect_ref must be Some when GraphNode has effect_row"
    );
    assert_eq!(
        effect_ref.as_ref().unwrap().0,
        "database.read",
        "effect_ref must be threaded from the first declared effect"
    );
}

// RED → GREEN: lower_to_anf_with_graph threads runtime_check_ref from
// the first RuntimeCheckMeta.hash in GraphNode.runtime_checks.
#[test]
fn lower_to_anf_with_graph_threads_runtime_check_ref_from_checks() {
    let graph = graph_with_rich_provenance();
    let core = lower_to_core_ir(&graph, &proven_report()).expect("lower_to_core_ir");
    let anf = lower_to_anf_with_graph(&core, &graph).expect("lower_to_anf_with_graph");
    let rt_ref = &anf.source_map.entries[0].runtime_check_ref;
    assert!(
        rt_ref.is_some(),
        "runtime_check_ref must be Some when GraphNode has runtime_checks"
    );
    assert_eq!(
        rt_ref.as_ref().unwrap().0,
        "rtcheck_hash_abc123",
        "runtime_check_ref must be threaded from the first runtime_check hash"
    );
}

// RED → GREEN: Module nodes receive a block_ref (block identity).
#[test]
fn lower_to_anf_with_graph_sets_block_ref_for_module_nodes() {
    let graph = graph_with_module_and_function();
    let core = lower_to_core_ir(&graph, &proven_report()).expect("lower_to_core_ir");
    let anf = lower_to_anf_with_graph(&core, &graph).expect("lower_to_anf_with_graph");
    // Entry 0 is the Module node "mod_core".
    let block_ref = &anf.source_map.entries[0].block_ref;
    assert!(block_ref.is_some(), "Module node must receive a block_ref");
    assert!(
        block_ref.as_ref().unwrap().0.contains("mod_core"),
        "block_ref must be derived from module name, got {:?}",
        block_ref
    );
}

// RED → GREEN: Function nodes without explicit block metadata have block_ref = None.
#[test]
fn lower_to_anf_with_graph_leaves_block_ref_none_for_function_nodes() {
    let graph = graph_with_module_and_function();
    let core = lower_to_core_ir(&graph, &proven_report()).expect("lower_to_core_ir");
    let anf = lower_to_anf_with_graph(&core, &graph).expect("lower_to_anf_with_graph");
    // Entry 1 is the Function node "fn_run" (no block_ref expected).
    let block_ref = &anf.source_map.entries[1].block_ref;
    assert!(
        block_ref.is_none(),
        "Function node without block metadata must have block_ref = None, got {:?}",
        block_ref
    );
}

// RED → GREEN: Nodes without provenance metadata yield None for all
// optional provenance fields.
#[test]
fn lower_to_anf_with_graph_leaves_none_when_no_graph_data() {
    // Simple graph: one Function node with no provenance metadata at all.
    let graph = SemanticGraph {
        nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Function, "fn_bare")],
        edges: vec![],
    };
    let core = lower_to_core_ir(&graph, &proven_report()).expect("lower_to_core_ir");
    let anf = lower_to_anf_with_graph(&core, &graph).expect("lower_to_anf_with_graph");
    let entry = &anf.source_map.entries[0];
    assert!(
        entry.change_set.is_none(),
        "change_set must be None when no provenance on node"
    );
    assert!(
        entry.block_ref.is_none(),
        "block_ref must be None for Function without block data"
    );
    assert!(
        entry.contract_ref.is_none(),
        "contract_ref must be None when no contract_clauses"
    );
    assert!(
        entry.effect_ref.is_none(),
        "effect_ref must be None when no effect_row"
    );
    assert!(
        entry.runtime_check_ref.is_none(),
        "runtime_check_ref must be None when no runtime_checks"
    );
    assert!(
        entry.proof_obligation_ref.is_none(),
        "proof_obligation_ref must be None when node has no refinement_ref or Proves edge"
    );
}

// RED → GREEN: lower_to_anf_with_graph threads proof_obligation_ref from
// GraphNode.refinement_ref as "proof.refinement.<node_name>".
#[test]
fn lower_to_anf_with_graph_threads_proof_obligation_ref_from_refinement() {
    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn_balance");
    node.refinement_ref = Some(RefinementRef {
        base_type: "Int".to_string(),
        predicate: "value >= 0".to_string(),
        status: RefinementStatus::Proven,
        erased: false,
    });
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let core = lower_to_core_ir(&graph, &proven_report()).expect("lower_to_core_ir");
    let anf = lower_to_anf_with_graph(&core, &graph).expect("lower_to_anf_with_graph");

    let proof_ref = anf.source_map.entries[0].proof_obligation_ref.as_ref();
    assert!(
        proof_ref.is_some(),
        "proof_obligation_ref must be Some when GraphNode has refinement_ref"
    );
    assert_eq!(
        proof_ref.unwrap().0,
        "proof.refinement.fn_balance",
        "proof_obligation_ref must be derived as 'proof.refinement.<node_name>'"
    );
}

// RED → GREEN: lower_to_anf_with_graph threads proof_obligation_ref from a
// Proves edge as "proof.<target_node_name>" when no refinement_ref exists.
#[test]
fn lower_to_anf_with_graph_threads_proof_obligation_ref_from_proves_edge() {
    let source_node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn_transfer");
    let target_node = GraphNode::new(NodeRef(1), NodeKind::Contract, "invariant.no_negative");
    let proves_edge = GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Proves);
    let graph = SemanticGraph {
        nodes: vec![source_node, target_node],
        edges: vec![proves_edge],
    };
    let core = lower_to_core_ir(&graph, &proven_report()).expect("lower_to_core_ir");
    let anf = lower_to_anf_with_graph(&core, &graph).expect("lower_to_anf_with_graph");

    // Entry 0 is fn_transfer (source of the Proves edge).
    let proof_ref = anf.source_map.entries[0].proof_obligation_ref.as_ref();
    assert!(
        proof_ref.is_some(),
        "proof_obligation_ref must be Some for a node that is the source of a Proves edge"
    );
    assert_eq!(
        proof_ref.unwrap().0,
        "proof.invariant.no_negative",
        "proof_obligation_ref must be 'proof.<target_name>' from the Proves edge"
    );
    // Entry 1 (target node) has no proves edge and no refinement_ref → None.
    assert!(
        anf.source_map.entries[1].proof_obligation_ref.is_none(),
        "target of a Proves edge must not receive proof_obligation_ref itself"
    );
}

// TRIANGULATE: refinement_ref takes priority over Proves edge for
// proof_obligation_ref when both are present on the same node.
#[test]
fn proof_obligation_ref_refinement_takes_priority_over_proves_edge() {
    let mut source_node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn_dual");
    source_node.refinement_ref = Some(RefinementRef {
        base_type: "Int".to_string(),
        predicate: "x > 0".to_string(),
        status: RefinementStatus::Unverified,
        erased: false,
    });
    let target_node = GraphNode::new(NodeRef(1), NodeKind::Contract, "contract.something");
    let proves_edge = GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Proves);
    let graph = SemanticGraph {
        nodes: vec![source_node, target_node],
        edges: vec![proves_edge],
    };
    let core = lower_to_core_ir(&graph, &proven_report()).expect("lower_to_core_ir");
    let anf = lower_to_anf_with_graph(&core, &graph).expect("lower_to_anf_with_graph");

    let proof_ref = anf.source_map.entries[0].proof_obligation_ref.as_ref();
    assert!(proof_ref.is_some());
    assert_eq!(
        proof_ref.unwrap().0,
        "proof.refinement.fn_dual",
        "refinement_ref must take priority over Proves edge for proof_obligation_ref"
    );
}

// TRIANGULATE: lower_to_anf_with_graph produces same hash as lower_to_anf
// for nodes without provenance (provenance fields are Option, CBOR-skipped).
// The anf_ir_hash covers only the bindings, not the source map, so hashes
// must be equal regardless of whether provenance is populated.
#[test]
fn lower_to_anf_with_graph_same_anf_ir_hash_as_lower_to_anf_for_plain_nodes() {
    let graph = SemanticGraph {
        nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Function, "fn_x")],
        edges: vec![],
    };
    let core = lower_to_core_ir(&graph, &proven_report()).expect("lower_to_core_ir");
    let anf_plain = lower_to_anf(&core).expect("lower_to_anf");
    let anf_with_graph = lower_to_anf_with_graph(&core, &graph).expect("lower_to_anf_with_graph");
    // anf_ir_hash covers bindings (not source map provenance), so must be identical.
    assert_eq!(
        anf_plain.stage_hashes.anf_ir_hash, anf_with_graph.stage_hashes.anf_ir_hash,
        "anf_ir_hash must be identical for the same bindings regardless of provenance enrichment"
    );
}

#[test]
fn emit_wasm_sidecar_preserves_enriched_source_map_provenance() {
    let graph = graph_with_rich_provenance();
    let core = lower_to_core_ir(&graph, &proven_report()).expect("lower_to_core_ir");
    let anf = lower_to_anf_with_graph(&core, &graph).expect("lower_to_anf_with_graph");
    let artifact = emit_wasm(&anf).expect("emit_wasm");

    let entry = artifact
        .source_map
        .entries
        .first()
        .expect("source map entry must exist");
    assert_eq!(entry.change_set.as_deref(), Some("change.add_checkout"));
    assert_eq!(
        entry.effect_ref.as_ref().map(|r| r.0.as_str()),
        Some("database.read")
    );
    assert_eq!(
        entry.runtime_check_ref.as_ref().map(|r| r.0.as_str()),
        Some("rtcheck_hash_abc123")
    );
    assert!(
        entry.wasm_offset.is_some(),
        "WASM backend must add wasm_offset"
    );

    let sidecar: ail_compiler::SourceMap = serde_json::from_slice(&artifact.source_map_json)
        .expect("source_map_json must decode to SourceMap");
    assert_eq!(sidecar, artifact.source_map);
}

#[test]
fn emit_native_sidecar_preserves_enriched_source_map_provenance() {
    let graph = graph_with_rich_provenance();
    let core = lower_to_core_ir(&graph, &proven_report()).expect("lower_to_core_ir");
    let anf = lower_to_anf_with_graph(&core, &graph).expect("lower_to_anf_with_graph");
    let artifact = emit_native(&anf).expect("emit_native");

    let entry = artifact
        .source_map
        .entries
        .first()
        .expect("source map entry must exist");
    assert_eq!(entry.change_set.as_deref(), Some("change.add_checkout"));
    assert_eq!(
        entry.contract_ref.as_ref().map(|r| r.0.as_str()),
        Some("contract.fn_checkout")
    );
    assert!(
        entry.native_offset.is_some(),
        "native backend must add native_offset"
    );

    let sidecar: ail_compiler::SourceMap = serde_json::from_slice(&artifact.source_map_json)
        .expect("source_map_json must decode to SourceMap");
    assert_eq!(sidecar, artifact.source_map);
}

#[test]
fn emit_wasm_preserves_boundary_audit_provenance() {
    let graph = graph_with_boundary_provenance();
    let core = lower_to_core_ir(&graph, &proven_report()).expect("lower_to_core_ir");
    let anf = lower_to_anf_with_graph(&core, &graph).expect("lower_to_anf_with_graph");
    let artifact = emit_wasm_with_profile(&anf, "prod").expect("emit_wasm_with_profile");

    let entry = artifact
        .source_map
        .entries
        .first()
        .expect("source map entry must exist");
    assert_eq!(entry.node_id, NodeRef(10));
    assert_eq!(entry.change_set.as_deref(), Some("change.public_boundary"));
    assert_eq!(
        entry.block_ref.as_ref().map(|r| r.0.as_str()),
        Some("block.public_api")
    );
    assert_eq!(
        entry.contract_ref.as_ref().map(|r| r.0.as_str()),
        Some("contract.public_api")
    );
    assert!(entry.wasm_offset.is_some(), "WASM offset must be retained");
}

#[test]
fn emit_native_preserves_boundary_provenance_and_capability_source_ref() {
    let graph = graph_with_boundary_provenance();
    let core = lower_to_core_ir(&graph, &proven_report()).expect("lower_to_core_ir");
    let anf = lower_to_anf_with_graph(&core, &graph).expect("lower_to_anf_with_graph");
    let artifact = emit_native_with_profile(&anf, "critical").expect("emit_native_with_profile");

    let entry = artifact
        .source_map
        .entries
        .first()
        .expect("source map entry must exist");
    assert_eq!(entry.change_set.as_deref(), Some("change.public_boundary"));
    assert_eq!(
        entry.block_ref.as_ref().map(|r| r.0.as_str()),
        Some("block.public_api")
    );
    assert!(
        entry.native_offset.is_some(),
        "native offset must be retained"
    );

    let capability = artifact
        .capabilities_manifest
        .entries
        .first()
        .expect("capability manifest entry must exist");
    assert_eq!(capability.source_ref, NodeRef(10));
}

#[test]
fn emit_wasm_prod_rejects_missing_change_set_provenance() {
    let anf = anf_for_n(1);
    let result = emit_wasm_with_profile(&anf, "prod");

    assert!(
        matches!(
            result,
            Err(CompileError::MissingProvenanceMetadata {
                field: "change_set",
                ..
            })
        ),
        "prod WASM emit must reject missing change_set provenance, got {result:?}"
    );
}

#[test]
fn emit_native_critical_rejects_missing_change_set_provenance() {
    let anf = anf_for_n(1);
    let result = emit_native_with_profile(&anf, "critical");

    assert!(
        matches!(
            result,
            Err(CompileError::MissingProvenanceMetadata {
                field: "change_set",
                ..
            })
        ),
        "critical native emit must reject missing change_set provenance, got {result:?}"
    );
}

#[test]
fn emit_wasm_prod_rejects_empty_source_map_with_bindings() {
    let mut anf = proven_anf_for_n(1);
    anf.source_map = SourceMap { entries: vec![] };
    let result = emit_wasm_with_profile(&anf, "prod");

    assert!(
        matches!(
            result,
            Err(CompileError::MissingProvenanceMetadata {
                field: "source_map_coverage",
                ..
            })
        ),
        "prod WASM emit must reject empty source map with bindings, got {result:?}"
    );
}

#[test]
fn emit_native_critical_rejects_short_source_map_with_bindings() {
    let mut anf = proven_anf_for_n(2);
    anf.source_map.entries.pop();
    let result = emit_native_with_profile(&anf, "critical");

    assert!(
        matches!(
            result,
            Err(CompileError::MissingProvenanceMetadata {
                field: "source_map_coverage",
                ..
            })
        ),
        "critical native emit must reject short source map with bindings, got {result:?}"
    );
}

#[test]
fn emit_wasm_production_rejects_mismatched_source_map_binding_name() {
    let mut anf = proven_anf_for_n(1);
    anf.source_map.entries[0].binding_name = "fn_wrong".to_string();
    let result = emit_wasm_with_profile(&anf, "production");

    assert!(
        matches!(
            result,
            Err(CompileError::MissingProvenanceMetadata {
                field: "binding_name",
                ..
            })
        ),
        "production WASM emit must reject mismatched binding_name, got {result:?}"
    );
}

#[test]
fn emit_native_prod_rejects_mismatched_source_map_node_id() {
    let mut anf = proven_anf_for_n(1);
    anf.source_map.entries[0].node_id = NodeRef(99);
    let result = emit_native_with_profile(&anf, "prod");

    assert!(
        matches!(
            result,
            Err(CompileError::MissingProvenanceMetadata {
                field: "node_id",
                ..
            })
        ),
        "prod native emit must reject mismatched node_id, got {result:?}"
    );
}
