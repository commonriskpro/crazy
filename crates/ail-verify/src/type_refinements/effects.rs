use ail_core::semantic_graph::{EdgeKind, SemanticGraph};

use crate::report::{VerificationEntry, VerificationState};

use crate::type_checker::{E_CAPABILITY_NOT_PROPAGATED, E_CAPABILITY_PARAM_NOT_THREADED, E_EFFECT_NOT_PROPAGATED, E_EFFECT_PARAM_NOT_THREADED, TypeContext};

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