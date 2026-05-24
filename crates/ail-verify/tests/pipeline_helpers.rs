// ── Shared helpers for pipeline integration tests ─────────────────────────
//
// Extracted from pipeline.rs. Used by pipeline_core, pipeline_tasks,
// and pipeline_integration test binaries via `mod pipeline_helpers;`.
// Dead-code and unused-import lints are expected when this file is compiled
// as a standalone (empty) test binary.
#![allow(dead_code)]

use ail_core::semantic_graph::SemanticGraph;
use ail_package::assumption::{AssumptionState, PackageAssumption};
use ail_package::manifest::{PackageDef, PackageManifest};
use ail_package::trust::TrustLevel as PackageTrustLevel;
use ail_verify::pipeline::PipelineContext;
use ail_verify::policy::{ApprovalRecord, ApprovalStrength, PolicyRule};
use ail_verify::solver::SimpleSolver;

pub fn empty_graph() -> SemanticGraph {
    SemanticGraph {
        nodes: vec![],
        edges: vec![],
    }
}

pub fn make_ctx<'a>(
    graph: &'a SemanticGraph,
    solver: &'a SimpleSolver,
    profile: &'a str,
    rules: &'a [PolicyRule],
) -> PipelineContext<'a> {
    PipelineContext {
        graph,
        manifests: &[],
        profile,
        solver,
        approvals: &[],
        rules,
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
        artifacts: &[],
        manifest_caps: &[],
        artifact_manifest_hash: None,
    }
}

pub fn assumed_package_manifest() -> PackageManifest {
    PackageManifest::from_def(PackageDef {
        name: "payments.stripe".into(),
        version: "2.3.1".into(),
        trust_level: PackageTrustLevel::Assumed,
        required_capabilities: vec![],
        exported_capabilities: vec![],
        assumptions: vec![PackageAssumption {
            id: "stripe_idempotency".into(),
            claim: "Stripe honors idempotency keys".into(),
            boundary: "boundary.Stripe".into(),
            owner: "team.payments".into(),
            expires: Some("2026-12-31".into()),
            state: AssumptionState::Active,
        }],
        unsafe_surface: vec![],
        artifact_hashes: vec![],
        build_env_hash: None,
        handlers: vec![],
        contracts: vec![],
        exports: vec![],
        imports: vec![],
        boundaries: vec!["boundary.Stripe".into()],
        license: None,
        provenance: None,
        verification_report: None,
        graph_schema: None,
        core_ir_schema: None,
        // 4G fields
        reproducible_evidence: None,
    })
}

pub fn strong_approval(scope: &str) -> ApprovalRecord {
    ApprovalRecord {
        scope: scope.into(),
        approver: "security-team".into(),
        reason: "approved active package assumption".into(),
        strength: ApprovalStrength::Strong,
    }
}

pub fn unsafe_package_manifest() -> PackageManifest {
    PackageManifest::from_def(PackageDef {
        name: "sketchy.ffi".into(),
        version: "0.1.0".into(),
        trust_level: PackageTrustLevel::Unsafe,
        required_capabilities: vec![],
        exported_capabilities: vec![],
        assumptions: vec![],
        unsafe_surface: vec![],
        artifact_hashes: vec![],
        build_env_hash: None,
        handlers: vec![],
        contracts: vec![],
        exports: vec![],
        imports: vec![],
        boundaries: vec![],
        license: None,
        provenance: None,
        verification_report: None,
        graph_schema: None,
        core_ir_schema: None,
        reproducible_evidence: None,
    })
}

pub fn unverified_package_manifest() -> PackageManifest {
    PackageManifest::from_def(PackageDef {
        name: "experimental.lib".into(),
        version: "1.0.0".into(),
        trust_level: PackageTrustLevel::Unverified,
        required_capabilities: vec![],
        exported_capabilities: vec![],
        assumptions: vec![],
        unsafe_surface: vec![],
        artifact_hashes: vec![],
        build_env_hash: None,
        handlers: vec![],
        contracts: vec![],
        exports: vec![],
        imports: vec![],
        boundaries: vec![],
        license: None,
        provenance: None,
        verification_report: None,
        graph_schema: None,
        core_ir_schema: None,
        reproducible_evidence: None,
    })
}
