// ── ail-context::assembly ─────────────────────────────────────────────────
//
// Response assembly helpers extracted from builder.rs.
//
// These helpers are `pub(crate)` — the public API remains in `builder.rs`.

use ail_core::semantic_graph::GraphNode;
use ail_storage::codec::ContentCodec;

use crate::dto::{IndexInfo, ProvenanceBlock};
use crate::error::{ContextError, ContextResult};

// ── BudgetResult ──────────────────────────────────────────────────────────

/// Output of the greedy budget-accumulation pass in `apply_budget`.
pub(crate) struct BudgetResult {
    /// Nodes that fit within the byte budget (sorted by `NodeRef`).
    pub(crate) structured: Vec<GraphNode>,
    /// Total bytes consumed by the accumulated nodes.
    pub(crate) bytes_used: usize,
    /// `true` when the budget was exhausted before all nodes were consumed.
    pub(crate) truncated: bool,
    /// Sections omitted due to truncation.
    ///
    /// Always `["structured_nodes"]` when `truncated` is `true`.
    pub(crate) omitted_sections: Vec<String>,
}

// ── apply_budget ──────────────────────────────────────────────────────────

/// Greedily accumulate `nodes` until the byte budget is exhausted.
///
/// Each node is CBOR-encoded individually; the first node that would push
/// `bytes_used` past `budget` stops accumulation and sets `truncated = true`.
///
/// # Errors
///
/// Returns `ContextError::Codec` if encoding a node fails.
pub(crate) fn apply_budget<C: ContentCodec>(
    nodes: Vec<GraphNode>,
    budget: usize,
    codec: &C,
) -> ContextResult<BudgetResult> {
    let mut structured: Vec<GraphNode> = Vec::new();
    let mut bytes_used: usize = 0;
    let mut truncated = false;
    let mut omitted_sections: Vec<String> = Vec::new();

    for node in nodes {
        let node_bytes = codec
            .encode(&node)
            .map_err(|e| ContextError::Codec(e.to_string()))?;
        if bytes_used + node_bytes.len() > budget {
            truncated = true;
            omitted_sections.push("structured_nodes".to_string());
            break;
        }
        bytes_used += node_bytes.len();
        structured.push(node);
    }

    Ok(BudgetResult {
        structured,
        bytes_used,
        truncated,
        omitted_sections,
    })
}

// ── assemble_provenance ───────────────────────────────────────────────────

/// Build a `ProvenanceBlock`, ensuring `"semantic_graph"` is always the
/// first source entry, then preserving any extra `provenance_sources`.
///
/// `verification_report_hash` is appended to `reports` when `Some`.
pub(crate) fn assemble_provenance(
    provenance_sources: &[String],
    index_info: &[IndexInfo],
    verification_report_hash: Option<[u8; 32]>,
) -> ProvenanceBlock {
    let mut provenance = ProvenanceBlock {
        sources: provenance_sources.to_vec(),
        indexes: index_info.to_vec(),
        reports: Vec::new(),
    };
    if !provenance.sources.iter().any(|s| s == "semantic_graph") {
        provenance.sources.insert(0, "semantic_graph".to_string());
    }
    if let Some(report_hash) = verification_report_hash {
        provenance.reports.push(report_hash);
    }
    provenance
}
