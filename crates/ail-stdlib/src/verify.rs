// ── ail-stdlib::verify ────────────────────────────────────────────────────
//
// Verification helpers for the AIL `std.verify` module.

use crate::diagnostics::{Diagnostic, ProofObligation, RepairOption};

// ── VerificationEntry ─────────────────────────────────────────────────────

/// A single entry in a verification report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerificationEntry {
    pub id: String,
    pub requirement: String,
    pub status: VerificationStatus,
    pub diagnostics: Vec<Diagnostic>,
}

/// Status of a verification entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerificationStatus {
    Pass,
    Fail,
    PartiallyMet,
    NotChecked,
}

impl std::fmt::Display for VerificationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerificationStatus::Pass => write!(f, "PASS"),
            VerificationStatus::Fail => write!(f, "FAIL"),
            VerificationStatus::PartiallyMet => write!(f, "PARTIAL"),
            VerificationStatus::NotChecked => write!(f, "NOT_CHECKED"),
        }
    }
}

impl VerificationEntry {
    pub fn new(
        id: impl Into<String>,
        requirement: impl Into<String>,
        status: VerificationStatus,
    ) -> Self {
        Self {
            id: id.into(),
            requirement: requirement.into(),
            status,
            diagnostics: Vec::new(),
        }
    }
}

// ── VerificationReport ────────────────────────────────────────────────────

/// A complete verification report.
#[derive(Clone, Debug, Default)]
pub struct VerificationReport {
    pub entries: Vec<VerificationEntry>,
    pub obligations: Vec<ProofObligation>,
}

impl VerificationReport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_entry(&mut self, entry: VerificationEntry) {
        self.entries.push(entry);
    }

    pub fn add_obligation(&mut self, obligation: ProofObligation) {
        self.obligations.push(obligation);
    }

    pub fn pass_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.status == VerificationStatus::Pass)
            .count()
    }

    pub fn fail_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.status == VerificationStatus::Fail)
            .count()
    }

    pub fn coverage(&self) -> f64 {
        let total = self.entries.len();
        if total == 0 {
            return 0.0;
        }
        self.pass_count() as f64 / total as f64
    }
}

// ── PolicyReport ──────────────────────────────────────────────────────────

/// A policy compliance report.
#[derive(Clone, Debug, Default)]
pub struct PolicyReport {
    pub policy_id: String,
    pub violations: Vec<Diagnostic>,
    pub passed: bool,
}

impl PolicyReport {
    pub fn new(policy_id: impl Into<String>) -> Self {
        Self {
            policy_id: policy_id.into(),
            violations: Vec::new(),
            passed: true,
        }
    }

    pub fn add_violation(&mut self, d: Diagnostic) {
        self.passed = false;
        self.violations.push(d);
    }
}

// ── RuntimeCheck ─────────────────────────────────────────────────────────

/// A runtime invariant check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCheck {
    pub id: String,
    pub description: String,
    pub passed: bool,
    pub error: Option<String>,
}

impl RuntimeCheck {
    pub fn pass(id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            passed: true,
            error: None,
        }
    }

    pub fn fail(
        id: impl Into<String>,
        description: impl Into<String>,
        error: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            passed: false,
            error: Some(error.into()),
        }
    }
}

// ── extract_repair_ops ────────────────────────────────────────────────────

/// Extract repair operations from a `VerificationReport`.
///
/// Returns suggestions for all failed entries.
pub fn extract_repair_ops(report: &VerificationReport) -> Vec<RepairOption> {
    report
        .entries
        .iter()
        .filter(|e| e.status == VerificationStatus::Fail)
        .map(|e| {
            RepairOption::new(
                format!("repair-{}", e.id),
                format!("Fix failing requirement: {}", e.requirement),
                60,
            )
        })
        .collect()
}

// ── group_obligations ─────────────────────────────────────────────────────

/// Group obligations by satisfaction status.
///
/// Returns `(satisfied, unsatisfied)`.
pub fn group_obligations(
    report: &VerificationReport,
) -> (Vec<&ProofObligation>, Vec<&ProofObligation>) {
    let mut satisfied = Vec::new();
    let mut unsatisfied = Vec::new();
    for ob in &report.obligations {
        if ob.satisfied {
            satisfied.push(ob);
        } else {
            unsatisfied.push(ob);
        }
    }
    (satisfied, unsatisfied)
}

// ── format_report ─────────────────────────────────────────────────────────

/// Format a `VerificationReport` as human-readable text.
pub fn format_report(report: &VerificationReport) -> String {
    let total = report.entries.len();
    let pass = report.pass_count();
    let fail = report.fail_count();
    let mut lines = vec![format!(
        "VerificationReport: {}/{} passed ({:.0}%)",
        pass,
        total,
        report.coverage() * 100.0
    )];
    for entry in &report.entries {
        lines.push(format!(
            "  [{}] {} — {}",
            entry.status, entry.id, entry.requirement
        ));
    }
    if !report.obligations.is_empty() {
        lines.push(format!("Obligations: {} total", report.obligations.len()));
    }
    if fail > 0 {
        lines.push(format!("FAIL: {} requirement(s) not met", fail));
    }
    lines.join("\n")
}

// ── Alias for diagnostic format_diagnostic ────────────────────────────────

pub use crate::diagnostics::format_diagnostic;

/// Re-exported diagnostic type for verify consumers.
pub use crate::diagnostics::Diagnostic as VerifyDiagnostic;
pub use crate::diagnostics::DiagnosticSeverity as VerifyDiagnosticSeverity;
