use std::collections::BTreeSet;

use ail_core::semantic_graph::{EdgeKind, SemanticGraph};

use crate::effect_checker::EFFECT_DIAGNOSTIC_CATEGORY_MISSING_EFFECT;
use crate::report::{VerificationEntry, VerificationState};

use crate::type_checker::{
    E_CAPABILITY_NOT_PROPAGATED, E_CAPABILITY_PARAM_NOT_THREADED, E_EFFECT_NOT_PROPAGATED,
    E_EFFECT_PARAM_NOT_THREADED, TYPE_DIAGNOSTIC_CATEGORY_EFFECT, TypeContext,
};

const EFFECT_DIAGNOSTIC_CATEGORY_MISSING_CAPABILITY: &str = "capability.missing";

// ── Subpass 3c: Effects and capabilities must propagate across calls ─────

pub(crate) fn check_effect_capability_propagation(
    graph: &SemanticGraph,
    ctx: &TypeContext<'_>,
    entries: &mut Vec<VerificationEntry>,
) {
    let mut effect_entries = Vec::new();

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
            let caller_effects: BTreeSet<&str> = caller
                .effect_row
                .as_ref()
                .map(|row| row.effects.iter().map(String::as_str).collect())
                .unwrap_or_default();
            let callee_effects: BTreeSet<&str> =
                callee_effects.effects.iter().map(String::as_str).collect();
            let missing: Vec<String> = callee_effects
                .iter()
                .filter(|effect| !caller_effects.contains(*effect))
                .map(|effect| (*effect).to_string())
                .collect();
            if !missing.is_empty() {
                effect_entries.push(VerificationEntry {
                    claim: "effect-propagation".into(),
                    state: VerificationState::Failed,
                    scope: scope.clone(),
                    evidence: Some(effect_issue_evidence(
                        E_EFFECT_NOT_PROPAGATED,
                        EFFECT_DIAGNOSTIC_CATEGORY_MISSING_EFFECT,
                        "callee effect(s) missing from caller effect_row",
                        "effect",
                        missing.len(),
                    )),
                    blocking: true,
                    repair_options: vec![],
                });
            } else if !callee_effects.is_empty() {
                effect_entries.push(VerificationEntry {
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
            let caller_caps: BTreeSet<&str> = caller
                .capability_reqs
                .as_ref()
                .map(|reqs| reqs.caps.iter().map(String::as_str).collect())
                .unwrap_or_default();
            let callee_caps: BTreeSet<&str> = callee_caps.caps.iter().map(String::as_str).collect();
            let missing: Vec<String> = callee_caps
                .iter()
                .filter(|cap| !caller_caps.contains(*cap))
                .map(|cap| (*cap).to_string())
                .collect();
            if !missing.is_empty() {
                effect_entries.push(VerificationEntry {
                    claim: "capability-propagation".into(),
                    state: VerificationState::Failed,
                    scope: scope.clone(),
                    evidence: Some(effect_issue_evidence(
                        E_CAPABILITY_NOT_PROPAGATED,
                        EFFECT_DIAGNOSTIC_CATEGORY_MISSING_CAPABILITY,
                        "callee capability requirement(s) missing from caller capability_reqs",
                        "capability",
                        missing.len(),
                    )),
                    blocking: true,
                    repair_options: vec![],
                });
            } else if !callee_caps.is_empty() {
                effect_entries.push(VerificationEntry {
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

    normalize_effect_entries(&mut effect_entries);
    entries.extend(effect_entries);
}
// ── Subpass 18: Effect and capability parameter threading ────────────────

pub(crate) fn check_effect_capability_param_threading(
    graph: &SemanticGraph,
    ctx: &TypeContext<'_>,
    entries: &mut Vec<VerificationEntry>,
) {
    let mut effect_entries = Vec::new();

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
            let caller_effects: BTreeSet<&str> = caller
                .effect_row
                .as_ref()
                .map(|row| row.effects.iter().map(String::as_str).collect())
                .unwrap_or_default();

            let missing: BTreeSet<&str> = effect_bindings
                .iter()
                .flat_map(|binding| binding.effects.iter().map(String::as_str))
                .filter(|effect| !caller_effects.contains(*effect))
                .collect();
            if missing.is_empty() {
                effect_entries.push(VerificationEntry {
                    claim: "effect-param-threading".into(),
                    state: VerificationState::Proven,
                    scope: scope.clone(),
                    evidence: None,
                    blocking: false,
                    repair_options: vec![],
                });
            } else {
                effect_entries.push(VerificationEntry {
                    claim: "effect-param-threading".into(),
                    state: VerificationState::Failed,
                    scope: scope.clone(),
                    evidence: Some(effect_issue_evidence(
                        E_EFFECT_PARAM_NOT_THREADED,
                        EFFECT_DIAGNOSTIC_CATEGORY_MISSING_EFFECT,
                        "call-site EffectParam binding references effect(s) absent from caller \
                         effect_row",
                        "effect",
                        missing.len(),
                    )),
                    blocking: true,
                    repair_options: vec![],
                });
            }
        }

        if let Some(cap_bindings) = &edge.capability_arg_bindings {
            let caller_caps: BTreeSet<&str> = caller
                .capability_reqs
                .as_ref()
                .map(|reqs| reqs.caps.iter().map(String::as_str).collect())
                .unwrap_or_default();

            let missing: BTreeSet<&str> = cap_bindings
                .iter()
                .flat_map(|binding| binding.caps.iter().map(String::as_str))
                .filter(|cap| !caller_caps.contains(*cap))
                .collect();
            if missing.is_empty() {
                effect_entries.push(VerificationEntry {
                    claim: "capability-param-threading".into(),
                    state: VerificationState::Proven,
                    scope: scope.clone(),
                    evidence: None,
                    blocking: false,
                    repair_options: vec![],
                });
            } else {
                effect_entries.push(VerificationEntry {
                    claim: "capability-param-threading".into(),
                    state: VerificationState::Failed,
                    scope: scope.clone(),
                    evidence: Some(effect_issue_evidence(
                        E_CAPABILITY_PARAM_NOT_THREADED,
                        EFFECT_DIAGNOSTIC_CATEGORY_MISSING_CAPABILITY,
                        "call-site CapabilityParam binding references capability requirement(s) \
                         absent from caller capability_reqs",
                        "capability",
                        missing.len(),
                    )),
                    blocking: true,
                    repair_options: vec![],
                });
            }
        }
    }

    normalize_effect_entries(&mut effect_entries);
    entries.extend(effect_entries);
}

fn effect_issue_evidence(
    code: &str,
    category: &str,
    reason: &str,
    descriptor_kind: &str,
    descriptor_count: usize,
) -> String {
    let descriptors = redacted_descriptors(descriptor_kind, descriptor_count);
    format!(
        "{code}: category={category}; type-category={TYPE_DIAGNOSTIC_CATEGORY_EFFECT}; \
         reason={reason}; count={descriptor_count}; descriptors=[{}]",
        descriptors.join(", ")
    )
}

fn redacted_descriptors(kind: &str, count: usize) -> Vec<String> {
    (0..count).map(|index| format!("{kind}#{index}")).collect()
}

fn normalize_effect_entries(entries: &mut Vec<VerificationEntry>) {
    entries.sort_by(|a, b| {
        effect_claim_rank(&a.claim)
            .cmp(&effect_claim_rank(&b.claim))
            .then_with(|| a.scope.cmp(&b.scope))
            .then_with(|| verification_state_rank(a.state).cmp(&verification_state_rank(b.state)))
            .then_with(|| a.evidence.cmp(&b.evidence))
            .then_with(|| a.repair_options.cmp(&b.repair_options))
    });
    entries.dedup();
}

fn effect_claim_rank(claim: &str) -> u8 {
    match claim {
        "effect-propagation" => 0,
        "capability-propagation" => 1,
        "effect-param-threading" => 2,
        "capability-param-threading" => 3,
        _ => 9,
    }
}

fn verification_state_rank(state: VerificationState) -> u8 {
    match state {
        VerificationState::Proven => 0,
        VerificationState::RuntimeChecked => 1,
        VerificationState::Assumed => 2,
        VerificationState::Unverified => 3,
        VerificationState::Unsafe => 4,
        VerificationState::Failed => 5,
    }
}
