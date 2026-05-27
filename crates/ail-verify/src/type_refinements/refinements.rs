use ail_core::semantic_graph::{EdgeKind, RefinementStatus, SemanticGraph};

use crate::report::{VerificationEntry, VerificationState};

use crate::type_checker::{
    E_ASSOC_TYPE_NOT_RESOLVED, E_REFINEMENT_ERASURE, E_REFINEMENT_PROOF_UNDISCHARGED,
    E_REFINEMENT_RUNTIME_CHECK_MISSING, TypeContext,
};

use super::helpers::{resolve_assoc_type, split_assoc_type};

// ── Subpass 7: Refinement proof obligations ──────────────────────────────

pub(crate) fn check_refinements(graph: &SemanticGraph, entries: &mut Vec<VerificationEntry>) {
    for node in &graph.nodes {
        let Some(rf) = &node.refinement_ref else {
            continue;
        };

        let mut state = match rf.status {
            RefinementStatus::Proven => VerificationState::Proven,
            RefinementStatus::RuntimeChecked => VerificationState::RuntimeChecked,
            RefinementStatus::Assumed => VerificationState::Assumed,
            RefinementStatus::Unverified => VerificationState::Unverified,
            RefinementStatus::Failed => VerificationState::Failed,
        };
        let mut evidence = format!("predicate: '{}'; base: '{}'", rf.predicate, rf.base_type);

        if matches!(rf.status, RefinementStatus::Proven) {
            match rf.predicate.trim() {
                "true" => {}
                "false" | "" => {
                    state = VerificationState::Failed;
                    evidence = format!(
                        "{E_REFINEMENT_PROOF_UNDISCHARGED}: proven refinement '{}' cannot be \
                         discharged locally",
                        rf.predicate
                    );
                }
                _ => {
                    if node.contract_clauses.as_ref().map(|clauses| {
                        clauses
                            .ensures
                            .iter()
                            .any(|p| p.trim() == rf.predicate.trim())
                    }) != Some(true)
                    {
                        state = VerificationState::Unverified;
                        evidence = format!(
                            "{E_REFINEMENT_PROOF_UNDISCHARGED}: refinement '{}' has no matching \
                             ensures clause or literal proof",
                            rf.predicate
                        );
                    }
                }
            }
        }

        if matches!(rf.status, RefinementStatus::RuntimeChecked)
            && node
                .runtime_checks
                .as_ref()
                .map(|checks| checks.is_empty())
                .unwrap_or(true)
        {
            state = VerificationState::Failed;
            evidence = format!(
                "{E_REFINEMENT_RUNTIME_CHECK_MISSING}: runtime-checked refinement '{}' has no \
                 materialized runtime check",
                rf.predicate
            );
        }

        entries.push(VerificationEntry {
            claim: "refinement".into(),
            state,
            scope: node.name.clone(),
            evidence: Some(evidence),
            blocking: false,
            repair_options: vec![],
        });

        if rf.erased {
            entries.push(VerificationEntry {
                claim: "refinement-erasure".into(),
                state: VerificationState::Assumed,
                scope: node.name.clone(),
                evidence: Some(format!(
                    "{E_REFINEMENT_ERASURE}: refinement '{}' \
                     erased to base type '{}'; \
                     erasure is explicit and tracked",
                    rf.predicate, rf.base_type
                )),
                blocking: false,
                repair_options: vec![],
            });
        }
    }
}
// ── Subpass 14: Associated type resolution ───────────────────────────────

pub(crate) fn check_associated_type_resolution(
    graph: &SemanticGraph,
    ctx: &TypeContext<'_>,
    entries: &mut Vec<VerificationEntry>,
) {
    for edge in &graph.edges {
        if edge.kind != EdgeKind::Calls {
            continue;
        }
        let Some(call_args) = &edge.call_args else {
            continue;
        };
        let Some(callee) = ctx.by_ref.get(&edge.target).copied() else {
            continue;
        };
        let Some(return_type) = &callee.return_type else {
            continue;
        };
        if !return_type.contains("::") {
            continue;
        }

        let scope = format!("{}→{}", edge.source.0, callee.name);

        let (interface_base, assoc_name) = split_assoc_type(return_type);

        let resolved = call_args
            .iter()
            .find_map(|arg_ty| resolve_assoc_type(ctx, arg_ty, interface_base, assoc_name));

        match resolved {
            Some(concrete_ty) => {
                entries.push(VerificationEntry {
                    claim: "assoc-type-resolution".into(),
                    state: VerificationState::Proven,
                    scope,
                    evidence: Some(format!("resolved '{return_type}' → '{concrete_ty}'")),
                    blocking: false,
                    repair_options: vec![],
                });
            }
            None => {
                entries.push(VerificationEntry {
                    claim: "assoc-type-resolution".into(),
                    state: VerificationState::Unverified,
                    scope,
                    evidence: Some(format!(
                        "{E_ASSOC_TYPE_NOT_RESOLVED}: \
                         associated type '{return_type}' on callee '{}' \
                         could not be resolved from call_args {call_args:?}",
                        callee.name
                    )),
                    blocking: false,
                    repair_options: vec![],
                });
            }
        }
    }
}
