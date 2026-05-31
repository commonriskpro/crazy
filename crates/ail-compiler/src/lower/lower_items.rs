// ── ail-compiler::lower::lower_items ─────────────────────────────────────
//
// Node, type, and provenance lowering helpers.
//
// Covers: `NodeKind → CoreNodeKind` mapping, `nominal → CoreType` mapping,
// `CoreNode → AnfBinding` lowering, graph-node body parsing, and source-map
// provenance extraction.  All functions are re-exported through `lower.rs`.

use std::collections::BTreeMap;

use ail_core::semantic_graph::{
    BlockRef, ContractRef, EdgeKind, EffectRef, NodeKind, NodeRef, ProofObligationRef,
    RuntimeCheckRef, SemanticGraph,
};

use crate::anf::{AnfBinding, AnfExpr, SourceMap, SourceMapEntry, SourceMapSpan};
use crate::core_ir::{CoreExpr, CoreNode, CoreNodeKind, CoreType, LiteralValue, ResourceMode};
use crate::error::CompileError;
use crate::expr_parser::parse_expr;

use super::lower_expr::lower_core_expr_to_anf_local;

// ── map_node_kind ─────────────────────────────────────────────────────────

/// Map a `NodeKind` (source graph) to its `CoreNodeKind` counterpart.
///
/// In Phase 7 the two enums are structurally identical; the mapping is kept
/// explicit so the compiler IR can diverge from the source model in future
/// phases without a breaking change here.
pub(crate) fn map_node_kind(kind: NodeKind) -> CoreNodeKind {
    match kind {
        NodeKind::Module => CoreNodeKind::Module,
        NodeKind::Function => CoreNodeKind::Function,
        NodeKind::Type => CoreNodeKind::Type,
        NodeKind::Effect => CoreNodeKind::Effect,
        NodeKind::Capability => CoreNodeKind::Capability,
        NodeKind::Contract => CoreNodeKind::Contract,
        NodeKind::Invariant => CoreNodeKind::Invariant,
        NodeKind::Test => CoreNodeKind::Test,
        NodeKind::Boundary => CoreNodeKind::Boundary,
        NodeKind::Package => CoreNodeKind::Package,
        NodeKind::Interface => CoreNodeKind::Interface,
        NodeKind::Impl => CoreNodeKind::Impl,
        NodeKind::EffectAlias => CoreNodeKind::EffectAlias,
        NodeKind::Import => CoreNodeKind::Import,
        NodeKind::Export => CoreNodeKind::Export,
        NodeKind::VersionConstraint => CoreNodeKind::VersionConstraint,
        NodeKind::CapabilityExport => CoreNodeKind::CapabilityExport,
        NodeKind::ContractExport => CoreNodeKind::ContractExport,
    }
}

// ── map_core_node_to_anf ──────────────────────────────────────────────────

/// Lower one `CoreNode` into one or more `AnfBinding`s.
///
/// If the node has a `CoreExpr` body, inline temporaries are kept inside the
/// node binding as local `let` expressions so executable function parameters do
/// not accidentally become global synthetic bindings.
///
/// Nodes without `expr` (modules, types, capabilities, etc.) get a default
/// `AnfExpr::Literal(LiteralValue::Unit)`.
///
/// Provenance (`source_ref`) is preserved verbatim on every emitted binding.
pub(crate) fn map_core_node_to_anf(node: &CoreNode, fresh: &mut u32, out: &mut Vec<AnfBinding>) {
    let anf_expr = match &node.expr {
        Some(core_expr) => lower_core_expr_to_anf_local(core_expr, fresh, node.source_ref),
        None => AnfExpr::Literal(LiteralValue::Unit),
    };
    out.push(AnfBinding {
        source_ref: node.source_ref,
        name: node.name.clone(),
        expr: anf_expr,
    });
}

// ── nominal_to_core_type ─────────────────────────────────────────────────

/// Map a `TypeFacts.nominal` string to a `CoreType` variant.
///
/// Recognised nominals correspond to the 20 type primitives listed in
/// `docs/core-ir.md §3`.  Any unrecognised nominal falls back to
/// `CoreType::Generic(None)`.
pub fn nominal_to_core_type(nominal: &str) -> CoreType {
    match nominal {
        "Unit" => CoreType::Unit,
        "Never" => CoreType::Never,
        "Bool" => CoreType::Bool,
        "Int" => CoreType::Int,
        "UInt" => CoreType::UInt,
        "Float" => CoreType::Float,
        "Text" => CoreType::Text,
        "Bytes" => CoreType::Bytes,
        "Record" => CoreType::Record,
        "Variant" => CoreType::Variant,
        "Tuple" => CoreType::Tuple,
        // Parameterized variants: inner type defaults to Generic when only the
        // nominal name is available (full resolution requires type-param phase).
        "List" => CoreType::List(Box::new(CoreType::Generic(None))),
        "Map" => CoreType::Map(
            Box::new(CoreType::Generic(None)),
            Box::new(CoreType::Generic(None)),
        ),
        "Set" => CoreType::Set(Box::new(CoreType::Generic(None))),
        "Option" => CoreType::Option(Box::new(CoreType::Generic(None))),
        "Result" => CoreType::Result(
            Box::new(CoreType::Generic(None)),
            Box::new(CoreType::Generic(None)),
        ),
        "Function" => CoreType::Function {
            params: vec![],
            ret: Box::new(CoreType::Generic(None)),
            effects: vec![],
        },
        "Handle" => CoreType::Handle {
            resource: Box::new(CoreType::Generic(None)),
            mode: ResourceMode::Copy,
        },
        "Refinement" => CoreType::Refinement {
            base: Box::new(CoreType::Generic(None)),
            predicate: String::new(),
        },
        "Generic" => CoreType::Generic(None),
        _ => CoreType::Generic(None),
    }
}

// ── graph node body helpers ───────────────────────────────────────────────

pub(super) fn literal_expr_from_runtime_checks(
    checks: Option<&Vec<ail_core::semantic_graph::RuntimeCheckMeta>>,
) -> Option<CoreExpr> {
    let predicate = checks?.first()?.predicate.strip_prefix("literal:i64=")?;
    let value = predicate.parse::<i64>().ok()?;
    Some(CoreExpr::Literal(LiteralValue::Int(value)))
}

pub(super) fn expr_from_graph_node(
    node: &ail_core::semantic_graph::GraphNode,
) -> Result<Option<CoreExpr>, CompileError> {
    if let Some(body) = &node.body_expr {
        if body.trim_start().starts_with('@') {
            return Ok(None);
        }
        return parse_expr(body)
            .map(Some)
            .map_err(|err| CompileError::InvalidGraph(format!("{} body: {err}", node.name)));
    }
    Ok(literal_expr_from_runtime_checks(
        node.runtime_checks.as_ref(),
    ))
}

// ── Semantic provenance extraction ───────────────────────────────────────

/// Node-level provenance data extracted from a `GraphNode` for source-map
/// enrichment.  All fields are `Option` because graph nodes are not required
/// to carry this metadata.
pub(super) struct NodeProvenance {
    /// From `GraphNode.provenance.change_id` — the `ChangeSet` that last
    /// created or modified this node.
    pub(super) change_set: Option<String>,
    /// Derived block identity: `Some(format!("block.{name}"))` for
    /// `Module` / `Boundary` nodes; `None` for all other kinds.
    pub(super) block_ref: Option<String>,
    /// Derived contract ref: `Some(format!("contract.{name}"))` when the
    /// node has `contract_clauses`; `None` otherwise.
    pub(super) contract_ref: Option<String>,
    /// First declared effect from `GraphNode.effect_row.effects`, if any.
    pub(super) effect_ref: Option<String>,
    /// Proof obligation ref derived from the node.
    ///
    /// Populated from two sources (first wins):
    /// 1. `GraphNode.refinement_ref` → `"proof.refinement.<node_name>"`
    /// 2. A `Proves` edge from this node → `"proof.<target_name>"`
    pub(super) proof_obligation_ref: Option<String>,
    /// Content hash of the first `RuntimeCheckMeta` in
    /// `GraphNode.runtime_checks`, if any.
    pub(super) runtime_check_ref: Option<String>,
    /// Authored source span copied from `GraphNode.span`, if present.
    pub(super) source_span: Option<SourceMapSpan>,
}

/// Build a `NodeRef → NodeProvenance` lookup from a `SemanticGraph`.
///
/// Used by `lower_to_anf_with_graph` to enrich `SourceMapEntry` fields
/// without changing the `lower_to_anf` public API.
///
/// `proof_obligation_ref` is populated from two sources (first wins):
/// 1. `GraphNode.refinement_ref` — any node with a refinement predicate
///    gets `"proof.refinement.<node_name>"`.
/// 2. A `Proves` edge from the node — the first `Proves` edge whose source
///    matches the node id produces `"proof.<target_name>"`.
pub(super) fn extract_provenance_lookup(
    graph: &SemanticGraph,
) -> BTreeMap<NodeRef, NodeProvenance> {
    // First pass: collect proof refs from `Proves` edges.
    // source_node → "proof.<target_node_name>" (first edge wins).
    let mut edge_proof_refs: BTreeMap<NodeRef, String> = BTreeMap::new();
    for edge in &graph.edges {
        if edge.kind == EdgeKind::Proves
            && let Some(target) = graph.nodes.iter().find(|n| n.id == edge.target)
        {
            edge_proof_refs
                .entry(edge.source)
                .or_insert_with(|| format!("proof.{}", target.name));
        }
    }

    graph
        .nodes
        .iter()
        .map(|gn| {
            // Refinement-based proof ref takes priority over edge-derived one.
            let proof_obligation_ref = gn
                .refinement_ref
                .as_ref()
                .map(|_| format!("proof.refinement.{}", gn.name))
                .or_else(|| edge_proof_refs.get(&gn.id).cloned());

            let prov = NodeProvenance {
                change_set: gn.provenance.as_ref().map(|p| p.change_id.clone()),
                block_ref: match gn.kind {
                    NodeKind::Module | NodeKind::Boundary => Some(format!("block.{}", gn.name)),
                    _ => None,
                },
                contract_ref: gn
                    .contract_clauses
                    .as_ref()
                    .map(|_| format!("contract.{}", gn.name)),
                effect_ref: gn
                    .effect_row
                    .as_ref()
                    .and_then(|er| er.effects.first().cloned()),
                proof_obligation_ref,
                runtime_check_ref: gn
                    .runtime_checks
                    .as_ref()
                    .and_then(|rcs| rcs.first())
                    .map(|rc| rc.hash.clone()),
                source_span: gn.span.as_ref().map(SourceMapSpan::from_graph_span),
            };
            (gn.id, prov)
        })
        .collect()
}

/// Build an enriched `SourceMap` from ANF bindings and a provenance lookup.
///
/// Each `SourceMapEntry` is populated with the provenance fields available
/// in the lookup for the binding's `source_ref`.  Fields for which no
/// upstream data exists remain `None`.
///
/// `proof_obligation_ref` is populated when the originating graph node carries
/// a refinement predicate or is the source of a `Proves` edge; otherwise `None`.
pub(super) fn build_enriched_source_map(
    bindings: &[AnfBinding],
    lookup: &BTreeMap<NodeRef, NodeProvenance>,
) -> SourceMap {
    let entries = bindings
        .iter()
        .map(|b| {
            let prov = lookup.get(&b.source_ref);
            SourceMapEntry {
                binding_name: b.name.clone(),
                node_id: b.source_ref,
                block_ref: prov
                    .and_then(|p| p.block_ref.as_ref())
                    .map(|s| BlockRef(s.clone())),
                change_set: prov.and_then(|p| p.change_set.clone()),
                contract_ref: prov
                    .and_then(|p| p.contract_ref.as_ref())
                    .map(|s| ContractRef(s.clone())),
                effect_ref: prov
                    .and_then(|p| p.effect_ref.as_ref())
                    .map(|s| EffectRef(s.clone())),
                proof_obligation_ref: prov
                    .and_then(|p| p.proof_obligation_ref.as_ref())
                    .map(|s| ProofObligationRef(s.clone())),
                runtime_check_ref: prov
                    .and_then(|p| p.runtime_check_ref.as_ref())
                    .map(|s| RuntimeCheckRef(s.clone())),
                source_span: prov.and_then(|p| p.source_span.clone()),
                generated_span: None,
                wasm_offset: None,
                native_offset: None,
            }
        })
        .collect();
    SourceMap { entries }
}
