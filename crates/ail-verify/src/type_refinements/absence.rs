use ail_core::semantic_graph::SemanticGraph;

use crate::report::{VerificationEntry, VerificationState};

use crate::type_checker::{
    E_FLOAT_EQ_IMPLICIT, E_FLOAT_ORD_IMPLICIT, E_NULL_IN_CORE_IR, E_PARTIAL_ORD_REQUIRED,
    E_PATCHFIELD_EMPTY_INNER,
};

use crate::type_obligations::split_generic;

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
