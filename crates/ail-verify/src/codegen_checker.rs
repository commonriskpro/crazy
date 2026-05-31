// ── ail-verify::codegen_checker ───────────────────────────────────────────
//
// Codegen consistency checker — verification layer 12 per verification.md.
//
// # Responsibility
//
// `CodegenChecker` verifies that generated artifacts correspond to the IR
// that was verified.  Two surfaces:
//
// 1. `check_artifacts` — compares declared vs. actual artifact hashes.
//    Any mismatch → `Failed`.  Matching pairs → `Proven`.
//
// 2. `check_manifest_consistency` — verifies that `NodeKind::Capability`
//    nodes in the semantic graph match the capabilities listed in the
//    capabilities manifest.  Missing from manifest → `Failed`.
//    Present in manifest → `Proven`.
//
// # Rules (from verification.md)
//
// - artifact hash mismatch → `failed`
// - manifest extra capability → `failed`
// - WASM imports not in manifest → `failed`
// - report cannot authorize changed artifacts
// - `canonical_change`, `graph_diff`, `core_ir`, `anf_ir` hashes must match report
// - WASM/imports must match capabilities_manifest
// - capabilities_manifest must match effect analysis

use ail_core::semantic_graph::{NodeKind, NodeRef, SemanticGraph};

use crate::diagnostic::{Diagnostic, DiagnosticSeverity, RepairOption};
use crate::report::{
    ArtifactHash, SummaryCounts, VerificationEntry, VerificationReport, VerificationState,
};

// ── Stable codegen diagnostic codes ───────────────────────────────────────

/// Requested codegen backend is not in the verifier's supported backend set.
pub const VERIFY_CODEGEN_UNSUPPORTED_BACKEND: &str = "VERIFY_CODEGEN_UNSUPPORTED_BACKEND";

/// Generated artifact hash does not match the verifier's expected hash.
pub const VERIFY_CODEGEN_ARTIFACT_MISMATCH: &str = "VERIFY_CODEGEN_ARTIFACT_MISMATCH";

/// Production codegen verification is missing proof-obligation evidence.
pub const VERIFY_CODEGEN_MISSING_PROOF_EVIDENCE: &str = "VERIFY_CODEGEN_MISSING_PROOF_EVIDENCE";

/// Production codegen verification is missing translation-validation evidence.
pub const VERIFY_CODEGEN_MISSING_TRANSLATION_EVIDENCE: &str =
    "VERIFY_CODEGEN_MISSING_TRANSLATION_EVIDENCE";

/// Codegen output was reported as nondeterministic.
pub const VERIFY_CODEGEN_NONDETERMINISTIC_OUTPUT: &str = "VERIFY_CODEGEN_NONDETERMINISTIC_OUTPUT";

const CODEGEN_DIAGNOSTIC_TARGET: NodeRef = NodeRef(0);

// ── ArtifactEntry ─────────────────────────────────────────────────────────

/// One artifact with its declared (expected) hash and actual (computed) hash.
///
/// The codegen checker compares `expected_hash` with `actual_hash` to
/// determine whether the artifact corresponds to the verified IR.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactEntry {
    /// Artifact name (e.g. `"canonical_change"`, `"core_ir"`, `"anf_ir"`, `"wasm"`).
    pub name: String,
    /// Hash declared in the verification report.
    pub expected_hash: String,
    /// Hash actually computed from the artifact.
    pub actual_hash: String,
}

// ── CodegenVerificationContext ───────────────────────────────────────────

/// Production-oriented codegen verification inputs.
///
/// This augments artifact hash checks with the compiler-verifier evidence
/// needed before generated output can be accepted as production-grade:
/// supported backend, proof evidence, translation evidence, and determinism.
#[derive(Clone, Copy, Debug)]
pub struct CodegenVerificationContext<'a> {
    /// Requested output backend, e.g. `"wasm"`.
    pub backend: &'a str,
    /// Backends this verifier can validate for production acceptance.
    pub supported_backends: &'a [&'a str],
    /// Declared vs. actual generated artifact hashes.
    pub artifacts: &'a [ArtifactEntry],
    /// Whether proof-obligation evidence was attached to the verifier report.
    pub proof_evidence_present: bool,
    /// Whether translation-validation evidence was attached to the verifier report.
    pub translation_evidence_present: bool,
    /// `Some(false)` means codegen reported nondeterministic output.
    /// `None` means the backend has not represented determinism yet.
    pub output_deterministic: Option<bool>,
}

// ── CodegenChecker ────────────────────────────────────────────────────────

/// Pure, stateless codegen consistency checker.
pub struct CodegenChecker;

impl CodegenChecker {
    /// Compare declared vs. actual artifact hashes.
    ///
    /// For each `ArtifactEntry`:
    /// - `expected_hash == actual_hash` → `Proven`
    /// - `expected_hash != actual_hash` → `Failed`
    /// - `expected_hash.is_empty() || actual_hash.is_empty()` → `Unverified`
    ///
    /// Returns a `VerificationReport` with one entry per artifact.
    /// An empty `artifacts` slice returns an empty report.
    pub fn check_artifacts(artifacts: &[ArtifactEntry]) -> VerificationReport {
        let mut entries = Vec::new();

        for artifact in artifacts {
            let scope = format!("artifact:{}", artifact.name);
            let claim = "codegen-consistency".to_string();

            let (state, evidence) =
                if artifact.expected_hash.is_empty() || artifact.actual_hash.is_empty() {
                    (
                        VerificationState::Unverified,
                        Some(format!(
                            "artifact '{}' hash not computed; cannot verify consistency",
                            artifact.name
                        )),
                    )
                } else if artifact.expected_hash == artifact.actual_hash {
                    (VerificationState::Proven, None)
                } else {
                    (
                        VerificationState::Failed,
                        Some(format!(
                            "artifact '{}' hash mismatch: expected '{}', actual '{}'",
                            artifact.name, artifact.expected_hash, artifact.actual_hash
                        )),
                    )
                };

            let blocking = matches!(state, VerificationState::Failed | VerificationState::Unsafe);
            entries.push(VerificationEntry {
                claim,
                state,
                scope,
                evidence,
                blocking,
                repair_options: vec![],
            });
        }

        normalize_codegen_entries(&mut entries);
        let diagnostics = artifact_diagnostics(artifacts);
        let summary_counts = compute_counts(&entries);
        let mut artifact_hashes: Vec<ArtifactHash> = artifacts
            .iter()
            .map(|a| ArtifactHash {
                artifact: a.name.clone(),
                hash: a.actual_hash.clone(),
            })
            .collect();
        normalize_artifact_hashes(&mut artifact_hashes);

        VerificationReport {
            entries,
            diagnostics,
            schema_version: "verification/1.0".into(),
            summary_counts,
            artifact_hashes,
            ..Default::default()
        }
    }

    /// Run production-oriented codegen diagnostics as one vertical slice.
    ///
    /// The report includes normal artifact consistency entries plus stable,
    /// redacted diagnostics for production acceptance blockers. Exact
    /// duplicate diagnostics are removed and the remaining diagnostics are
    /// sorted by stable machine fields.
    pub fn check_production_codegen(ctx: CodegenVerificationContext<'_>) -> VerificationReport {
        let mut report = Self::check_artifacts(ctx.artifacts);

        if !ctx
            .supported_backends
            .iter()
            .any(|backend| *backend == ctx.backend)
        {
            report.diagnostics.push(codegen_diagnostic(
                VERIFY_CODEGEN_UNSUPPORTED_BACKEND,
                "requested codegen backend is not supported by production verifier",
                "supported codegen backend",
                "unsupported codegen backend",
                "choose a verifier-supported backend or add backend verification coverage",
            ));
        }

        if !ctx.proof_evidence_present {
            report.diagnostics.push(codegen_diagnostic(
                VERIFY_CODEGEN_MISSING_PROOF_EVIDENCE,
                "production codegen verification is missing proof-obligation evidence",
                "proof evidence attached",
                "proof evidence missing",
                "attach proof-obligation ledger evidence before accepting generated output",
            ));
        }

        if !ctx.translation_evidence_present {
            report.diagnostics.push(codegen_diagnostic(
                VERIFY_CODEGEN_MISSING_TRANSLATION_EVIDENCE,
                "production codegen verification is missing translation-validation evidence",
                "translation evidence attached",
                "translation evidence missing",
                "attach translation-validation evidence before accepting generated output",
            ));
        }

        if ctx.output_deterministic == Some(false) {
            report.diagnostics.push(codegen_diagnostic(
                VERIFY_CODEGEN_NONDETERMINISTIC_OUTPUT,
                "codegen reported nondeterministic output for the verified input",
                "deterministic output",
                "nondeterministic output",
                "stabilize codegen ordering before accepting generated output",
            ));
        }

        normalize_codegen_diagnostics(&mut report.diagnostics);
        report
    }

    /// Verify that `NodeKind::Capability` nodes in `graph` match `manifest_caps`.
    ///
    /// For each capability node in the graph:
    /// - If the node's name is in `manifest_caps` → `Proven`
    /// - If not → `Failed` (WASM/manifest imports do not cover declared capabilities)
    ///
    /// For each entry in `manifest_caps` not covered by any graph capability node:
    /// - Emits an additional `Failed` entry (manifest has extra capabilities not
    ///   declared in the graph).
    pub fn check_manifest_consistency(
        graph: &SemanticGraph,
        manifest_caps: &[String],
    ) -> VerificationReport {
        let mut entries = Vec::new();

        // Check graph capability nodes against manifest
        let mut manifest_cap_set: Vec<&str> = manifest_caps.iter().map(|s| s.as_str()).collect();

        for node in &graph.nodes {
            if node.kind != NodeKind::Capability {
                continue;
            }

            let scope = format!("capability:{}", node.name);
            let claim = "manifest-consistency".to_string();

            let pos = manifest_cap_set.iter().position(|&c| c == node.name);
            let (state, evidence) = if let Some(idx) = pos {
                manifest_cap_set.remove(idx);
                (VerificationState::Proven, None)
            } else {
                (
                    VerificationState::Failed,
                    Some(format!(
                        "capability '{}' declared in graph but not present in capabilities manifest; manifest mismatch",
                        node.name
                    )),
                )
            };

            let blocking = matches!(state, VerificationState::Failed | VerificationState::Unsafe);
            entries.push(VerificationEntry {
                claim,
                state,
                scope,
                evidence,
                blocking,
                repair_options: vec![],
            });
        }

        // Remaining manifest entries have no corresponding graph capability → extra
        for extra_cap in manifest_cap_set {
            entries.push(VerificationEntry {
                claim: "manifest-consistency".to_string(),
                state: VerificationState::Failed,
                scope: format!("capability:{}", extra_cap),
                evidence: Some(format!(
                    "capability '{}' is in capabilities manifest but not declared in the semantic graph; manifest mismatch",
                    extra_cap
                )),
                blocking: true,
                repair_options: vec![],
            });
        }

        normalize_codegen_entries(&mut entries);
        let summary_counts = compute_counts(&entries);
        VerificationReport {
            entries,
            schema_version: "verification/1.0".into(),
            summary_counts,
            ..Default::default()
        }
    }
}

// ── helpers ───────────────────────────────────────────────────────────────

fn artifact_diagnostics(artifacts: &[ArtifactEntry]) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for artifact in artifacts {
        if artifact.expected_hash.is_empty() || artifact.actual_hash.is_empty() {
            continue;
        }

        if artifact.expected_hash != artifact.actual_hash {
            diagnostics.push(codegen_diagnostic(
                VERIFY_CODEGEN_ARTIFACT_MISMATCH,
                "generated artifact hash does not match verifier expectation",
                redacted_hash_label(&artifact.expected_hash),
                redacted_hash_label(&artifact.actual_hash),
                "regenerate the artifact from the verified IR or reject the changed output",
            ));
        }
    }

    normalize_codegen_diagnostics(&mut diagnostics);
    diagnostics
}

fn codegen_diagnostic(
    code: &'static str,
    evidence: impl Into<String>,
    expected: impl Into<String>,
    actual: impl Into<String>,
    repair: impl Into<String>,
) -> Diagnostic {
    Diagnostic {
        code: code.to_string(),
        severity: DiagnosticSeverity::Error,
        target: CODEGEN_DIAGNOSTIC_TARGET,
        evidence: Some(evidence.into()),
        expected: Some(expected.into()),
        actual: Some(actual.into()),
        repair_options: vec![RepairOption::Explanation(repair.into())],
        blocking: true,
    }
}

fn redacted_hash_label(hash: &str) -> String {
    if hash.is_empty() {
        "hash:<missing>".into()
    } else {
        "hash:<redacted>".into()
    }
}

fn normalize_codegen_entries(entries: &mut Vec<VerificationEntry>) {
    entries.sort_by(|a, b| {
        codegen_claim_rank(&a.claim)
            .cmp(&codegen_claim_rank(&b.claim))
            .then_with(|| a.scope.cmp(&b.scope))
            .then_with(|| verification_state_rank(a.state).cmp(&verification_state_rank(b.state)))
            .then_with(|| a.evidence.cmp(&b.evidence))
            .then_with(|| a.repair_options.cmp(&b.repair_options))
    });
    entries.dedup();
}

fn normalize_codegen_diagnostics(diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.sort_by(|a, b| {
        a.code
            .cmp(&b.code)
            .then_with(|| a.target.0.cmp(&b.target.0))
            .then_with(|| {
                diagnostic_severity_rank(a.severity).cmp(&diagnostic_severity_rank(b.severity))
            })
            .then_with(|| a.evidence.cmp(&b.evidence))
            .then_with(|| a.expected.cmp(&b.expected))
            .then_with(|| a.actual.cmp(&b.actual))
            .then_with(|| a.repair_options.cmp(&b.repair_options))
            .then_with(|| a.blocking.cmp(&b.blocking))
    });
    diagnostics.dedup();
}

fn normalize_artifact_hashes(artifact_hashes: &mut Vec<ArtifactHash>) {
    artifact_hashes.sort_by(|a, b| {
        a.artifact
            .cmp(&b.artifact)
            .then_with(|| a.hash.cmp(&b.hash))
    });
    artifact_hashes.dedup();
}

fn codegen_claim_rank(claim: &str) -> u8 {
    match claim {
        "codegen-consistency" => 0,
        "manifest-consistency" => 1,
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

fn diagnostic_severity_rank(severity: DiagnosticSeverity) -> u8 {
    match severity {
        DiagnosticSeverity::Error => 0,
        DiagnosticSeverity::Warning => 1,
        DiagnosticSeverity::Info => 2,
    }
}

fn compute_counts(entries: &[VerificationEntry]) -> SummaryCounts {
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
