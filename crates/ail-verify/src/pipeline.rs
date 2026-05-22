// ── ail-verify::pipeline ─────────────────────────────────────────────────
//
// Canonical verification pipeline facade — sequences all checkers in the
// order specified by verification.md and merges their outputs into one
// `VerificationReport`.
//
// # Pipeline order (verification.md §"Pipeline completo")
//
// The full 23-step pipeline is split between compile-time phases and the
// runtime verification phase.  `VerificationPipeline` covers the runtime
// verification steps relevant to `ail-verify`:
//
//  7.  Type check          (`TypeChecker`)
//  8.  Effect/capability   (`Checker` + `EffectChecker`)
//  9.  Generate obligations + contract check (`ContractChecker`, `ProofObligationPipeline`)
// 10.  Refinements         (obligation ledger — part of proof pipeline)
// 11.  Contracts           (`ContractChecker`)
// 13.  Resource lifecycle  (`ResourceChecker`)
// 14.  Concurrency safety  (`ConcurrencyChecker`)
// 15.  Boundary/FFI trust  (`BoundaryChecker`)
// 16.  Package trust       (`PackageTrustChecker`)
// 17.  Policy gates        (`PolicyEngine`)
// 22.  Codegen consistency (`CodegenChecker`)
//
// # Merge strategy
//
// Each stage returns a `VerificationReport`.  The pipeline merges them by
// concatenating `entries` and `diagnostics` in stage order, then runs the
// policy engine once over the merged entries.  The final `policy_decision`
// and `policy_audit` fields are set on the merged report.
//
// `proof_obligations` from `ProofObligationPipeline::run_with_ledger` are
// stored in the merged report's `proof_obligations` field.
//
// # Determinism
//
// All stages are pure.  The pipeline produces identical output for identical
// inputs.

use ail_package::manifest::PackageManifest;

use ail_core::semantic_graph::SemanticGraph;

use crate::boundary_checker::BoundaryChecker;
use crate::checker::Checker;
use crate::codegen_checker::{ArtifactEntry, CodegenChecker};
use crate::concurrency_checker::ConcurrencyChecker;
use crate::contract_checker::ContractChecker;
use crate::effect_checker::EffectChecker;
use crate::package_checker::PackageTrustChecker;
use crate::policy::{
    ApprovalRecord, CapabilityGrant, PackageTrustEntry, PolicyEngine, PolicyInput, PolicyRule,
    PublicApiChange, StructuralDiff,
};
use crate::proof::ProofObligationPipeline;
use crate::report::{DegradationEvent, VerificationReport, VerificationState};
use crate::resource_checker::ResourceChecker;
use crate::solver::Solver;
use crate::type_checker::TypeChecker;

// ── PipelineContext ───────────────────────────────────────────────────────

/// Input bundle for `VerificationPipeline::run`.
///
/// All fields are borrowed references; the pipeline is pure and does not
/// mutate or store any of them.
pub struct PipelineContext<'a> {
    /// The semantic graph to verify.
    pub graph: &'a SemanticGraph,
    /// Package manifests for package trust checking.
    pub manifests: &'a [PackageManifest],
    /// Verification profile name (e.g. `"prod"`, `"dev"`, `"draft"`).
    pub profile: &'a str,
    /// Solver for proof obligation evaluation.
    pub solver: &'a dyn Solver,
    /// Approval records that can satisfy policy rules.
    pub approvals: &'a [ApprovalRecord],
    /// Ordered list of policy rules to evaluate.
    pub rules: &'a [PolicyRule],
    /// Optional structural diff for policy assessment.
    pub structural_diff: Option<&'a StructuralDiff>,
    /// Capability grants active for this changeset.
    pub capability_grants: &'a [CapabilityGrant],
    /// Public API changes in this changeset.
    pub public_api_changes: &'a [PublicApiChange],
    /// Package trust metadata for dependency checks.
    pub package_trust_metadata: &'a [PackageTrustEntry],
    /// Artifact entries for codegen consistency checking.
    ///
    /// Pass an empty slice to skip codegen hash checking.
    pub artifacts: &'a [ArtifactEntry],
    /// Capability names from the capabilities manifest for consistency checks.
    ///
    /// Pass an empty slice to skip manifest consistency checking.
    pub manifest_caps: &'a [String],
}

// ── VerificationPipeline ──────────────────────────────────────────────────

/// Canonical verification pipeline facade.
///
/// `VerificationPipeline::run` sequences all checker stages in the order
/// defined by `verification.md`, merges their `VerificationReport` outputs,
/// and runs the policy engine once over the merged result.
pub struct VerificationPipeline;

impl VerificationPipeline {
    /// Run the full verification pipeline over `ctx` and return the merged report.
    ///
    /// The returned report contains:
    /// - `entries` — all verification entries from all stages in order
    /// - `diagnostics` — all diagnostics from all stages in order
    /// - `proof_obligations` — obligation ledger from the proof pipeline
    /// - `degradation_events` — degradation events inferred from Assumed entries
    /// - `artifact_hashes` — artifact hash entries from codegen checker
    /// - `policy_decision` — result of `PolicyEngine::evaluate`
    /// - `policy_audit` — per-entry audit trail
    /// - `summary_counts` — aggregated counts from merged entries
    ///
    /// # Determinism
    ///
    /// Identical `PipelineContext` inputs produce identical output.
    pub fn run(ctx: &PipelineContext<'_>) -> VerificationReport {
        let mut all_entries = Vec::new();
        let mut all_diagnostics = Vec::new();

        // ── Stage 7: Type check ───────────────────────────────────────────
        let type_report = TypeChecker::check(ctx.graph);
        all_entries.extend(type_report.entries);
        all_diagnostics.extend(type_report.diagnostics);

        // ── Stage 8: Effect/capability check ─────────────────────────────
        let effect_report = Checker::check(ctx.graph);
        all_entries.extend(effect_report.entries);
        all_diagnostics.extend(effect_report.diagnostics);

        // Stage 8b: detailed effect checker
        let effect_detail_report = EffectChecker::check(ctx.graph);
        all_entries.extend(effect_detail_report.entries);
        all_diagnostics.extend(effect_detail_report.diagnostics);

        // ── Stage 9+11: Contract check ────────────────────────────────────
        let contract_checker = ContractChecker::new(ctx.solver);
        let contract_report = contract_checker.check(ctx.graph);
        all_entries.extend(contract_report.entries);
        all_diagnostics.extend(contract_report.diagnostics);

        // ── Stage 10: Proof obligation pipeline ───────────────────────────
        let proof_obligations = ProofObligationPipeline::run_with_ledger(ctx.graph, ctx.solver);

        // ── Stage 13: Resource lifecycle ──────────────────────────────────
        let resource_report = ResourceChecker::check(ctx.graph);
        all_entries.extend(resource_report.entries);
        all_diagnostics.extend(resource_report.diagnostics);

        // ── Stage 14: Concurrency safety ──────────────────────────────────
        let concurrency_report = ConcurrencyChecker::check(ctx.graph);
        all_entries.extend(concurrency_report.entries);
        all_diagnostics.extend(concurrency_report.diagnostics);

        // ── Stage 15: Boundary/FFI trust ──────────────────────────────────
        let boundary_report = BoundaryChecker::check(ctx.graph);
        all_entries.extend(boundary_report.entries);
        all_diagnostics.extend(boundary_report.diagnostics);

        // ── Stage 16: Package trust ───────────────────────────────────────
        let package_entries = PackageTrustChecker::check(ctx.manifests, ctx.profile);
        all_entries.extend(package_entries);

        let mut artifact_hashes = Vec::new();

        // ── Compute summary counts ────────────────────────────────────────
        let summary_counts = crate::report::SummaryCounts {
            verified_count: all_entries
                .iter()
                .filter(|e| {
                    e.state == VerificationState::Proven
                        || e.state == VerificationState::RuntimeChecked
                })
                .count(),
            runtime_checked_count: all_entries
                .iter()
                .filter(|e| e.state == VerificationState::RuntimeChecked)
                .count(),
            assumed_count: all_entries
                .iter()
                .filter(|e| e.state == VerificationState::Assumed)
                .count(),
            unverified_count: all_entries
                .iter()
                .filter(|e| e.state == VerificationState::Unverified)
                .count(),
            unsafe_count: all_entries
                .iter()
                .filter(|e| e.state == VerificationState::Unsafe)
                .count(),
            failed_count: all_entries
                .iter()
                .filter(|e| e.state == VerificationState::Failed)
                .count(),
        };

        // ── Infer degradation events from Assumed entries ─────────────────
        let degradation_events: Vec<DegradationEvent> = proof_obligations
            .iter()
            .filter_map(|entry| match &entry.state {
                crate::proof::ObligationState::Assumed(reason) => Some(DegradationEvent {
                    obligation_id: entry.id.clone(),
                    source_stage: entry.source_stage.clone(),
                    from_state: VerificationState::Proven, // would have been proven
                    to_state: VerificationState::Assumed,
                    reason: reason.clone(),
                    repair_options: entry.repair_options.clone(),
                }),
                _ => None,
            })
            .collect();

        // ── Assemble pre-policy report ────────────────────────────────────
        let mut pre_policy = VerificationReport {
            entries: all_entries,
            diagnostics: all_diagnostics,
            schema_version: "verification/1.0".into(),
            summary_counts,
            proof_obligations,
            degradation_events,
            artifact_hashes: vec![],
            ..Default::default()
        };

        // ── Stage 17: Policy ──────────────────────────────────────────────
        let policy_input = PolicyInput {
            report: &pre_policy,
            rules: ctx.rules,
            approvals: ctx.approvals,
            structural_diff: ctx.structural_diff,
            capability_grants: ctx.capability_grants,
            public_api_changes: ctx.public_api_changes,
            package_trust_metadata: ctx.package_trust_metadata,
        };
        let (policy_decision, policy_audit) = PolicyEngine::evaluate_with_audit(&policy_input);

        // ── Stage 22: Codegen consistency ─────────────────────────────────
        // verification.md orders policy before post-lowering/codegen checks.
        // Codegen diagnostics are appended after policy evaluation so policy
        // decisions are based on verifier facts rather than backend artifacts.
        if !ctx.artifacts.is_empty() {
            let codegen_report = CodegenChecker::check_artifacts(ctx.artifacts);
            pre_policy.entries.extend(codegen_report.entries);
            pre_policy.diagnostics.extend(codegen_report.diagnostics);
            artifact_hashes.extend(codegen_report.artifact_hashes);
        }
        if !ctx.manifest_caps.is_empty() {
            let manifest_report =
                CodegenChecker::check_manifest_consistency(ctx.graph, ctx.manifest_caps);
            pre_policy.entries.extend(manifest_report.entries);
            pre_policy.diagnostics.extend(manifest_report.diagnostics);
        }
        pre_policy.artifact_hashes = artifact_hashes;
        pre_policy.summary_counts = crate::report::SummaryCounts {
            verified_count: pre_policy
                .entries
                .iter()
                .filter(|e| {
                    e.state == VerificationState::Proven
                        || e.state == VerificationState::RuntimeChecked
                })
                .count(),
            runtime_checked_count: pre_policy
                .entries
                .iter()
                .filter(|e| e.state == VerificationState::RuntimeChecked)
                .count(),
            assumed_count: pre_policy
                .entries
                .iter()
                .filter(|e| e.state == VerificationState::Assumed)
                .count(),
            unverified_count: pre_policy
                .entries
                .iter()
                .filter(|e| e.state == VerificationState::Unverified)
                .count(),
            unsafe_count: pre_policy
                .entries
                .iter()
                .filter(|e| e.state == VerificationState::Unsafe)
                .count(),
            failed_count: pre_policy
                .entries
                .iter()
                .filter(|e| e.state == VerificationState::Failed)
                .count(),
        };

        VerificationReport {
            policy_decision: Some(policy_decision),
            policy_audit: Some(policy_audit),
            ..pre_policy
        }
    }
}
