// ── ail-verify::type_refinements ─────────────────────────────────────────
//
// Refinement, policy, boundary, generic-param, and effect subpasses for the
// type checker.
//
// # Subpasses contained here
//
// 3.  Generic param kind validation    — `check_generic_params`
// 3b. Call-site generic arity/binding  — `check_generic_call_bindings`
// 3c. Effect/capability propagation    — `check_effect_capability_propagation`
// 7.  Refinement proof obligations     — `check_refinements`
// 8.  Boundary materialization         — `check_boundary_materialization`
//     (with local type inference pre-pass: `infer_local_types`)
// 9.  Null/absence policy              — `check_null_policy`
// 10. Float equality/ordering policy   — `check_float_policy`
// 11. PatchField inner-type validation — `check_patchfield`
// 12. PartialOrd vs Ord distinction    — `check_partial_ord`
// 13. Boundary inference cross-check   — `check_boundary_inference`
// 14. Associated type resolution       — `check_associated_type_resolution`
// 15. ConstParam value validation      — `check_const_param_call_bindings`
// 16. ForeignType boundary schema      — `check_boundary_schema`
// 18. Effect/capability param threading— `check_effect_capability_param_threading`
//
// All functions are `pub(crate)` free functions extracted verbatim from
// `impl TypeChecker` — no behavior changes.

use std::collections::BTreeMap;

use ail_core::semantic_graph::{
    EdgeKind, GenericParamKind, NodeKind, NodeRef, RefinementStatus, SemanticGraph,
};

use crate::report::{VerificationEntry, VerificationState};
use crate::type_checker::{
    E_ASSOC_TYPE_NOT_RESOLVED, E_BOUNDARY_INFERENCE_MISMATCH, E_BOUNDARY_NOT_MATERIALIZED,
    E_CAPABILITY_NOT_PROPAGATED, E_CAPABILITY_PARAM_NOT_THREADED, E_CAPABILITY_PARAM_WIDENED,
    E_CONST_PARAM_UNDECIDABLE, E_CONST_PARAM_VALUE_INVALID, E_EFFECT_NOT_PROPAGATED,
    E_EFFECT_PARAM_NOT_THREADED, E_EFFECT_PARAM_WIDENED, E_FLOAT_EQ_IMPLICIT, E_FLOAT_ORD_IMPLICIT,
    E_FOREIGN_TYPE_NO_SCHEMA, E_GENERIC_BINDING_ARITY, E_NULL_IN_CORE_IR, E_PARTIAL_ORD_REQUIRED,
    E_PATCHFIELD_EMPTY_INNER, E_REFINEMENT_ERASURE, E_REFINEMENT_PROOF_UNDISCHARGED,
    E_REFINEMENT_RUNTIME_CHECK_MISSING, TypeContext,
};
use crate::type_obligations::split_generic;

// ── Subpass 3: Generic param kind validation ────────────────────────────

pub(crate) fn check_generic_params(graph: &SemanticGraph, entries: &mut Vec<VerificationEntry>) {
    for node in &graph.nodes {
        let Some(generic_params) = &node.generic_params else {
            continue;
        };
        for gp in generic_params {
            let scope = format!("{}::{}", node.name, gp.name);

            if gp.name.is_empty() {
                entries.push(VerificationEntry {
                    claim: "generic-param-kind".into(),
                    state: VerificationState::Failed,
                    scope,
                    evidence: Some(format!(
                        "E_GENERIC_ARITY: generic parameter name is empty on node '{}'",
                        node.name
                    )),
                    blocking: true,
                    repair_options: vec![],
                });
                continue;
            }

            let state_and_evidence: (VerificationState, Option<String>) = match gp.kind {
                GenericParamKind::EffectParam => {
                    let in_row = node
                        .effect_row
                        .as_ref()
                        .map(|er| er.effects.iter().any(|e| e == &gp.name))
                        .unwrap_or(false);
                    if in_row {
                        (VerificationState::Proven, None)
                    } else {
                        (
                            VerificationState::Failed,
                            Some(format!(
                                "{E_EFFECT_PARAM_WIDENED}: EffectParam '{}' \
                                 not found in effect_row of '{}'; \
                                 effect precision would be silently widened",
                                gp.name, node.name
                            )),
                        )
                    }
                }
                GenericParamKind::CapabilityParam => {
                    let in_caps = node
                        .capability_reqs
                        .as_ref()
                        .map(|cr| cr.caps.iter().any(|c| c == &gp.name))
                        .unwrap_or(false);
                    if in_caps {
                        (VerificationState::Proven, None)
                    } else {
                        (
                            VerificationState::Failed,
                            Some(format!(
                                "{E_CAPABILITY_PARAM_WIDENED}: CapabilityParam '{}' \
                                 not found in capability_reqs of '{}'; \
                                 capability precision would be silently widened",
                                gp.name, node.name
                            )),
                        )
                    }
                }
                GenericParamKind::TypeParam => (VerificationState::Proven, None),
                GenericParamKind::ConstParam => {
                    if is_simple_identifier(&gp.name) {
                        (VerificationState::Proven, None)
                    } else {
                        (
                            VerificationState::Failed,
                            Some(format!(
                                "{E_CONST_PARAM_UNDECIDABLE}: ConstParam '{}' \
                                 contains a complex expression; only simple \
                                 decidable identifiers are permitted",
                                gp.name
                            )),
                        )
                    }
                }
            };

            entries.push(VerificationEntry {
                claim: "generic-param-kind".into(),
                state: state_and_evidence.0,
                scope,
                evidence: state_and_evidence.1,
                blocking: false,
                repair_options: vec![],
            });
        }
    }
}

// ── Subpass 3b: Call-site generic arity/binding validation ───────────────

pub(crate) fn check_generic_call_bindings(
    graph: &SemanticGraph,
    ctx: &TypeContext<'_>,
    entries: &mut Vec<VerificationEntry>,
) {
    for edge in &graph.edges {
        if edge.kind != EdgeKind::Calls {
            continue;
        }
        let Some(bindings) = &edge.type_arg_bindings else {
            continue;
        };
        let Some(callee) = ctx.by_ref.get(&edge.target).copied() else {
            continue;
        };
        let Some(generic_params) = &callee.generic_params else {
            continue;
        };

        let declared_type_params: Vec<&str> = generic_params
            .iter()
            .filter(|p| p.kind == GenericParamKind::TypeParam)
            .map(|p| p.name.as_str())
            .collect();

        let scope = format!("{}→{}", edge.source.0, callee.name);

        let type_bindings: Vec<_> = bindings
            .iter()
            .filter(|b| {
                let is_const = generic_params
                    .iter()
                    .any(|p| p.name == b.param && p.kind == GenericParamKind::ConstParam);
                !is_const
            })
            .collect();

        let unknown = type_bindings
            .iter()
            .find(|b| !declared_type_params.iter().any(|name| *name == b.param));
        if let Some(binding) = unknown {
            entries.push(VerificationEntry {
                claim: "generic-call-binding".into(),
                state: VerificationState::Failed,
                scope,
                evidence: Some(format!(
                    "{E_GENERIC_BINDING_ARITY}: call binds unknown generic '{}' on '{}'",
                    binding.param, callee.name
                )),
                blocking: true,
                repair_options: vec![],
            });
            continue;
        }

        if type_bindings.len() != declared_type_params.len() {
            entries.push(VerificationEntry {
                claim: "generic-call-binding".into(),
                state: VerificationState::Failed,
                scope,
                evidence: Some(format!(
                    "{E_GENERIC_BINDING_ARITY}: '{}' expects {} type generic bindings, got {}",
                    callee.name,
                    declared_type_params.len(),
                    type_bindings.len()
                )),
                blocking: true,
                repair_options: vec![],
            });
        } else if !type_bindings.is_empty() {
            entries.push(VerificationEntry {
                claim: "generic-call-binding".into(),
                state: VerificationState::Proven,
                scope,
                evidence: None,
                blocking: false,
                repair_options: vec![],
            });
        }
    }
}

// ── Subpass 3c: Effects and capabilities must propagate across calls ─────

pub(crate) fn check_effect_capability_propagation(
    graph: &SemanticGraph,
    ctx: &TypeContext<'_>,
    entries: &mut Vec<VerificationEntry>,
) {
    for edge in &graph.edges {
        if edge.kind != EdgeKind::Calls {
            continue;
        }
        let Some(caller) = ctx.by_ref.get(&edge.source).copied() else {
            continue;
        };
        let Some(callee) = ctx.by_ref.get(&edge.target).copied() else {
            continue;
        };
        let scope = format!("{}→{}", caller.name, callee.name);

        if let Some(callee_effects) = &callee.effect_row {
            let caller_effects = caller
                .effect_row
                .as_ref()
                .map(|row| row.effects.as_slice())
                .unwrap_or(&[]);
            if let Some(missing) = callee_effects
                .effects
                .iter()
                .find(|effect| !caller_effects.iter().any(|e| e == *effect))
            {
                entries.push(VerificationEntry {
                    claim: "effect-propagation".into(),
                    state: VerificationState::Failed,
                    scope: scope.clone(),
                    evidence: Some(format!(
                        "{E_EFFECT_NOT_PROPAGATED}: callee effect '{}' from '{}' is missing \
                         from caller '{}'",
                        missing, callee.name, caller.name
                    )),
                    blocking: true,
                    repair_options: vec![],
                });
            } else if !callee_effects.effects.is_empty() {
                entries.push(VerificationEntry {
                    claim: "effect-propagation".into(),
                    state: VerificationState::Proven,
                    scope: scope.clone(),
                    evidence: None,
                    blocking: false,
                    repair_options: vec![],
                });
            }
        }

        if let Some(callee_caps) = &callee.capability_reqs {
            let caller_caps = caller
                .capability_reqs
                .as_ref()
                .map(|reqs| reqs.caps.as_slice())
                .unwrap_or(&[]);
            if let Some(missing) = callee_caps
                .caps
                .iter()
                .find(|cap| !caller_caps.iter().any(|c| c == *cap))
            {
                entries.push(VerificationEntry {
                    claim: "capability-propagation".into(),
                    state: VerificationState::Failed,
                    scope: scope.clone(),
                    evidence: Some(format!(
                        "{E_CAPABILITY_NOT_PROPAGATED}: callee capability '{}' from '{}' is \
                         missing from caller '{}'",
                        missing, callee.name, caller.name
                    )),
                    blocking: true,
                    repair_options: vec![],
                });
            } else if !callee_caps.caps.is_empty() {
                entries.push(VerificationEntry {
                    claim: "capability-propagation".into(),
                    state: VerificationState::Proven,
                    scope: scope.clone(),
                    evidence: None,
                    blocking: false,
                    repair_options: vec![],
                });
            }
        }
    }
}

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

// ── Subpass 9: Null/absence policy ──────────────────────────────────────

pub(crate) fn check_null_policy(graph: &SemanticGraph, entries: &mut Vec<VerificationEntry>) {
    const NULL_WORDS: &[&str] = &["null", "nil", "undefined", "void"];
    for node in &graph.nodes {
        let Some(return_type) = &node.return_type else {
            continue;
        };
        let lower = return_type.to_lowercase();
        if NULL_WORDS.iter().any(|&w| lower == w) {
            entries.push(VerificationEntry {
                claim: "null-policy".into(),
                state: VerificationState::Failed,
                scope: node.name.clone(),
                evidence: Some(format!(
                    "{E_NULL_IN_CORE_IR}: return_type '{}' of '{}' is a null/nil sentinel; \
                     Core IR prohibits null — use Option<T>, Result<T,E>, or PatchField<T>",
                    return_type, node.name
                )),
                blocking: true,
                repair_options: vec![],
            });
        }
    }
}

// ── Subpass 10: Float equality/ordering policy ───────────────────────────

pub(crate) fn check_float_policy(graph: &SemanticGraph, entries: &mut Vec<VerificationEntry>) {
    for node in &graph.nodes {
        let Some(tf) = &node.type_facts else {
            continue;
        };
        if tf.nominal != "Float" {
            continue;
        }
        let Some(cs) = &node.constraint_set else {
            continue;
        };
        let scope = node.name.clone();
        if cs.has_eq {
            entries.push(VerificationEntry {
                claim: "float-policy".into(),
                state: VerificationState::Failed,
                scope: scope.clone(),
                evidence: Some(format!(
                    "{E_FLOAT_EQ_IMPLICIT}: Float type '{}' declares has_eq=true; \
                     Float equality must be explicit (approximately_equal, bitwise_equal, \
                     or a domain-specific comparator) — not implicit `==`",
                    node.name
                )),
                blocking: true,
                repair_options: vec![],
            });
        }
        if cs.has_ord {
            entries.push(VerificationEntry {
                claim: "float-policy".into(),
                state: VerificationState::Failed,
                scope,
                evidence: Some(format!(
                    "{E_FLOAT_ORD_IMPLICIT}: Float type '{}' declares has_ord=true; \
                     Float has no default total order (NaN breaks totality) — \
                     use NonNaNFloat or an explicit comparator/wrapper instead",
                    node.name
                )),
                blocking: true,
                repair_options: vec![],
            });
        }
    }
}

// ── Subpass 11: PatchField inner-type validation ─────────────────────────

pub(crate) fn check_patchfield(graph: &SemanticGraph, entries: &mut Vec<VerificationEntry>) {
    for node in &graph.nodes {
        let Some(return_type) = &node.return_type else {
            continue;
        };
        if !return_type.starts_with("PatchField<") {
            continue;
        }
        let (_, inner) = split_generic(return_type);
        let scope = node.name.clone();
        if inner.is_empty() {
            entries.push(VerificationEntry {
                claim: "patchfield".into(),
                state: VerificationState::Failed,
                scope,
                evidence: Some(format!(
                    "{E_PATCHFIELD_EMPTY_INNER}: PatchField on '{}' has no inner type; \
                     Core IR requires PatchField<T> where T is a non-empty concrete type",
                    node.name
                )),
                blocking: true,
                repair_options: vec![],
            });
        } else {
            entries.push(VerificationEntry {
                claim: "patchfield".into(),
                state: VerificationState::Proven,
                scope,
                evidence: None,
                blocking: false,
                repair_options: vec![],
            });
        }
    }
}

// ── Subpass 12: PartialOrd vs Ord distinction ────────────────────────────

pub(crate) fn check_partial_ord(graph: &SemanticGraph, entries: &mut Vec<VerificationEntry>) {
    for node in &graph.nodes {
        let Some(cs) = &node.constraint_set else {
            continue;
        };
        if !cs.has_partial_ord {
            continue;
        }
        let Some(return_type) = &node.return_type else {
            continue;
        };
        let scope = node.name.clone();
        if return_type.starts_with("PartialOrd<") {
            entries.push(VerificationEntry {
                claim: "partial-ord".into(),
                state: VerificationState::Proven,
                scope,
                evidence: None,
                blocking: false,
                repair_options: vec![],
            });
        } else if !cs.has_ord {
            entries.push(VerificationEntry {
                claim: "partial-ord".into(),
                state: VerificationState::Unverified,
                scope,
                evidence: Some(format!(
                    "{E_PARTIAL_ORD_REQUIRED}: type '{}' has has_partial_ord=true \
                     but lacks has_ord=true; in a total-order context only partial \
                     ordering is available, which may be insufficient",
                    node.name
                )),
                blocking: false,
                repair_options: vec![],
            });
        }
    }
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

// ── Subpass 15: ConstParam value validation at call sites ────────────────

pub(crate) fn check_const_param_call_bindings(
    graph: &SemanticGraph,
    ctx: &TypeContext<'_>,
    entries: &mut Vec<VerificationEntry>,
) {
    for edge in &graph.edges {
        if edge.kind != EdgeKind::Calls {
            continue;
        }
        let Some(bindings) = &edge.type_arg_bindings else {
            continue;
        };
        let Some(callee) = ctx.by_ref.get(&edge.target).copied() else {
            continue;
        };
        let Some(generic_params) = &callee.generic_params else {
            continue;
        };

        for binding in bindings {
            let is_const = generic_params
                .iter()
                .any(|p| p.name == binding.param && p.kind == GenericParamKind::ConstParam);
            if !is_const {
                continue;
            }

            let scope = format!(
                "{}→{}[{}={}]",
                edge.source.0, callee.name, binding.param, binding.ty
            );

            if is_const_param_value(&binding.ty) {
                entries.push(VerificationEntry {
                    claim: "const-param-value".into(),
                    state: VerificationState::Proven,
                    scope,
                    evidence: None,
                    blocking: false,
                    repair_options: vec![],
                });
            } else {
                entries.push(VerificationEntry {
                    claim: "const-param-value".into(),
                    state: VerificationState::Failed,
                    scope,
                    evidence: Some(format!(
                        "{E_CONST_PARAM_VALUE_INVALID}: ConstParam '{}' on '{}' \
                         bound to '{}' which is not a decidable literal; \
                         only numeric strings or simple identifiers are permitted",
                        binding.param, callee.name, binding.ty
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

// ── Subpass 18: Effect and capability parameter threading ────────────────

pub(crate) fn check_effect_capability_param_threading(
    graph: &SemanticGraph,
    ctx: &TypeContext<'_>,
    entries: &mut Vec<VerificationEntry>,
) {
    for edge in &graph.edges {
        if edge.kind != EdgeKind::Calls {
            continue;
        }
        let Some(caller) = ctx.by_ref.get(&edge.source).copied() else {
            continue;
        };
        let Some(callee) = ctx.by_ref.get(&edge.target).copied() else {
            continue;
        };
        let scope = format!("{}→{}", caller.name, callee.name);

        if let Some(effect_bindings) = &edge.effect_arg_bindings {
            let caller_effects = caller
                .effect_row
                .as_ref()
                .map(|row| row.effects.as_slice())
                .unwrap_or(&[]);

            let mut all_ok = true;
            for binding in effect_bindings {
                for effect in &binding.effects {
                    if !caller_effects.iter().any(|e| e == effect) {
                        entries.push(VerificationEntry {
                            claim: "effect-param-threading".into(),
                            state: VerificationState::Failed,
                            scope: scope.clone(),
                            evidence: Some(format!(
                                "{E_EFFECT_PARAM_NOT_THREADED}: EffectParam '{}' requires \
                                 effect '{}' which is not in caller '{}' effect_row {:?}",
                                binding.param, effect, caller.name, caller_effects
                            )),
                            blocking: true,
                            repair_options: vec![],
                        });
                        all_ok = false;
                    }
                }
            }
            if all_ok {
                entries.push(VerificationEntry {
                    claim: "effect-param-threading".into(),
                    state: VerificationState::Proven,
                    scope: scope.clone(),
                    evidence: None,
                    blocking: false,
                    repair_options: vec![],
                });
            }
        }

        if let Some(cap_bindings) = &edge.capability_arg_bindings {
            let caller_caps = caller
                .capability_reqs
                .as_ref()
                .map(|reqs| reqs.caps.as_slice())
                .unwrap_or(&[]);

            let mut all_ok = true;
            for binding in cap_bindings {
                for cap in &binding.caps {
                    if !caller_caps.iter().any(|c| c == cap) {
                        entries.push(VerificationEntry {
                            claim: "capability-param-threading".into(),
                            state: VerificationState::Failed,
                            scope: scope.clone(),
                            evidence: Some(format!(
                                "{E_CAPABILITY_PARAM_NOT_THREADED}: CapabilityParam '{}' \
                                 requires cap '{}' which is not in caller '{}' \
                                 capability_reqs {:?}",
                                binding.param, cap, caller.name, caller_caps
                            )),
                            blocking: true,
                            repair_options: vec![],
                        });
                        all_ok = false;
                    }
                }
            }
            if all_ok {
                entries.push(VerificationEntry {
                    claim: "capability-param-threading".into(),
                    state: VerificationState::Proven,
                    scope: scope.clone(),
                    evidence: None,
                    blocking: false,
                    repair_options: vec![],
                });
            }
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Attempt to infer a return type from a `body_expr` string using local
/// pattern matching.  Returns `None` when the expression form is unrecognized.
fn infer_expr_type(body: &str, ctx: &TypeContext<'_>) -> Option<String> {
    if body.parse::<i64>().is_ok() {
        return Some("Int".to_string());
    }

    if body == "true" || body == "false" {
        return Some("Bool".to_string());
    }

    if (body.starts_with("if ") || body.starts_with("if("))
        && let Some(inferred) = infer_if_expr_type(body, ctx)
    {
        return Some(inferred);
    }

    let callee_name = body
        .split(|c: char| c == '(' || c.is_whitespace())
        .next()
        .unwrap_or(body)
        .trim();

    if !callee_name.is_empty()
        && let Some(node) = ctx.get_by_name(callee_name)
        && let Some(rt) = &node.return_type
    {
        return Some(rt.clone());
    }

    None
}

/// Infer the type of an if-expression by examining its else branch.
fn infer_if_expr_type(body: &str, ctx: &TypeContext<'_>) -> Option<String> {
    let else_pos = body.find("else")?;
    let after_else = body[else_pos + 4..].trim();

    let else_body = if after_else.starts_with('{') {
        after_else
            .strip_prefix('{')
            .and_then(|s| s.strip_suffix('}'))
            .map(str::trim)
            .unwrap_or(after_else)
    } else {
        after_else
    };

    infer_expr_type(else_body, ctx)
}

/// Returns `true` when `name` is a simple decidable identifier:
/// letters, digits, or underscores only.
pub(crate) fn is_simple_identifier(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Returns `true` when `ty` is a valid decidable ConstParam value:
/// either an all-digit numeric literal or a simple identifier.
fn is_const_param_value(ty: &str) -> bool {
    if ty.is_empty() {
        return false;
    }
    if ty.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    is_simple_identifier(ty)
}

/// Split an associated type reference `"Interface::AssocName"` into
/// `("Interface", "AssocName")`.
///
/// Returns `(input, "")` if the input does not contain `"::"`.
fn split_assoc_type(ty: &str) -> (&str, &str) {
    if let Some(pos) = ty.find("::") {
        (&ty[..pos], &ty[pos + 2..])
    } else {
        (ty, "")
    }
}

/// Attempt to resolve an associated type for `arg_ty` by scanning the
/// `interface_impls` of the corresponding node in `ctx`.
fn resolve_assoc_type<'a>(
    ctx: &TypeContext<'a>,
    arg_ty: &str,
    interface_base: &str,
    assoc_name: &str,
) -> Option<String> {
    let node = ctx.get_by_name(arg_ty)?;
    let impls = node.interface_impls.as_ref()?;
    impls.iter().find_map(|impl_meta| {
        if impl_meta.interface.starts_with(interface_base) {
            impl_meta
                .associated_types
                .iter()
                .find(|at| at.name == assoc_name)
                .map(|at| at.ty.clone())
        } else {
            None
        }
    })
}
