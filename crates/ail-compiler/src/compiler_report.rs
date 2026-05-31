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
            schema_version: "compiler/1.0".into(),
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
}
