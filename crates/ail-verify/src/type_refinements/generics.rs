use ail_core::semantic_graph::{EdgeKind, GenericParamKind, SemanticGraph};

use crate::report::{VerificationEntry, VerificationState};

use crate::type_checker::{E_CAPABILITY_PARAM_WIDENED, E_CONST_PARAM_UNDECIDABLE, E_CONST_PARAM_VALUE_INVALID, E_EFFECT_PARAM_WIDENED, E_GENERIC_BINDING_ARITY, TypeContext};

use super::helpers::{is_const_param_value, is_simple_identifier};

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