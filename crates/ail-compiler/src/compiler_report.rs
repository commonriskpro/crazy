// ── ail-compiler::compiler_report ────────────────────────────────────────
//
// `CompilerReport` — structured compilation metadata produced alongside
// each compiled artifact.
//
// # Design (docs/compiler.md §Outputs)
//
// The compiler outputs list includes `compiler_report`.  This type captures:
// - Which compilation stages ran.
// - Per-stage timing (in microseconds).
// - Any non-fatal warnings emitted.
// - The profile the artifact was compiled for.
// - The `verification_report_hash` that authorized compilation.
//
// `CompilerReport` is additive — fields use `serde(default)` so older
// serialized reports without optional fields still deserialize cleanly.
//
// # Relationship to CompileError
//
// `CompileError` is returned on fatal pipeline failure.
// `CompilerReport` is returned alongside a successful artifact — it describes
// what happened, not that something went wrong.

use serde::{Deserialize, Serialize};

// ── Compiler report diagnostics ─────────────────────────────────────────

/// Current schema identifier for compiler reports emitted beside artifacts.
pub const COMPILER_REPORT_SCHEMA_VERSION: &str = "compiler/1.0";

/// Stable issue code for stale compiler report schema sidecars.
pub const E_COMPILER_REPORT_STALE_SCHEMA: &str = "E_COMPILER_REPORT_STALE_SCHEMA";

/// Stable issue code for outputs missing a persisted artifact path/id.
pub const E_COMPILER_REPORT_MISSING_ARTIFACT: &str = "E_COMPILER_REPORT_MISSING_ARTIFACT";

/// Stable issue code for outputs missing a content hash.
pub const E_COMPILER_REPORT_MISSING_HASH: &str = "E_COMPILER_REPORT_MISSING_HASH";

/// Stable issue code for outputs missing a compiler target.
pub const E_COMPILER_REPORT_MISSING_TARGET: &str = "E_COMPILER_REPORT_MISSING_TARGET";

/// Stable issue code for reports missing the compilation profile.
pub const E_COMPILER_REPORT_MISSING_PROFILE: &str = "E_COMPILER_REPORT_MISSING_PROFILE";

/// Stable issue code for report indexes containing the same output id twice.
pub const E_COMPILER_REPORT_DUPLICATE_OUTPUT: &str = "E_COMPILER_REPORT_DUPLICATE_OUTPUT";

// ── StageRecord ───────────────────────────────────────────────────────────

/// A record of one stage that ran during compilation.
///
/// `name` matches the pipeline stage identifier (e.g., `"graph-selection"`,
/// `"core-ir"`, `"anf"`, `"optimize"`, `"ssa"`, `"backend"`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageRecord {
    /// Stage name identifier.
    pub name: String,
    /// Whether the stage completed successfully.
    pub success: bool,
    /// Wall-clock duration of the stage in microseconds.
    ///
    /// `None` if timing was not captured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_us: Option<u64>,
}

// ── CompilerWarning ───────────────────────────────────────────────────────

/// A non-fatal warning produced during compilation.
///
/// Warnings do not prevent artifact generation but may indicate
/// suboptimal patterns, missing optimisation opportunities, or
/// recoverable inconsistencies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerWarning {
    /// Warning code (e.g., `"W_STUB_LOWERING"`, `"W_UNOPTIMIZED_EFFECT"`).
    pub code: String,
    /// Human-readable message.
    pub message: String,
    /// Scope (node name or stage) where the warning originated.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub scope: String,
}

// ── CompilerReport ────────────────────────────────────────────────────────

/// Structured compilation metadata produced alongside each compiled artifact.
///
/// Returned as part of the successful compilation output alongside
/// `WasmArtifact` or `NativeArtifact`.  Callers that only care about the
/// artifact can ignore this type; tooling can use it for diagnostics,
/// audit trails, and incremental build decisions.
///
/// All optional fields use `serde(default)` for backward-compatible
/// deserialization.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerReport {
    /// Compilation profile (e.g., `"prod"`, `"dev"`, `"draft"`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub profile: String,

    /// Hash of the `VerificationReport` that authorized this compilation.
    ///
    /// Empty string if the compilation was not authorized by a report
    /// (e.g., in tests or draft mode without full verification).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub verification_report_hash: String,

    /// Ordered list of stages that ran during compilation.
    ///
    /// Stages appear in pipeline order.  Empty for reports produced before
    /// stage tracking was added.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stages: Vec<StageRecord>,

    /// Non-fatal warnings emitted during compilation.
    ///
    /// Warnings are stored in deterministic `(code, scope, message)` order so
    /// reports do not depend on caller traversal order. Empty means no
    /// warnings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<CompilerWarning>,

    /// Total compilation wall-clock time in microseconds.
    ///
    /// `None` if timing was not captured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_duration_us: Option<u64>,

    /// Schema version of this report format.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub schema_version: String,
}

impl CompilerReport {
    /// Create a minimal report with a given profile string.
    pub fn new(profile: impl Into<String>) -> Self {
        Self {
            profile: profile.into(),
            schema_version: COMPILER_REPORT_SCHEMA_VERSION.into(),
            ..Default::default()
        }
    }

    /// Add a successfully completed stage record.
    pub fn add_stage(&mut self, name: impl Into<String>, duration_us: Option<u64>) {
        self.stages.push(StageRecord {
            name: name.into(),
            success: true,
            duration_us,
        });
    }

    /// Add a warning to this report.
    pub fn add_warning(
        &mut self,
        code: impl Into<String>,
        message: impl Into<String>,
        scope: impl Into<String>,
    ) {
        self.warnings.push(CompilerWarning {
            code: code.into(),
            message: message.into(),
            scope: scope.into(),
        });
        self.warnings.sort_by(|left, right| {
            (&left.code, &left.scope, &left.message).cmp(&(
                &right.code,
                &right.scope,
                &right.message,
            ))
        });
    }

    /// Return `true` if no warnings were emitted.
    pub fn is_clean(&self) -> bool {
        self.warnings.is_empty()
    }
}

// ── Compiler report validation ───────────────────────────────────────────

/// One compiler output plus its report-sidecar metadata for production gates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompilerReportValidationEntry<'a> {
    /// Stable output identifier from the build plan or package manifest.
    pub output_id: &'a str,
    /// Persisted artifact path or package artifact reference.
    pub artifact: Option<&'a str>,
    /// Backend target that produced the artifact, e.g. `wasm32` or `native`.
    pub target: Option<&'a str>,
    /// Content hash recorded for the emitted artifact.
    pub artifact_hash: Option<&'a str>,
    /// Compiler report sidecar emitted with the artifact.
    pub report: &'a CompilerReport,
}

impl<'a> CompilerReportValidationEntry<'a> {
    /// Build a validation entry for one compiler output/report pair.
    pub fn new(
        output_id: &'a str,
        artifact: Option<&'a str>,
        target: Option<&'a str>,
        artifact_hash: Option<&'a str>,
        report: &'a CompilerReport,
    ) -> Self {
        Self {
            output_id,
            artifact,
            target,
            artifact_hash,
            report,
        }
    }
}

/// Machine-readable compiler output/report diagnostic.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerReportValidationIssue {
    /// Stable issue code for downstream build gates.
    pub code: String,
    /// Redacted stable output identifier for the failing output.
    pub output_id: String,
    /// Output or report field that failed validation.
    pub field: String,
    /// Human-readable explanation that avoids embedding raw paths or hashes.
    pub message: String,
}

impl CompilerReportValidationIssue {
    fn new(
        code: &'static str,
        output_id: impl Into<String>,
        field: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.to_string(),
            output_id: redact_pathlike_id(&output_id.into()),
            field: field.to_string(),
            message: message.into(),
        }
    }

    fn sort_key(&self) -> (&str, &str, &str, &str) {
        (&self.code, &self.output_id, &self.field, &self.message)
    }
}

/// Validate compiler outputs and report sidecars before production packaging.
///
/// The diagnostics intentionally report stable machine codes and redacted
/// output ids, never raw artifact paths or hashes. Returned issues are sorted
/// and deduplicated so filesystem/package traversal order cannot affect
/// production build logs.
pub fn validate_compiler_report_entries(
    entries: &[CompilerReportValidationEntry<'_>],
) -> Vec<CompilerReportValidationIssue> {
    use std::collections::BTreeMap;

    let mut issues = Vec::new();
    let mut output_counts: BTreeMap<&str, usize> = BTreeMap::new();

    for entry in entries {
        *output_counts.entry(entry.output_id).or_default() += 1;

        if missing(entry.artifact) {
            issues.push(CompilerReportValidationIssue::new(
                E_COMPILER_REPORT_MISSING_ARTIFACT,
                entry.output_id,
                "artifact",
                "artifact reference is required for production packaging",
            ));
        }

        if missing(entry.artifact_hash) {
            issues.push(CompilerReportValidationIssue::new(
                E_COMPILER_REPORT_MISSING_HASH,
                entry.output_id,
                "artifact_hash",
                "artifact content hash is required for production packaging",
            ));
        }

        if missing(entry.target) {
            issues.push(CompilerReportValidationIssue::new(
                E_COMPILER_REPORT_MISSING_TARGET,
                entry.output_id,
                "target",
                "compiler target is required for production packaging",
            ));
        }

        if entry.report.profile.trim().is_empty() {
            issues.push(CompilerReportValidationIssue::new(
                E_COMPILER_REPORT_MISSING_PROFILE,
                entry.output_id,
                "profile",
                "compiler report profile is required for production packaging",
            ));
        }

        if entry.report.schema_version != COMPILER_REPORT_SCHEMA_VERSION {
            issues.push(CompilerReportValidationIssue::new(
                E_COMPILER_REPORT_STALE_SCHEMA,
                entry.output_id,
                "schema_version",
                "compiler report schema is stale; re-emit with the current compiler",
            ));
        }
    }

    for (output_id, count) in output_counts {
        if count > 1 {
            issues.push(CompilerReportValidationIssue::new(
                E_COMPILER_REPORT_DUPLICATE_OUTPUT,
                output_id,
                "output_id",
                format!("output id appears {count} times in compiler report index"),
            ));
        }
    }

    issues.sort_by(|left, right| left.sort_key().cmp(&right.sort_key()));
    issues.dedup();
    issues
}

fn missing(value: Option<&str>) -> bool {
    match value {
        Some(value) => value.trim().is_empty(),
        None => true,
    }
}

fn redact_pathlike_id(value: &str) -> String {
    let trimmed = value.trim();
    let Some(name) = trimmed.rsplit(['/', '\\']).next() else {
        return "<missing>".to_string();
    };

    if name.is_empty() {
        "<missing>".to_string()
    } else if name == trimmed {
        name.to_string()
    } else {
        format!("…/{name}")
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_report_new_sets_profile_and_schema_version() {
        let report = CompilerReport::new("prod");
        assert_eq!(report.profile, "prod");
        assert_eq!(report.schema_version, "compiler/1.0");
        assert!(report.stages.is_empty());
        assert!(report.warnings.is_empty());
        assert!(report.is_clean());
    }

    #[test]
    fn add_stage_records_name_and_timing() {
        let mut report = CompilerReport::new("dev");
        report.add_stage("graph-selection", Some(120));
        report.add_stage("core-ir", None);
        assert_eq!(report.stages.len(), 2);
        assert_eq!(report.stages[0].name, "graph-selection");
        assert_eq!(report.stages[0].duration_us, Some(120));
        assert!(report.stages[0].success);
        assert_eq!(report.stages[1].name, "core-ir");
        assert_eq!(report.stages[1].duration_us, None);
    }

    #[test]
    fn add_warning_records_code_message_scope() {
        let mut report = CompilerReport::new("draft");
        report.add_warning(
            "W_STUB_LOWERING",
            "concurrency ops are trap stubs",
            "fn.spawn",
        );
        assert_eq!(report.warnings.len(), 1);
        assert_eq!(report.warnings[0].code, "W_STUB_LOWERING");
        assert!(!report.is_clean());
    }

    #[test]
    fn add_warning_sorts_warnings_deterministically() {
        let mut report = CompilerReport::new("prod");
        report.add_warning("W_ZETA", "z message", "scope.b");
        report.add_warning("W_ALPHA", "a message", "scope.c");
        report.add_warning("W_ALPHA", "b message", "scope.a");

        let warning_keys: Vec<(&str, &str, &str)> = report
            .warnings
            .iter()
            .map(|warning| {
                (
                    warning.code.as_str(),
                    warning.scope.as_str(),
                    warning.message.as_str(),
                )
            })
            .collect();

        assert_eq!(
            warning_keys,
            vec![
                ("W_ALPHA", "scope.a", "b message"),
                ("W_ALPHA", "scope.c", "a message"),
                ("W_ZETA", "scope.b", "z message"),
            ]
        );
    }

    #[test]
    fn compiler_report_roundtrip_json() {
        let mut report = CompilerReport::new("staging");
        report.add_stage("anf", Some(250));
        report.add_warning("W_UNOPTIMIZED_EFFECT", "effect not inlined", "fn.checkout");
        report.total_duration_us = Some(1500);
        let json = serde_json::to_string(&report).expect("serialize");
        let decoded: CompilerReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, report);
    }

    #[test]
    fn compiler_report_default_deserializes_from_empty_object() {
        let json = r#"{"profile":"prod"}"#;
        let report: CompilerReport = serde_json::from_str(json).expect("deserialize");
        assert_eq!(report.profile, "prod");
        assert!(report.stages.is_empty());
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn validation_reports_missing_fields_and_stale_schema_redacted() {
        let mut report = CompilerReport::new(" ");
        report.schema_version = "compiler/0.9".to_string();

        let issues = validate_compiler_report_entries(&[CompilerReportValidationEntry::new(
            "/private/build/program.wasm",
            None,
            Some(""),
            None,
            &report,
        )]);

        let issue_keys: Vec<(&str, &str, &str)> = issues
            .iter()
            .map(|issue| {
                (
                    issue.code.as_str(),
                    issue.output_id.as_str(),
                    issue.field.as_str(),
                )
            })
            .collect();

        assert_eq!(
            issue_keys,
            vec![
                (
                    E_COMPILER_REPORT_MISSING_ARTIFACT,
                    "…/program.wasm",
                    "artifact",
                ),
                (
                    E_COMPILER_REPORT_MISSING_HASH,
                    "…/program.wasm",
                    "artifact_hash",
                ),
                (
                    E_COMPILER_REPORT_MISSING_PROFILE,
                    "…/program.wasm",
                    "profile",
                ),
                (E_COMPILER_REPORT_MISSING_TARGET, "…/program.wasm", "target",),
                (
                    E_COMPILER_REPORT_STALE_SCHEMA,
                    "…/program.wasm",
                    "schema_version",
                ),
            ]
        );
        assert!(
            issues
                .iter()
                .all(|issue| !issue.message.contains("/private/build")),
            "diagnostics must not leak raw artifact paths: {issues:?}"
        );
    }

    #[test]
    fn validation_reports_duplicate_outputs_once_and_dedups() {
        let report = CompilerReport::new("prod");

        let issues = validate_compiler_report_entries(&[
            CompilerReportValidationEntry::new("program.wasm", None, None, None, &report),
            CompilerReportValidationEntry::new("program.wasm", None, None, None, &report),
        ]);

        let issue_keys: Vec<(&str, &str)> = issues
            .iter()
            .map(|issue| (issue.code.as_str(), issue.field.as_str()))
            .collect();

        assert_eq!(
            issue_keys,
            vec![
                (E_COMPILER_REPORT_DUPLICATE_OUTPUT, "output_id"),
                (E_COMPILER_REPORT_MISSING_ARTIFACT, "artifact"),
                (E_COMPILER_REPORT_MISSING_HASH, "artifact_hash"),
                (E_COMPILER_REPORT_MISSING_TARGET, "target"),
            ]
        );
    }

    #[test]
    fn validation_orders_issues_deterministically() {
        let mut stale = CompilerReport::new("prod");
        stale.schema_version = "compiler/0.8".to_string();
        let complete = CompilerReport::new("prod");

        let issues = validate_compiler_report_entries(&[
            CompilerReportValidationEntry::new("zeta.wasm", Some("zeta.wasm"), None, None, &stale),
            CompilerReportValidationEntry::new(
                "alpha.wasm",
                Some("alpha.wasm"),
                Some("wasm32"),
                Some("hash-a"),
                &complete,
            ),
            CompilerReportValidationEntry::new(
                "alpha.wasm",
                Some("alpha.wasm"),
                Some("wasm32"),
                Some("hash-a"),
                &complete,
            ),
        ]);
        let reversed_issues = validate_compiler_report_entries(&[
            CompilerReportValidationEntry::new(
                "alpha.wasm",
                Some("alpha.wasm"),
                Some("wasm32"),
                Some("hash-a"),
                &complete,
            ),
            CompilerReportValidationEntry::new(
                "alpha.wasm",
                Some("alpha.wasm"),
                Some("wasm32"),
                Some("hash-a"),
                &complete,
            ),
            CompilerReportValidationEntry::new("zeta.wasm", Some("zeta.wasm"), None, None, &stale),
        ]);

        let sorted_issue_keys = issues
            .windows(2)
            .all(|pair| pair[0].sort_key() <= pair[1].sort_key());

        assert!(
            sorted_issue_keys,
            "compiler report validation issues must have deterministic ordering: {issues:?}"
        );
        assert_eq!(
            issues, reversed_issues,
            "validation issues must not depend on compiler output traversal order"
        );
    }
}
