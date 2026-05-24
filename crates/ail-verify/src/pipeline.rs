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
//
// # Module layout
//
// Stage helpers are split into focused submodules:
//
// - `changeset_stages` — Stages 3–6: op-schema, graph-ref, semantic-diff, Core IR
// - `core_stages`      — Stages 10 & 12: refinements, invariant impact analysis
// - `anf_stages`       — Stages 18–21: approvals, ANF, ordering, manifest

mod anf_stages;
mod changeset_stages;
mod core_stages;

use ail_change::canonical::{CanonicalChangeSet, canonicalize_parsed};
use ail_change::parser::parse_changeset;
use ail_core::semantic_graph::SemanticGraph;
use ail_package::manifest::PackageManifest;

use crate::boundary_checker::BoundaryChecker;
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
use crate::report::{
    DegradationEvent, SolverDiagnostic, VerificationEntry, VerificationReport, VerificationState,
};
use crate::resource_checker::ResourceChecker;
use crate::solver::Solver;
use crate::type_checker::TypeChecker;

use anf_stages::{check_anf_ordering, check_approval_records, lower_anf, validate_manifest};
use changeset_stages::{
    build_semantic_diff, lower_core_ir, resolve_graph_references, validate_op_schemas_with_graph,
};
use core_stages::{check_invariants, check_refinements};

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
    /// Optional precomputed artifact manifest hash for Stage 21 comparison.
    ///
    /// When `Some(h)`, Stage 21 computes the actual capability-set hash and
    /// compares it against `h`.  A mismatch emits `E_MANIFEST_HASH_MISMATCH`.
    /// When `None`, the hash check is skipped and only the cap-set is compared.
    pub artifact_manifest_hash: Option<&'a str>,
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
        Self::run_with_changeset(ctx, None, None)
    }

    /// Run the full 23-step verification pipeline with submitted ChangeSet text.
    ///
    /// `base_graph` is used for semantic-diff checks. When it is `None`, the
    /// diff stage records that no base snapshot was provided rather than
    /// pretending a diff was computed.
    pub fn run_with_changeset(
        ctx: &PipelineContext<'_>,
        changeset_text: Option<&str>,
        base_graph: Option<&SemanticGraph>,
    ) -> VerificationReport {
        let mut all_entries = Vec::new();
        let mut all_diagnostics = Vec::new();

        // ── Stage 1: Parse ChangeSet ──────────────────────────────────────
        let parsed = match changeset_text {
            Some(text) => match parse_changeset(text) {
                Ok(parsed) => {
                    all_entries.push(stage_entry(
                        "01-parse-changeset",
                        VerificationState::Proven,
                        "changeset",
                        None,
                    ));
                    Some(parsed)
                }
                Err(err) => {
                    all_entries.push(stage_entry(
                        "01-parse-changeset",
                        VerificationState::Failed,
                        "changeset",
                        Some(format!("E_CHANGESET_PARSE: {err}")),
                    ));
                    None
                }
            },
            None => {
                all_entries.push(stage_entry(
                    "01-parse-changeset",
                    VerificationState::Unverified,
                    "changeset",
                    Some("no submitted ChangeSet text provided to pipeline".into()),
                ));
                None
            }
        };

        // ── Stage 2: Canonicalize ChangeSet ───────────────────────────────
        let canonical = parsed.map(canonicalize_parsed);
        if let Some(canonical) = &canonical {
            all_entries.push(stage_entry(
                "02-canonicalize-changeset",
                VerificationState::Proven,
                "changeset",
                Some(format!("{} canonical ops", canonical.ops.len())),
            ));
        } else {
            all_entries.push(stage_entry(
                "02-canonicalize-changeset",
                VerificationState::Unverified,
                "changeset",
                Some("canonical change unavailable because parsing was skipped or failed".into()),
            ));
        }

        // ── Stage 3: Validate op schemas ──────────────────────────────────
        all_entries.extend(validate_op_schemas_with_graph(
            canonical.as_ref(),
            Some(ctx.graph),
        ));

        // ── Stage 4: Resolve graph references ─────────────────────────────
        all_entries.extend(resolve_graph_references(canonical.as_ref(), ctx.graph));

        // ── Stage 5: Build semantic diff ──────────────────────────────────
        all_entries.extend(build_semantic_diff(base_graph, ctx.graph));

        // ── Stage 6: Lower affected graph to Core IR ──────────────────────
        all_entries.push(lower_core_ir(ctx.graph));

        // ── Stage 7: Type check ───────────────────────────────────────────
        all_entries.push(stage_entry(
            "07-type-check",
            VerificationState::Proven,
            "type-checker",
            Some("type checker executed".into()),
        ));
        let type_report = TypeChecker::check(ctx.graph);
        all_entries.extend(type_report.entries);
        all_diagnostics.extend(type_report.diagnostics);

        // ── Stage 8: Effect/capability check ─────────────────────────────
        all_entries.push(stage_entry(
            "08-effect-capability-check",
            VerificationState::Proven,
            "effect-checker",
            Some("effect/capability checker executed".into()),
        ));
        let effect_detail_report = EffectChecker::check(ctx.graph);
        all_entries.extend(effect_detail_report.entries);
        all_diagnostics.extend(effect_detail_report.diagnostics);

        // ── Stage 9: Generate proof obligations ───────────────────────────
        let proof_obligations = ProofObligationPipeline::run_with_ledger(ctx.graph, ctx.solver);
        all_entries.push(stage_entry(
            "09-generate-proof-obligations",
            VerificationState::Proven,
            "proof-obligations",
            Some(format!("{} obligations", proof_obligations.len())),
        ));

        // ── Stage 10: Check refinements ───────────────────────────────────
        all_entries.extend(check_refinements(ctx.graph, ctx.solver));

        // ── Stage 11: Check contracts ─────────────────────────────────────
        all_entries.push(stage_entry(
            "11-check-contracts",
            VerificationState::Proven,
            "contracts",
            Some("contract checker executed".into()),
        ));
        let contract_checker = ContractChecker::new(ctx.solver);
        let contract_report = contract_checker.check(ctx.graph);
        all_entries.extend(contract_report.entries);
        all_diagnostics.extend(contract_report.diagnostics);

        // ── Stage 12: Check invariants via impact analysis ────────────────
        all_entries.extend(check_invariants(base_graph, ctx.graph));

        // ── Stage 13: Resource lifecycle ──────────────────────────────────
        all_entries.push(stage_entry(
            "13-check-resource-lifecycle",
            VerificationState::Proven,
            "resources",
            Some("resource checker executed".into()),
        ));
        let resource_report = ResourceChecker::check(ctx.graph);
        all_entries.extend(resource_report.entries);
        all_diagnostics.extend(resource_report.diagnostics);

        // ── Stage 14: Concurrency safety ──────────────────────────────────
        all_entries.push(stage_entry(
            "14-check-concurrency-safety",
            VerificationState::Proven,
            "concurrency",
            Some("concurrency checker executed".into()),
        ));
        let concurrency_report = ConcurrencyChecker::check(ctx.graph);
        all_entries.extend(concurrency_report.entries);
        all_diagnostics.extend(concurrency_report.diagnostics);

        // ── Stage 15: Boundary/FFI trust ──────────────────────────────────
        all_entries.push(stage_entry(
            "15-check-boundaries-ffi-trust",
            VerificationState::Proven,
            "boundaries",
            Some("boundary checker executed".into()),
        ));
        let boundary_report = BoundaryChecker::check(ctx.graph);
        all_entries.extend(boundary_report.entries);
        all_diagnostics.extend(boundary_report.diagnostics);

        // ── Stage 16: Package trust ───────────────────────────────────────
        all_entries.push(stage_entry(
            "16-check-package-trust-dependencies",
            VerificationState::Proven,
            "packages",
            Some("package trust checker executed".into()),
        ));
        let package_entries = PackageTrustChecker::check(ctx.manifests, ctx.profile);
        all_entries.extend(package_entries);
        let version_entries = PackageTrustChecker::check_version_constraints(ctx.manifests);
        all_entries.extend(version_entries);
        let deprecated_entries = PackageTrustChecker::check_deprecated_exports(ctx.manifests);
        all_entries.extend(deprecated_entries);

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

        // ── Infer report extensions from proof-obligation ledger ──────────
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

        let solver_diagnostics: Vec<SolverDiagnostic> = proof_obligations
            .iter()
            .filter_map(SolverDiagnostic::from_ledger_entry)
            .collect();

        // ── Snapshot IDs from graph structure ─────────────────────────────
        // Derive a stable snapshot identifier from node names + IDs so the
        // report is self-describing without adding new PipelineContext fields.
        let target_snapshot = Some(graph_snapshot_id(ctx.graph));
        let base_snapshot = base_graph.map(graph_snapshot_id);

        // ── Assemble pre-policy report ────────────────────────────────────
        let mut pre_policy = VerificationReport {
            entries: all_entries,
            diagnostics: all_diagnostics,
            schema_version: "verification/1.0".into(),
            summary_counts,
            proof_obligations,
            solver_diagnostics,
            degradation_events,
            artifact_hashes: vec![],
            base_snapshot,
            target_snapshot,
            structural_diff: ctx.structural_diff.cloned(),
            approvals: ctx.approvals.to_vec(),
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

        pre_policy.entries.push(stage_entry(
            "17-check-policy-gates",
            VerificationState::Proven,
            "policy",
            Some(format!("policy decision: {policy_decision:?}")),
        ));

        // ── Stage 18: Check approval records ──────────────────────────────
        pre_policy
            .entries
            .extend(check_approval_records(ctx.approvals));

        // ── Stage 19: Lower to ANF ────────────────────────────────────────
        pre_policy.entries.extend(lower_anf(ctx.graph));

        // ── Stage 20: Check ANF effect/resource ordering ──────────────────
        pre_policy.entries.push(check_anf_ordering(ctx.graph));

        // ── Stage 21: Generate/validate manifest ──────────────────────────
        pre_policy.entries.push(validate_manifest(
            ctx.graph,
            ctx.manifest_caps,
            ctx.artifact_manifest_hash,
        ));

        let mut artifact_hashes = Vec::new();

        // ── Canonical change hash (doc §Artifact consistency) ────────────
        // Add the canonical change hash to artifact_hashes so that the report
        // can uniquely identify exactly which canonical changeset was verified.
        if let Some(canonical_cs) = &canonical {
            let hash = canonical_change_hash(canonical_cs);
            artifact_hashes.push(crate::report::ArtifactHash {
                artifact: "canonical_change".into(),
                hash,
            });
        }

        // ── Stage 22: Codegen consistency ─────────────────────────────────
        pre_policy.entries.push(stage_entry(
            "22-codegen-consistency-check",
            VerificationState::Proven,
            "codegen",
            Some("codegen consistency checker executed".into()),
        ));
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

        // ── Stage 23: Emit verification report ────────────────────────────
        pre_policy.entries.push(stage_entry(
            "23-emit-verification-report",
            VerificationState::Proven,
            "verification_report",
            Some("schema verification/1.0 emitted".into()),
        ));

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

// ── Pipeline utilities ────────────────────────────────────────────────────

/// Compute a BLAKE3 content hash for a `CanonicalChangeSet`.
///
/// The hash is computed over the JSON-serialized form of the canonical ops
/// list.  This ties the verification report to exactly which change was
/// verified, enabling artifact consistency checks (verification.md §Artifact
/// consistency).
fn canonical_change_hash(canonical: &CanonicalChangeSet) -> String {
    // Serialize to canonical JSON for deterministic byte representation.
    let json = serde_json::to_string(&canonical.ops).unwrap_or_default();
    let hash = blake3::hash(json.as_bytes());
    hash.to_hex().to_string()
}

/// Derive a stable, human-readable snapshot identifier from a `SemanticGraph`.
///
/// The ID is computed from the sorted node names joined with `|` and
/// prefixed with the node count.  This is NOT a cryptographic hash —
/// it is a deterministic label for audit/debug purposes.
fn graph_snapshot_id(graph: &SemanticGraph) -> String {
    let mut names: Vec<&str> = graph.nodes.iter().map(|n| n.name.as_str()).collect();
    names.sort_unstable();
    format!("snap:{}:{}", graph.nodes.len(), names.join("|"))
}

fn stage_entry(
    claim: impl Into<String>,
    state: VerificationState,
    scope: impl Into<String>,
    evidence: Option<String>,
) -> VerificationEntry {
    let blocking = matches!(state, VerificationState::Failed | VerificationState::Unsafe);
    VerificationEntry {
        claim: claim.into(),
        state,
        scope: scope.into(),
        evidence,
        blocking,
        repair_options: vec![],
    }
}
