// ── ail-verify::type_obligations ─────────────────────────────────────────
//
// Structural type-proof obligation subpasses for the type checker.
//
// # Subpasses contained here
//
// 1. Nominal presence        — `check_nominal_presence`
// 2. Nominal call check      — `check_nominal_calls`
// 4. Variance enforcement    — `check_variance`
// 4b. Structural/Dyn checks  — `check_structural_and_dyn_calls`
// 5. Interface coherence     — `check_interface_coherence`
// 6. Constraint enforcement  — `check_constraints` (+ helpers)
// 17. Blanket impl coherence — `check_blanket_impl_coherence`
//
// All functions are `pub(crate)` free functions (TypeChecker methods that use
// no `self` state, extracted verbatim for readability).

use std::collections::BTreeMap;

use ail_core::semantic_graph::{EdgeKind, GraphNode, NodeKind, SemanticGraph};

use crate::report::{VerificationEntry, VerificationState};
use crate::type_checker::{
    COLLECTION_CONSTRAINTS, E_ASSOC_TYPE_EMPTY_BINDING, E_ASSOC_TYPE_MISMATCH,
    E_BLANKET_IMPL_OVERLAP, E_COHERENCE_DUPLICATE, E_DYN_INTERFACE_UNAVAILABLE, E_MISSING_EQ,
    E_MISSING_HASH, E_MISSING_ORD, E_NOMINAL_MISMATCH, E_ORPHAN_RULE_VIOLATION,
    E_STRUCTURAL_TYPE_MISMATCH, E_VARIANCE_COERCION, TypeContext,
};

// ── Subpass 1: Nominal presence ──────────────────────────────────────────

pub(crate) fn check_nominal_presence(graph: &SemanticGraph, entries: &mut Vec<VerificationEntry>) {
    for node in &graph.nodes {
        if !matches!(node.kind, NodeKind::Function | NodeKind::Type) {
            continue;
        }
        entries.push(classify_nominal_presence(node));
    }
}

fn classify_nominal_presence(node: &GraphNode) -> VerificationEntry {
    let scope = node.name.clone();

    match &node.type_facts {
        None => VerificationEntry {
            claim: "type-check".into(),
            state: VerificationState::Unverified,
            scope,
            evidence: None,
            blocking: false,
            repair_options: vec![],
        },
        Some(tf) if tf.nominal.is_empty() => VerificationEntry {
            claim: "type-check".into(),
            state: VerificationState::Unverified,
            scope,
            evidence: None,
            blocking: false,
            repair_options: vec![],
        },
        Some(tf) => {
            let bad_generic = tf.generics.iter().any(|g| g.is_empty());
            if bad_generic {
                VerificationEntry {
                    claim: "type-check".into(),
                    state: VerificationState::Failed,
                    scope,
                    evidence: Some("E_GENERIC_ARITY: generic parameter name is empty".into()),
                    blocking: true,
                    repair_options: vec![],
                }
            } else {
                VerificationEntry {
                    claim: "type-check".into(),
                    state: VerificationState::Proven,
                    scope,
                    evidence: None,
                    blocking: true,
                    repair_options: vec![],
                }
            }
        }
    }
}

// ── Subpass 2: Nominal call check ────────────────────────────────────────

pub(crate) fn check_nominal_calls(
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
        let Some(params) = &callee.params else {
            continue;
        };
        for (i, (arg_ty, param)) in call_args.iter().zip(params.iter()).enumerate() {
            let scope = format!("{}→{}[{}]", edge.source.0, callee.name, i);
            if arg_ty == &param.ty {
                entries.push(VerificationEntry {
                    claim: "nominal-call".into(),
                    state: VerificationState::Proven,
                    scope,
                    evidence: None,
                    blocking: false,
                    repair_options: vec![],
                });
            } else {
                entries.push(VerificationEntry {
                    claim: "nominal-call".into(),
                    state: VerificationState::Failed,
                    scope,
                    evidence: Some(format!(
                        "{E_NOMINAL_MISMATCH}: expected '{}', got '{arg_ty}' \
                         at param '{}' of '{}'",
                        param.ty, param.name, callee.name
                    )),
                    blocking: true,
                    repair_options: vec![],
                });
            }
        }
    }
}

// ── Subpass 4: Variance enforcement ─────────────────────────────────────

pub(crate) fn check_variance(
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
        let Some(params) = &callee.params else {
            continue;
        };

        for (i, (arg_ty, param)) in call_args.iter().zip(params.iter()).enumerate() {
            if !arg_ty.contains('<') || !param.ty.contains('<') {
                continue;
            }

            let (arg_base, arg_inner) = split_generic(arg_ty);
            let (param_base, param_inner) = split_generic(&param.ty);

            if arg_base != param_base {
                continue;
            }

            if arg_inner != param_inner {
                let scope = format!("{}→{}[{}]", edge.source.0, callee.name, i);
                entries.push(VerificationEntry {
                    claim: "variance".into(),
                    state: VerificationState::Failed,
                    scope,
                    evidence: Some(format!(
                        "{E_VARIANCE_COERCION}: implicit coercion from \
                         '{arg_ty}' to '{}' violates invariance; \
                         use an explicit adapter/constraint instead",
                        param.ty
                    )),
                    blocking: true,
                    repair_options: vec![],
                });
            }
        }
    }
}

// ── Subpass 4b: Structural/Dyn interface call-site checks ────────────────

pub(crate) fn check_structural_and_dyn_calls(
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
        let Some(params) = &callee.params else {
            continue;
        };

        for (idx, (arg_ty, param)) in call_args.iter().zip(params.iter()).enumerate() {
            let scope = format!("{}→{}[{}]", edge.source.0, callee.name, idx);
            if let Some(required) = structural_fields(&param.ty) {
                if structural_type_satisfies(ctx, arg_ty, &required) {
                    entries.push(VerificationEntry {
                        claim: "structural-type".into(),
                        state: VerificationState::Proven,
                        scope: scope.clone(),
                        evidence: None,
                        blocking: false,
                        repair_options: vec![],
                    });
                } else {
                    entries.push(VerificationEntry {
                        claim: "structural-type".into(),
                        state: VerificationState::Failed,
                        scope: scope.clone(),
                        evidence: Some(format!(
                            "{E_STRUCTURAL_TYPE_MISMATCH}: argument type '{}' does not satisfy \
                             structural requirement '{}'",
                            arg_ty, param.ty
                        )),
                        blocking: true,
                        repair_options: vec![],
                    });
                }
            }

            if let Some(interface) = dyn_interface(&param.ty) {
                let implements = ctx
                    .get_by_name(arg_ty)
                    .and_then(|node| node.interface_impls.as_ref())
                    .map(|impls| impls.iter().any(|impl_| impl_.interface == interface))
                    .unwrap_or(false);
                let dyn_state = if implements {
                    VerificationState::Proven
                } else {
                    VerificationState::Failed
                };
                entries.push(VerificationEntry {
                    claim: "dyn-interface".into(),
                    state: dyn_state,
                    scope,
                    evidence: if implements {
                        None
                    } else {
                        Some(format!(
                            "{E_DYN_INTERFACE_UNAVAILABLE}: argument type '{}' has no impl \
                             for Dyn<{}>",
                            arg_ty, interface
                        ))
                    },
                    blocking: !implements,
                    repair_options: vec![],
                });
            }
        }
    }
}

// ── Subpass 5: Interface coherence ───────────────────────────────────────

pub(crate) fn check_interface_coherence(
    graph: &SemanticGraph,
    entries: &mut Vec<VerificationEntry>,
) {
    for node in &graph.nodes {
        let Some(impls) = &node.interface_impls else {
            continue;
        };

        let mut seen_non_adapter: BTreeMap<&str, usize> = BTreeMap::new();
        for (idx, impl_) in impls.iter().enumerate() {
            for at in &impl_.associated_types {
                if at.name.is_empty() {
                    entries.push(VerificationEntry {
                        claim: "coherence".into(),
                        state: VerificationState::Failed,
                        scope: node.name.clone(),
                        evidence: Some(format!(
                            "{E_ASSOC_TYPE_MISMATCH}: associated type binding \
                             has empty name in impl of '{}' on '{}'",
                            impl_.interface, node.name
                        )),
                        blocking: true,
                        repair_options: vec![],
                    });
                }
                if at.ty.is_empty() {
                    entries.push(VerificationEntry {
                        claim: "coherence".into(),
                        state: VerificationState::Failed,
                        scope: node.name.clone(),
                        evidence: Some(format!(
                            "{E_ASSOC_TYPE_EMPTY_BINDING}: associated type binding '{}' \
                             has no concrete type in impl of '{}' on '{}'; \
                             associated types must be explicit in the IR",
                            at.name, impl_.interface, node.name
                        )),
                        blocking: true,
                        repair_options: vec![],
                    });
                }
            }

            if impl_.is_adapter {
                continue;
            }

            if let Some(first_idx) = seen_non_adapter.get(impl_.interface.as_str()).copied() {
                entries.push(VerificationEntry {
                    claim: "coherence".into(),
                    state: VerificationState::Failed,
                    scope: node.name.clone(),
                    evidence: Some(format!(
                        "{E_COHERENCE_DUPLICATE}: duplicate non-adapter \
                         implementation #{idx} of '{}' on '{}' \
                         (first at #{first_idx}); \
                         ambiguous impl must fail deterministically",
                        impl_.interface, node.name
                    )),
                    blocking: true,
                    repair_options: vec![],
                });
            } else {
                seen_non_adapter.insert(&impl_.interface, idx);
            }
        }
    }
}

// ── Subpass 6: Constraint enforcement ───────────────────────────────────

pub(crate) fn check_constraints(
    graph: &SemanticGraph,
    ctx: &TypeContext<'_>,
    entries: &mut Vec<VerificationEntry>,
) {
    // 6a: Collection type constraint requirements.
    for node in &graph.nodes {
        let Some(tf) = &node.type_facts else {
            continue;
        };
        if tf.generics.is_empty() {
            continue;
        }
        for &(coll_nominal, needs_eq, needs_hash, needs_ord) in COLLECTION_CONSTRAINTS {
            if tf.nominal != coll_nominal {
                continue;
            }
            let type_arg = &tf.generics[0];
            emit_constraint_check(
                ctx,
                type_arg,
                node.name.as_str(),
                needs_eq,
                needs_hash,
                needs_ord,
                entries,
            );
            break;
        }
    }

    // 6b: Call-site generic instantiation constraint checks.
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
            let Some(gp) = generic_params.iter().find(|p| p.name == binding.param) else {
                continue;
            };
            if gp.required_constraints.is_empty() {
                continue;
            }
            let needs_eq = gp.required_constraints.iter().any(|c| c.interface == "Eq");
            let needs_hash = gp
                .required_constraints
                .iter()
                .any(|c| c.interface == "Hashable");
            let needs_ord = gp.required_constraints.iter().any(|c| c.interface == "Ord");
            let scope = format!(
                "{}→{}[{}={}]",
                edge.source.0, callee.name, binding.param, binding.ty
            );
            emit_constraint_check_for_scope(
                ctx,
                &binding.ty,
                &scope,
                needs_eq,
                needs_hash,
                needs_ord,
                entries,
            );
        }
    }
}

fn emit_constraint_check(
    ctx: &TypeContext<'_>,
    type_arg: &str,
    node_name: &str,
    needs_eq: bool,
    needs_hash: bool,
    needs_ord: bool,
    entries: &mut Vec<VerificationEntry>,
) {
    let scope = format!("{node_name}<{type_arg}>");
    emit_constraint_check_for_scope(
        ctx, type_arg, &scope, needs_eq, needs_hash, needs_ord, entries,
    );
}

fn emit_constraint_check_for_scope(
    ctx: &TypeContext<'_>,
    type_name: &str,
    scope: &str,
    needs_eq: bool,
    needs_hash: bool,
    needs_ord: bool,
    entries: &mut Vec<VerificationEntry>,
) {
    let Some(type_node) = ctx.get_by_name(type_name) else {
        return;
    };
    let cs = type_node.constraint_set.as_ref();

    let has_eq = cs.map(|c| c.has_eq).unwrap_or(false);
    let has_hash = cs.map(|c| c.has_hash).unwrap_or(false);
    let has_ord = cs.map(|c| c.has_ord).unwrap_or(false);

    let mut evidence_parts: Vec<String> = Vec::new();

    if needs_eq && !has_eq {
        evidence_parts.push(format!(
            "{E_MISSING_EQ}: type '{type_name}' requires Eq constraint"
        ));
    }
    if needs_hash && !has_hash {
        evidence_parts.push(format!(
            "{E_MISSING_HASH}: type '{type_name}' requires Hashable constraint"
        ));
    }
    if needs_ord && !has_ord {
        evidence_parts.push(format!(
            "{E_MISSING_ORD}: type '{type_name}' requires Ord constraint"
        ));
    }

    if evidence_parts.is_empty() {
        entries.push(VerificationEntry {
            claim: "constraint-check".into(),
            state: VerificationState::Proven,
            scope: scope.to_string(),
            evidence: None,
            blocking: false,
            repair_options: vec![],
        });
    } else {
        entries.push(VerificationEntry {
            claim: "constraint-check".into(),
            state: VerificationState::Failed,
            scope: scope.to_string(),
            evidence: Some(evidence_parts.join("; ")),
            blocking: true,
            repair_options: vec![],
        });
    }
}

// ── Subpass 17: Blanket impl coherence and orphan rule ───────────────────

pub(crate) fn check_blanket_impl_coherence(
    graph: &SemanticGraph,
    ctx: &TypeContext<'_>,
    entries: &mut Vec<VerificationEntry>,
) {
    let mut impl_map: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for node in &graph.nodes {
        let Some(impls) = &node.interface_impls else {
            continue;
        };
        for impl_meta in impls {
            if impl_meta.is_adapter {
                continue;
            }
            impl_map
                .entry(impl_meta.interface.clone())
                .or_default()
                .push(node.name.clone());
        }
    }

    for (interface, node_names) in &impl_map {
        if node_names.len() > 1 {
            entries.push(VerificationEntry {
                claim: "blanket-impl-coherence".into(),
                state: VerificationState::Failed,
                scope: node_names.join(","),
                evidence: Some(format!(
                    "{E_BLANKET_IMPL_OVERLAP}: interface '{}' has overlapping \
                     non-adapter impls on nodes {:?}; \
                     blanket impl coherence violation",
                    interface, node_names
                )),
                blocking: true,
                repair_options: vec![],
            });
        }
    }

    for node in &graph.nodes {
        let Some(impls) = &node.interface_impls else {
            continue;
        };
        for impl_meta in impls {
            if impl_meta.is_adapter {
                continue;
            }
            let interface_owned = ctx
                .get_by_name(&impl_meta.interface)
                .is_some_and(|n| matches!(n.kind, NodeKind::Interface | NodeKind::Type));
            if !interface_owned {
                entries.push(VerificationEntry {
                    claim: "orphan-rule".into(),
                    state: VerificationState::Failed,
                    scope: node.name.clone(),
                    evidence: Some(format!(
                        "{E_ORPHAN_RULE_VIOLATION}: node '{}' implements interface '{}' \
                         (is_adapter=false) but neither an Interface nor a Type node \
                         named '{}' exists in the graph; \
                         orphan rule requires the interface or type to be declared locally",
                        node.name, impl_meta.interface, impl_meta.interface
                    )),
                    blocking: true,
                    repair_options: vec![],
                });
            }
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Split `"Base<Inner>"` into `("Base", "Inner")`.
///
/// Returns `(input, "")` if the input is not a parameterized type.
pub(crate) fn split_generic(ty: &str) -> (&str, &str) {
    if let Some(lt) = ty.find('<') {
        let base = ty[..lt].trim_end();
        let inner = ty[lt + 1..].trim_end_matches('>').trim();
        (base, inner)
    } else {
        (ty, "")
    }
}

pub(crate) fn dyn_interface(ty: &str) -> Option<&str> {
    ty.strip_prefix("Dyn<")
        .and_then(|rest| rest.strip_suffix('>'))
        .map(str::trim)
        .filter(|interface| !interface.is_empty())
}

pub(crate) fn structural_fields(ty: &str) -> Option<Vec<String>> {
    let body = ty
        .strip_prefix("struct{")
        .and_then(|rest| rest.strip_suffix('}'))?;
    let fields: Vec<String> = body
        .split(',')
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .map(str::to_string)
        .collect();
    if fields.is_empty() {
        None
    } else {
        Some(fields)
    }
}

pub(crate) fn structural_type_satisfies(
    ctx: &TypeContext<'_>,
    arg_ty: &str,
    required: &[String],
) -> bool {
    if let Some(actual) = structural_fields(arg_ty) {
        return required
            .iter()
            .all(|field| actual.iter().any(|a| a == field));
    }
    let Some(node) = ctx.get_by_name(arg_ty) else {
        return false;
    };
    let Some(constraints) = &node.constraint_set else {
        return false;
    };
    required.iter().all(|field| {
        constraints.extras.iter().any(|extra| {
            extra == field
                || extra
                    .strip_prefix("field:")
                    .map(|declared| declared == field)
                    .unwrap_or(false)
        })
    })
}
