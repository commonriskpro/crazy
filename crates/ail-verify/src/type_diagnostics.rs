// ── ail-verify::type_diagnostics ─────────────────────────────────────────
//
// Report assembly helpers for the type checker.
//
// # Scope
//
// Single entry point: `build_summary_counts` consumes a slice of
// `VerificationEntry` values and returns the aggregated `SummaryCounts`
// included in the final `VerificationReport`.

use crate::report::{SummaryCounts, VerificationEntry, VerificationState};

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
