// ── ail-context::assembly ─────────────────────────────────────────────────
//
// Response assembly helpers extracted from builder.rs.
//
// These helpers are `pub(crate)` — the public API remains in `builder.rs`.

use std::collections::{BTreeMap, BTreeSet};

use ail_core::semantic_graph::{GraphNode, NodeRef, SemanticGraph};
use ail_storage::codec::ContentCodec;

use crate::dto::{
    BundleDescriptor, BundleIssue, ISSUE_DUPLICATE_NODE_REF, ISSUE_MISSING_NODE_REF,
    ISSUE_UNSTABLE_INPUT_ORDER, IndexInfo, ProvenanceBlock,
};
use crate::error::{ContextError, ContextResult};

// ── CanonicalBundleNodes ─────────────────────────────────────────────────

/// Canonicalized candidate nodes plus deterministic diagnostics found while
/// preparing the response bundle.
pub(crate) struct CanonicalBundleNodes {
    /// Nodes sorted by `NodeRef`, with duplicate ties broken by canonical CBOR.
    pub(crate) nodes: Vec<GraphNode>,
    /// Stable diagnostics for duplicate or unstable candidate ordering.
    pub(crate) diagnostics: Vec<BundleIssue>,
}

// ── canonicalize_bundle_nodes ────────────────────────────────────────────

/// Sort candidate nodes into canonical bundle order.
///
/// Selection code already aims to emit `NodeRef` order.  This final pass is a
/// production guardrail: it makes output byte-stable even when a caller hands
/// us duplicate refs or non-canonical input order, and it records stable issue
/// codes so operators can diagnose the upstream graph problem.
pub(crate) fn canonicalize_bundle_nodes<C: ContentCodec>(
    nodes: Vec<GraphNode>,
    codec: &C,
) -> ContextResult<CanonicalBundleNodes> {
    let mut diagnostics = Vec::new();
    let mut counts: BTreeMap<NodeRef, usize> = BTreeMap::new();

    let mut keyed = Vec::with_capacity(nodes.len());
    let mut last_ref: Option<NodeRef> = None;
    let mut unstable_reported = false;

    for (ordinal, node) in nodes.into_iter().enumerate() {
        if let Some(prev) = last_ref {
            if prev > node.id && !unstable_reported {
                diagnostics.push(BundleIssue {
                    code: ISSUE_UNSTABLE_INPUT_ORDER.to_string(),
                    descriptor: BundleDescriptor {
                        node_ref: Some(node.id),
                        edge_source: None,
                        edge_target: None,
                        edge_kind: None,
                        ordinal: Some(ordinal),
                    },
                });
                unstable_reported = true;
            }
        }
        last_ref = Some(node.id);

        *counts.entry(node.id).or_insert(0) += 1;
        let encoded = codec
            .encode(&node)
            .map_err(|e| ContextError::Codec(e.to_string()))?;
        keyed.push((node.id, encoded, ordinal, node));
    }

    for (node_ref, count) in counts {
        if count > 1 {
            diagnostics.push(BundleIssue {
                code: ISSUE_DUPLICATE_NODE_REF.to_string(),
                descriptor: BundleDescriptor {
                    node_ref: Some(node_ref),
                    edge_source: None,
                    edge_target: None,
                    edge_kind: None,
                    ordinal: None,
                },
            });
        }
    }

    keyed.sort_by(
        |(left_ref, left_bytes, left_ordinal, _), (right_ref, right_bytes, right_ordinal, _)| {
            left_ref
                .cmp(right_ref)
                .then_with(|| left_bytes.cmp(right_bytes))
                .then_with(|| left_ordinal.cmp(right_ordinal))
        },
    );

    Ok(CanonicalBundleNodes {
        nodes: keyed.into_iter().map(|(_, _, _, node)| node).collect(),
        diagnostics,
    })
}

// ── diagnose_graph_manifest ──────────────────────────────────────────────

/// Validate the source graph manifest enough to make bundle issues diagnosable.
///
/// This is intentionally non-fatal: context consumers still receive the best
/// available slice, while operators get stable codes for bad graph manifests.
pub(crate) fn diagnose_graph_manifest(graph: &SemanticGraph) -> Vec<BundleIssue> {
    let mut diagnostics = Vec::new();
    let mut seen = BTreeSet::new();
    let mut duplicate_refs = BTreeSet::new();
    let mut last_ref: Option<NodeRef> = None;
    let mut unstable_reported = false;

    for (ordinal, node) in graph.nodes.iter().enumerate() {
        if let Some(prev) = last_ref {
            if prev > node.id && !unstable_reported {
                diagnostics.push(BundleIssue {
                    code: ISSUE_UNSTABLE_INPUT_ORDER.to_string(),
                    descriptor: BundleDescriptor {
                        node_ref: Some(node.id),
                        edge_source: None,
                        edge_target: None,
                        edge_kind: None,
                        ordinal: Some(ordinal),
                    },
                });
                unstable_reported = true;
            }
        }
        last_ref = Some(node.id);

        if !seen.insert(node.id) {
            duplicate_refs.insert(node.id);
        }
    }

    for node_ref in duplicate_refs {
        diagnostics.push(BundleIssue {
            code: ISSUE_DUPLICATE_NODE_REF.to_string(),
            descriptor: BundleDescriptor {
                node_ref: Some(node_ref),
                edge_source: None,
                edge_target: None,
                edge_kind: None,
                ordinal: None,
            },
        });
    }

    for edge in &graph.edges {
        if !seen.contains(&edge.source) {
            diagnostics.push(BundleIssue {
                code: ISSUE_MISSING_NODE_REF.to_string(),
                descriptor: BundleDescriptor {
                    node_ref: Some(edge.source),
                    edge_source: Some(edge.source),
                    edge_target: Some(edge.target),
                    edge_kind: Some(format!("{:?}", edge.kind)),
                    ordinal: None,
                },
            });
        }
        if !seen.contains(&edge.target) {
            diagnostics.push(BundleIssue {
                code: ISSUE_MISSING_NODE_REF.to_string(),
                descriptor: BundleDescriptor {
                    node_ref: Some(edge.target),
                    edge_source: Some(edge.source),
                    edge_target: Some(edge.target),
                    edge_kind: Some(format!("{:?}", edge.kind)),
                    ordinal: None,
                },
            });
        }
    }

    diagnostics
}

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
