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

use ail_core::semantic_graph::NodeRef;

use crate::diagnostic::{Diagnostic, DiagnosticSeverity, RepairOption};
use crate::report::{SummaryCounts, VerificationEntry, VerificationState};
use crate::type_checker::{E_GENERIC_BINDING_ARITY, TYPE_DIAGNOSTIC_CATEGORY_GENERIC_CALL_BINDING};

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
/// This intentionally starts with one production-maturity slice: generic
/// call-site binding/arity failures.  Those failures were already encoded in
/// entry evidence; promoting them here gives downstream tooling a stable code
/// and category without parsing human text.
pub(crate) fn build_structured_diagnostics(entries: &[VerificationEntry]) -> Vec<Diagnostic> {
    entries
        .iter()
        .filter_map(generic_call_binding_diagnostic)
        .collect()
}

fn generic_call_binding_diagnostic(entry: &VerificationEntry) -> Option<Diagnostic> {
    if entry.claim != TYPE_DIAGNOSTIC_CATEGORY_GENERIC_CALL_BINDING
        || entry.state != VerificationState::Failed
    {
        return None;
    }

    let evidence = entry.evidence.as_deref()?;
    if !evidence.contains(E_GENERIC_BINDING_ARITY) {
        return None;
    }

    let target = call_site_target(&entry.scope)?;

    Some(Diagnostic {
        code: E_GENERIC_BINDING_ARITY.into(),
        severity: DiagnosticSeverity::Error,
        target,
        evidence: Some(format!(
            "category={TYPE_DIAGNOSTIC_CATEGORY_GENERIC_CALL_BINDING}; {evidence}"
        )),
        expected: Some("call-site generic bindings match callee type params".into()),
        actual: Some(entry.scope.clone()),
        repair_options: vec![RepairOption::Explanation(
            "bind every declared type generic exactly once and remove unknown generic bindings"
                .into(),
        )],
        blocking: true,
    })
}

fn call_site_target(scope: &str) -> Option<NodeRef> {
    let source_id = scope.split('→').next()?;
    Some(NodeRef(source_id.parse().ok()?))
}
