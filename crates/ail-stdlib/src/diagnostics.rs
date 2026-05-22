// ── ail-stdlib::diagnostics ───────────────────────────────────────────────
//
// Diagnostic types and helpers for the AIL `std.diagnostics` module.
//
// Used for tooling/LLM repair workflows.

// ── Diagnostic ────────────────────────────────────────────────────────────

/// Severity level for a diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
    Fatal,
}

impl std::fmt::Display for DiagnosticSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiagnosticSeverity::Info => write!(f, "info"),
            DiagnosticSeverity::Warning => write!(f, "warning"),
            DiagnosticSeverity::Error => write!(f, "error"),
            DiagnosticSeverity::Fatal => write!(f, "fatal"),
        }
    }
}

/// A diagnostic message from the compiler, verifier, or linter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub id: String,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub location: Option<SourceLocation>,
    pub notes: Vec<String>,
}

/// A source location for a diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceLocation {
    pub file: String,
    pub line: u32,
    pub column: u32,
}

impl Diagnostic {
    pub fn new(
        id: impl Into<String>,
        severity: DiagnosticSeverity,
        message: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            severity,
            message: message.into(),
            location: None,
            notes: Vec::new(),
        }
    }

    pub fn with_location(mut self, file: impl Into<String>, line: u32, column: u32) -> Self {
        self.location = Some(SourceLocation {
            file: file.into(),
            line,
            column,
        });
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }
}

// ── format_diagnostic ─────────────────────────────────────────────────────

/// Format a `Diagnostic` into a human-readable string.
///
/// Canonical helper required by `docs/stdlib.md`.
pub fn format_diagnostic(d: &Diagnostic) -> String {
    let loc = d
        .location
        .as_ref()
        .map(|l| format!(" at {}:{}:{}", l.file, l.line, l.column))
        .unwrap_or_default();
    let notes = if d.notes.is_empty() {
        String::new()
    } else {
        format!("\n  notes: {}", d.notes.join("; "))
    };
    format!(
        "[{}] {}{}{}{}",
        d.severity,
        d.id,
        loc,
        if notes.is_empty() { "" } else { ":" },
        notes
    )
}

// ── RepairOption ──────────────────────────────────────────────────────────

/// A suggested repair for a diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepairOption {
    pub id: String,
    pub description: String,
    pub confidence: u8, // 0–100
    pub patch: Option<String>,
}

impl RepairOption {
    pub fn new(id: impl Into<String>, description: impl Into<String>, confidence: u8) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            confidence,
            patch: None,
        }
    }

    pub fn with_patch(mut self, patch: impl Into<String>) -> Self {
        self.patch = Some(patch.into());
        self
    }
}

// ── extract_repair_ops ────────────────────────────────────────────────────

/// Extract repair options from a list of diagnostics.
///
/// In a real implementation, this queries the LLM or static analysis engine.
/// Here it returns placeholder repairs for each error-level diagnostic.
///
/// Canonical helper required by `docs/stdlib.md`.
pub fn extract_repair_ops(diagnostics: &[Diagnostic]) -> Vec<RepairOption> {
    diagnostics
        .iter()
        .filter(|d| d.severity >= DiagnosticSeverity::Error)
        .map(|d| {
            RepairOption::new(
                format!("repair-{}", d.id),
                format!("Fix diagnostic: {}", d.message),
                50,
            )
        })
        .collect()
}

// ── ProofObligation ───────────────────────────────────────────────────────

/// A proof obligation from the verifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProofObligation {
    pub id: String,
    pub description: String,
    pub module: String,
    pub satisfied: bool,
}

impl ProofObligation {
    pub fn new(
        id: impl Into<String>,
        description: impl Into<String>,
        module: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            module: module.into(),
            satisfied: false,
        }
    }
}

// ── group_obligations ─────────────────────────────────────────────────────

/// Group proof obligations by module.
///
/// Returns a `Vec<(module, Vec<ProofObligation>)>` ordered by module name.
///
/// Canonical helper required by `docs/stdlib.md`.
pub fn group_obligations(obligations: &[ProofObligation]) -> Vec<(String, Vec<ProofObligation>)> {
    use std::collections::BTreeMap;
    let mut by_module: BTreeMap<String, Vec<ProofObligation>> = BTreeMap::new();
    for ob in obligations {
        by_module
            .entry(ob.module.clone())
            .or_default()
            .push(ob.clone());
    }
    by_module.into_iter().collect()
}
