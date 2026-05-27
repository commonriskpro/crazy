use std::collections::BTreeMap;

use ail_core::semantic_graph::{NodeKind, NodeRef, SemanticGraph};

use crate::report::{VerificationEntry, VerificationState};

use crate::type_checker::{
    E_BOUNDARY_INFERENCE_MISMATCH, E_BOUNDARY_NOT_MATERIALIZED, E_FOREIGN_TYPE_NO_SCHEMA,
    TypeContext,
};

use super::helpers::infer_expr_type;

// ── Subpass 8: Boundary/inference materialization ────────────────────────

/// Check that Function nodes with declared params also declare a return type.
///
/// The `inferred_returns` map is produced by [`infer_local_types`] and treated
/// as equivalent to a declared return type for the purpose of this check.
pub(crate) fn check_boundary_materialization(
    graph: &SemanticGraph,
    inferred_returns: &BTreeMap<NodeRef, String>,
    entries: &mut Vec<VerificationEntry>,
) {
    for node in &graph.nodes {
        if node.kind != NodeKind::Function {
            continue;
        }
        let Some(params) = &node.params else {
            continue;
        };
        if params.is_empty() {
            continue;
        }
        let scope = node.name.clone();

        let has_return = node.return_type.is_some() || inferred_returns.contains_key(&node.id);

        if !has_return {
            entries.push(VerificationEntry {
                claim: "boundary-materialization".into(),
                state: VerificationState::Unverified,
                scope,
                evidence: Some(format!(
                    "{E_BOUNDARY_NOT_MATERIALIZED}: function '{}' declares params \
                     but has no return_type; boundary signature is not fully materialized \
                     in the canonical graph",
                    node.name
                )),
                blocking: false,
                repair_options: vec![],
            });
        } else {
            entries.push(VerificationEntry {
                claim: "boundary-materialization".into(),
                state: VerificationState::Proven,
                scope,
                evidence: None,
                blocking: false,
                repair_options: vec![],
            });
        }
    }
}

// ── Local type inference pre-pass ────────────────────────────────────────

/// Infer return types for `Function` nodes that lack an explicit `return_type`
/// but carry a `body_expr`.  Local inference only — no global unification.
pub(crate) fn infer_local_types(graph: &SemanticGraph) -> BTreeMap<NodeRef, String> {
    let ctx = TypeContext::collect(graph);
    let mut map: BTreeMap<NodeRef, String> = BTreeMap::new();

    for node in &graph.nodes {
        if node.kind != NodeKind::Function || node.return_type.is_some() {
            continue;
        }
        let Some(body) = &node.body_expr else {
            continue;
        };
        if let Some(ty) = infer_expr_type(body.trim(), &ctx) {
            map.insert(node.id, ty);
        }
    }

    map
}
// ── Subpass 13: Boundary inference cross-check ───────────────────────────

pub(crate) fn check_boundary_inference(
    graph: &SemanticGraph,
    entries: &mut Vec<VerificationEntry>,
) {
    for node in &graph.nodes {
        if node.kind != NodeKind::Function {
            continue;
        }
        for fact in &node.inferred {
            if fact.kind != "boundary" {
                continue;
            }
            let Some(claimed_return) = fact.value.strip_prefix("return:") else {
                continue;
            };
            let scope = node.name.clone();
            let declared = node.return_type.as_deref().unwrap_or("");
            if claimed_return == declared {
                entries.push(VerificationEntry {
                    claim: "boundary-inference".into(),
                    state: VerificationState::Proven,
                    scope,
                    evidence: None,
                    blocking: false,
                    repair_options: vec![],
                });
            } else {
                entries.push(VerificationEntry {
                    claim: "boundary-inference".into(),
                    state: VerificationState::Failed,
                    scope,
                    evidence: Some(format!(
                        "{E_BOUNDARY_INFERENCE_MISMATCH}: boundary inferred return \
                         type '{claimed_return}' does not match declared return_type \
                         '{declared}' on '{}'",
                        node.name
                    )),
                    blocking: true,
                    repair_options: vec![],
                });
            }
        }
    }
}
// ── Subpass 16: ForeignType boundary schema enforcement ──────────────────

pub(crate) fn check_boundary_schema(graph: &SemanticGraph, entries: &mut Vec<VerificationEntry>) {
    for node in &graph.nodes {
        if node.kind != NodeKind::Function {
            continue;
        }
        let Some(return_type) = &node.return_type else {
            continue;
        };
        if !return_type.starts_with("ForeignType") {
            continue;
        }

        let has_schema = node
            .inferred
            .iter()
            .any(|fact| fact.kind == "boundary-schema");

        let scope = node.name.clone();
        if has_schema {
            entries.push(VerificationEntry {
                claim: "boundary-schema".into(),
                state: VerificationState::Proven,
                scope,
                evidence: None,
                blocking: false,
                repair_options: vec![],
            });
        } else {
            entries.push(VerificationEntry {
                claim: "boundary-schema".into(),
                state: VerificationState::Failed,
                scope,
                evidence: Some(format!(
                    "{E_FOREIGN_TYPE_NO_SCHEMA}: function '{}' returns a ForeignType \
                     ('{}') but has no 'boundary-schema' inferred fact; \
                     foreign values crossing boundaries must declare a serialization schema",
                    node.name, return_type
                )),
                blocking: true,
                repair_options: vec![],
            });
        }
    }
}
