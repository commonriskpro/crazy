// ── ail-verify::type_diagnostics ─────────────────────────────────────────
//
// Report assembly helpers for the type checker.
//
// # Scope
//
// Entry points:
// - `build_summary_counts` consumes a slice of `VerificationEntry` values and
//   returns the aggregated `SummaryCounts` included in the final report.
// - `build_structured_diagnostics` promotes selected type-checker failures
//   from evidence strings into stable `Diagnostic` records.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use ail_core::semantic_graph::{NodeRef, SemanticGraph};

use crate::diagnostic::{Diagnostic, DiagnosticSeverity, RepairOption};
use crate::report::{SummaryCounts, VerificationEntry, VerificationState};
use crate::type_checker::{
    E_ASSOC_TYPE_EMPTY_BINDING, E_ASSOC_TYPE_MISMATCH, E_ASSOC_TYPE_NOT_RESOLVED,
    E_BOUNDARY_INFERENCE_MISMATCH, E_CAPABILITY_NOT_PROPAGATED, E_CAPABILITY_PARAM_NOT_THREADED,
    E_CAPABILITY_PARAM_WIDENED, E_COHERENCE_DUPLICATE, E_CONST_PARAM_UNDECIDABLE,
    E_CONST_PARAM_VALUE_INVALID, E_DYN_INTERFACE_UNAVAILABLE, E_EFFECT_NOT_PROPAGATED,
    E_EFFECT_PARAM_NOT_THREADED, E_EFFECT_PARAM_WIDENED, E_FLOAT_EQ_IMPLICIT, E_FLOAT_ORD_IMPLICIT,
    E_FOREIGN_TYPE_NO_SCHEMA, E_GENERIC_ARITY, E_GENERIC_BINDING_ARITY, E_MISSING_EQ,
    E_MISSING_HASH, E_MISSING_ORD, E_NOMINAL_MISMATCH, E_ORPHAN_RULE_VIOLATION,
    E_PARTIAL_ORD_REQUIRED, E_PATCHFIELD_EMPTY_INNER, E_REFINEMENT_PROOF_UNDISCHARGED,
    E_REFINEMENT_RUNTIME_CHECK_MISSING, E_STRUCTURAL_TYPE_MISMATCH, E_VARIANCE_COERCION,
    TYPE_DIAGNOSTIC_CATEGORY_EFFECT, TYPE_DIAGNOSTIC_CATEGORY_GENERIC_CALL_BINDING,
    TYPE_DIAGNOSTIC_CATEGORY_REFINEMENT, TYPE_DIAGNOSTIC_CATEGORY_TYPE_MISMATCH,
    TYPE_DIAGNOSTIC_CATEGORY_UNKNOWN_SYMBOL,
};

// ── Summary counts ────────────────────────────────────────────────────────

/// Build `SummaryCounts` from the entry list.
pub(crate) fn build_summary_counts(entries: &[VerificationEntry]) -> SummaryCounts {
    SummaryCounts {
        verified_count: entries
            .iter()
            .filter(|e| {
                e.state == VerificationState::Proven || e.state == VerificationState::RuntimeChecked
            })
            .count(),
        runtime_checked_count: entries
            .iter()
            .filter(|e| e.state == VerificationState::RuntimeChecked)
            .count(),
        assumed_count: entries
            .iter()
            .filter(|e| e.state == VerificationState::Assumed)
            .count(),
        unverified_count: entries
            .iter()
            .filter(|e| e.state == VerificationState::Unverified)
            .count(),
        unsafe_count: entries
            .iter()
            .filter(|e| e.state == VerificationState::Unsafe)
            .count(),
        failed_count: entries
            .iter()
            .filter(|e| e.state == VerificationState::Failed)
            .count(),
    }
}

// ── Structured diagnostics ───────────────────────────────────────────────

/// Build stable structured diagnostics from type-checker entries.
///
/// Type-checker subpasses still own the detailed `VerificationEntry` evidence.
/// This layer turns selected non-proven entries into production-facing
/// diagnostics with stable categories/codes, deterministic ordering and
/// redacted descriptors so external logs do not leak user type names,
/// predicates, effects or capabilities.
pub(crate) fn build_structured_diagnostics(
    entries: &[VerificationEntry],
    graph: &SemanticGraph,
) -> Vec<Diagnostic> {
    let ctx = DiagnosticContext::new(graph);
    let mut diagnostics: Vec<Diagnostic> = entries
        .iter()
        .filter_map(|entry| type_entry_diagnostic(entry, &ctx))
        .collect();
    canonicalize_type_diagnostics(&mut diagnostics);
    diagnostics
}

struct DiagnosticContext {
    node_by_name: BTreeMap<String, NodeRef>,
}

impl DiagnosticContext {
    fn new(graph: &SemanticGraph) -> Self {
        let mut node_by_name = BTreeMap::new();
        for node in &graph.nodes {
            node_by_name
                .entry(node.name.clone())
                .and_modify(|existing: &mut NodeRef| {
                    if node.id < *existing {
                        *existing = node.id;
                    }
                })
                .or_insert(node.id);
        }
        Self { node_by_name }
    }

    fn target_for_scope(&self, scope: &str) -> Option<NodeRef> {
        let source = scope_source(scope);
        source
            .parse::<u32>()
            .ok()
            .map(NodeRef)
            .or_else(|| self.node_by_name.get(source).copied())
    }
}

fn type_entry_diagnostic(entry: &VerificationEntry, ctx: &DiagnosticContext) -> Option<Diagnostic> {
    if !matches!(
        entry.state,
        VerificationState::Failed | VerificationState::Unverified
    ) {
        return None;
    }

    let evidence = entry.evidence.as_deref()?;
    let code = type_error_code(evidence)?;
    let category = type_diagnostic_category(entry, code, evidence)?;
    let target = ctx.target_for_scope(&entry.scope)?;
    let scope = redacted_scope_descriptor(&entry.scope);
    let detail = redacted_detail_descriptor(entry, code, evidence);

    Some(Diagnostic {
        code: code.into(),
        severity: severity_for(entry),
        target,
        evidence: Some(format!(
            "category={category}; code={code}; claim={}; target=node#{}; scope={scope}; detail={detail}",
            entry.claim, target.0
        )),
        expected: Some(expected_descriptor(category).into()),
        actual: Some(format!("{category} issue redacted; {scope}; {detail}")),
        repair_options: vec![RepairOption::Explanation(
            repair_descriptor(category).into(),
        )],
        blocking: entry.blocking,
    })
}

fn type_error_code(evidence: &str) -> Option<&'static str> {
    TYPE_ERROR_CODES
        .iter()
        .copied()
        .find(|code| evidence.contains(code))
}

fn type_diagnostic_category(
    entry: &VerificationEntry,
    code: &str,
    evidence: &str,
) -> Option<&'static str> {
    match code {
        E_NOMINAL_MISMATCH
        | E_STRUCTURAL_TYPE_MISMATCH
        | E_VARIANCE_COERCION
        | E_BOUNDARY_INFERENCE_MISMATCH
        | E_ASSOC_TYPE_MISMATCH
        | E_ASSOC_TYPE_EMPTY_BINDING
        | E_COHERENCE_DUPLICATE
        | E_MISSING_EQ
        | E_MISSING_HASH
        | E_MISSING_ORD
        | E_FLOAT_EQ_IMPLICIT
        | E_FLOAT_ORD_IMPLICIT
        | E_PARTIAL_ORD_REQUIRED
        | E_PATCHFIELD_EMPTY_INNER => Some(TYPE_DIAGNOSTIC_CATEGORY_TYPE_MISMATCH),

        E_ASSOC_TYPE_NOT_RESOLVED | E_DYN_INTERFACE_UNAVAILABLE | E_ORPHAN_RULE_VIOLATION => {
            Some(TYPE_DIAGNOSTIC_CATEGORY_UNKNOWN_SYMBOL)
        }

        E_EFFECT_NOT_PROPAGATED
        | E_CAPABILITY_NOT_PROPAGATED
        | E_EFFECT_PARAM_NOT_THREADED
        | E_CAPABILITY_PARAM_NOT_THREADED
        | E_EFFECT_PARAM_WIDENED
        | E_CAPABILITY_PARAM_WIDENED => Some(TYPE_DIAGNOSTIC_CATEGORY_EFFECT),

        E_REFINEMENT_PROOF_UNDISCHARGED | E_REFINEMENT_RUNTIME_CHECK_MISSING => {
            Some(TYPE_DIAGNOSTIC_CATEGORY_REFINEMENT)
        }

        E_GENERIC_BINDING_ARITY if evidence.contains("unknown generic") => {
            Some(TYPE_DIAGNOSTIC_CATEGORY_UNKNOWN_SYMBOL)
        }
        E_GENERIC_BINDING_ARITY if entry.claim == TYPE_DIAGNOSTIC_CATEGORY_GENERIC_CALL_BINDING => {
            Some(TYPE_DIAGNOSTIC_CATEGORY_GENERIC_CALL_BINDING)
        }
        E_GENERIC_ARITY | E_CONST_PARAM_UNDECIDABLE | E_CONST_PARAM_VALUE_INVALID => {
            Some(TYPE_DIAGNOSTIC_CATEGORY_GENERIC_CALL_BINDING)
        }

        E_FOREIGN_TYPE_NO_SCHEMA => Some(TYPE_DIAGNOSTIC_CATEGORY_UNKNOWN_SYMBOL),
        _ => None,
    }
}

fn severity_for(entry: &VerificationEntry) -> DiagnosticSeverity {
    match entry.state {
        VerificationState::Failed => DiagnosticSeverity::Error,
        VerificationState::Unverified => DiagnosticSeverity::Warning,
        _ => DiagnosticSeverity::Info,
    }
}

fn expected_descriptor(category: &str) -> &'static str {
    match category {
        TYPE_DIAGNOSTIC_CATEGORY_TYPE_MISMATCH => "compatible type descriptors",
        TYPE_DIAGNOSTIC_CATEGORY_UNKNOWN_SYMBOL => "resolved type symbol or binding",
        TYPE_DIAGNOSTIC_CATEGORY_EFFECT => "declared caller effect/capability coverage",
        TYPE_DIAGNOSTIC_CATEGORY_REFINEMENT => "discharged or materialized refinement proof",
        TYPE_DIAGNOSTIC_CATEGORY_GENERIC_CALL_BINDING => "complete generic call-site binding set",
        _ => "valid type-checker condition",
    }
}

fn repair_descriptor(category: &str) -> &'static str {
    match category {
        TYPE_DIAGNOSTIC_CATEGORY_TYPE_MISMATCH => {
            "align the call-site and declared type shapes without relying on implicit coercion"
        }
        TYPE_DIAGNOSTIC_CATEGORY_UNKNOWN_SYMBOL => {
            "declare the referenced type symbol or remove the unresolved binding"
        }
        TYPE_DIAGNOSTIC_CATEGORY_EFFECT => {
            "thread the required effect or capability through the caller declaration"
        }
        TYPE_DIAGNOSTIC_CATEGORY_REFINEMENT => {
            "add a proof obligation witness or materialize the required runtime check"
        }
        TYPE_DIAGNOSTIC_CATEGORY_GENERIC_CALL_BINDING => {
            "bind every declared generic parameter exactly once with a decidable value"
        }
        _ => "inspect the corresponding verification entry for local repair context",
    }
}

fn redacted_scope_descriptor(scope: &str) -> String {
    let mut parts = Vec::new();
    if scope.contains('→') {
        parts.push("call-site".to_string());
    } else if scope.contains('<') {
        parts.push("type-argument-scope".to_string());
    } else {
        parts.push("node".to_string());
    }

    if let Some(index) = param_index(scope) {
        parts.push(format!("param_index={index}"));
    } else if scope.contains('[') {
        parts.push("binding".to_string());
    }

    parts.join(";")
}

fn redacted_detail_descriptor(
    entry: &VerificationEntry,
    code: &str,
    evidence: &str,
) -> &'static str {
    match code {
        E_GENERIC_BINDING_ARITY if evidence.contains("unknown generic") => "unknown-binding",
        E_GENERIC_BINDING_ARITY => "arity-mismatch",
        E_NOMINAL_MISMATCH | E_STRUCTURAL_TYPE_MISMATCH | E_VARIANCE_COERCION => {
            "call-type-shape-mismatch"
        }
        E_ASSOC_TYPE_NOT_RESOLVED => "associated-type-unresolved",
        E_DYN_INTERFACE_UNAVAILABLE | E_ORPHAN_RULE_VIOLATION => "interface-unresolved",
        E_EFFECT_NOT_PROPAGATED | E_CAPABILITY_NOT_PROPAGATED => {
            "callee-requirement-not-propagated"
        }
        E_EFFECT_PARAM_NOT_THREADED | E_CAPABILITY_PARAM_NOT_THREADED => {
            "generic-requirement-not-threaded"
        }
        E_REFINEMENT_PROOF_UNDISCHARGED => "proof-undischarged",
        E_REFINEMENT_RUNTIME_CHECK_MISSING => "runtime-check-missing",
        _ if entry.claim == "boundary-inference" => "boundary-descriptor-mismatch",
        _ => "type-checker-condition",
    }
}

fn scope_source(scope: &str) -> &str {
    scope
        .split('→')
        .next()
        .unwrap_or(scope)
        .split('[')
        .next()
        .unwrap_or(scope)
}

fn param_index(scope: &str) -> Option<&str> {
    let index = scope.split_once('[')?.1.split_once(']')?.0;
    index.chars().all(|c| c.is_ascii_digit()).then_some(index)
}

fn canonicalize_type_diagnostics(diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.sort_by(cmp_type_diagnostic);
    diagnostics.dedup();
}

fn cmp_type_diagnostic(a: &Diagnostic, b: &Diagnostic) -> Ordering {
    type_diagnostic_category_rank(a)
        .cmp(&type_diagnostic_category_rank(b))
        .then_with(|| a.code.cmp(&b.code))
        .then_with(|| a.target.cmp(&b.target))
        .then_with(|| a.blocking.cmp(&b.blocking).reverse())
        .then_with(|| a.evidence.cmp(&b.evidence))
        .then_with(|| a.expected.cmp(&b.expected))
        .then_with(|| a.actual.cmp(&b.actual))
        .then_with(|| format!("{:?}", a.repair_options).cmp(&format!("{:?}", b.repair_options)))
}

fn type_diagnostic_category_rank(diagnostic: &Diagnostic) -> u8 {
    match diagnostic.evidence.as_deref().and_then(diagnostic_category) {
        Some(TYPE_DIAGNOSTIC_CATEGORY_TYPE_MISMATCH) => 0,
        Some(TYPE_DIAGNOSTIC_CATEGORY_UNKNOWN_SYMBOL) => 1,
        Some(TYPE_DIAGNOSTIC_CATEGORY_EFFECT) => 2,
        Some(TYPE_DIAGNOSTIC_CATEGORY_REFINEMENT) => 3,
        Some(TYPE_DIAGNOSTIC_CATEGORY_GENERIC_CALL_BINDING) => 4,
        _ => 5,
    }
}

fn diagnostic_category(evidence: &str) -> Option<&str> {
    evidence
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("category="))
}

const TYPE_ERROR_CODES: &[&str] = &[
    E_NOMINAL_MISMATCH,
    E_GENERIC_ARITY,
    E_EFFECT_PARAM_WIDENED,
    E_CAPABILITY_PARAM_WIDENED,
    E_VARIANCE_COERCION,
    E_COHERENCE_DUPLICATE,
    E_ASSOC_TYPE_MISMATCH,
    E_MISSING_EQ,
    E_MISSING_HASH,
    E_MISSING_ORD,
    E_CONST_PARAM_UNDECIDABLE,
    E_BOUNDARY_INFERENCE_MISMATCH,
    E_FLOAT_EQ_IMPLICIT,
    E_FLOAT_ORD_IMPLICIT,
    E_ASSOC_TYPE_EMPTY_BINDING,
    E_GENERIC_BINDING_ARITY,
    E_EFFECT_NOT_PROPAGATED,
    E_CAPABILITY_NOT_PROPAGATED,
    E_STRUCTURAL_TYPE_MISMATCH,
    E_DYN_INTERFACE_UNAVAILABLE,
    E_REFINEMENT_PROOF_UNDISCHARGED,
    E_REFINEMENT_RUNTIME_CHECK_MISSING,
    E_PATCHFIELD_EMPTY_INNER,
    E_PARTIAL_ORD_REQUIRED,
    E_ASSOC_TYPE_NOT_RESOLVED,
    E_FOREIGN_TYPE_NO_SCHEMA,
    E_ORPHAN_RULE_VIOLATION,
    E_EFFECT_PARAM_NOT_THREADED,
    E_CAPABILITY_PARAM_NOT_THREADED,
    E_CONST_PARAM_VALUE_INVALID,
];
