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

use ail_core::semantic_graph::{NodeKind, SemanticGraph};

use crate::report::{
    ArtifactHash, SummaryCounts, VerificationEntry, VerificationReport, VerificationState,
};

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

            let blocking =
                matches!(state, VerificationState::Failed | VerificationState::Unsafe);
            entries.push(VerificationEntry {
                claim,
                state,
                scope,
                evidence,
                blocking,
            });
        }

        let summary_counts = compute_counts(&entries);
        let artifact_hashes = artifacts
            .iter()
            .map(|a| ArtifactHash {
                artifact: a.name.clone(),
                hash: a.actual_hash.clone(),
            })
            .collect();

        VerificationReport {
            entries,
            schema_version: "verification/1.0".into(),
            summary_counts,
            artifact_hashes,
            ..Default::default()
        }
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

            let blocking =
                matches!(state, VerificationState::Failed | VerificationState::Unsafe);
            entries.push(VerificationEntry {
                claim,
                state,
                scope,
                evidence,
                blocking,
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
            });
        }

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
