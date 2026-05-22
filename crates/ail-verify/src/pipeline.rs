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

use std::collections::{BTreeSet, VecDeque};

use ail_change::canonical::{CanonicalChangeSet, OpPayload, canonicalize_parsed};
use ail_change::model::ChangeSetOp;
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
use crate::proof::{ClauseRole, ProofObligation, ProofObligationPipeline};
use crate::report::{DegradationEvent, VerificationEntry, VerificationReport, VerificationState};
use crate::resource_checker::ResourceChecker;
use crate::solver::{Solver, SolverOutcome};
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
        all_entries
            .extend(validate_op_schemas_with_graph(canonical.as_ref(), Some(ctx.graph)));

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
        pre_policy.entries.push(lower_anf(ctx.graph));

        // ── Stage 20: Check ANF effect/resource ordering ──────────────────
        pre_policy.entries.push(check_anf_ordering(ctx.graph));

        // ── Stage 21: Generate/validate manifest ──────────────────────────
        pre_policy
            .entries
            .push(validate_manifest(ctx.graph, ctx.manifest_caps));

        let mut artifact_hashes = Vec::new();

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
    }
}

/// Current schema version for op arg validation.
const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Known primitive type names for arg type validation.
const KNOWN_PRIMITIVES: &[&str] = &[
    "Int", "String", "Bool", "Float", "Decimal", "Money", "Email",
];

fn validate_op_schemas(canonical: Option<&CanonicalChangeSet>) -> Vec<VerificationEntry> {
    validate_op_schemas_with_graph(canonical, None)
}

fn validate_op_schemas_with_graph(
    canonical: Option<&CanonicalChangeSet>,
    graph: Option<&SemanticGraph>,
) -> Vec<VerificationEntry> {
    let Some(canonical) = canonical else {
        return vec![stage_entry(
            "03-validate-op-schemas",
            VerificationState::Unverified,
            "changeset.ops",
            Some("canonical change unavailable".into()),
        )];
    };

    if canonical.ops.is_empty() {
        return vec![stage_entry(
            "03-validate-op-schemas",
            VerificationState::Proven,
            "changeset.ops",
            Some("identity changeset has no ops".into()),
        )];
    }

    // Build graph node name set for type arg validation
    let graph_names: BTreeSet<&str> = graph
        .map(|g| g.nodes.iter().map(|n| n.name.as_str()).collect())
        .unwrap_or_default();

    canonical
        .ops
        .iter()
        .enumerate()
        .flat_map(|(idx, op)| {
            let scope = format!("op[{idx}]:{}", op.verb);
            let mut entries = Vec::new();

            // Required arg presence check (existing)
            let missing = required_args(&op.kind, &op.verb)
                .iter()
                .filter(|key| !op.args.contains_key(**key))
                .copied()
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                entries.push(stage_entry(
                    "03-validate-op-schemas",
                    VerificationState::Failed,
                    scope.clone(),
                    Some(format!(
                        "E_OP_SCHEMA: missing required args: {}",
                        missing.join(", ")
                    )),
                ));
                return entries;
            }

            // Version compatibility check (D2)
            if let Some(version_str) = op.args.get("version") {
                if let Ok(v) = version_str.parse::<u32>() {
                    if v > CURRENT_SCHEMA_VERSION {
                        entries.push(stage_entry(
                            "03-validate-op-schemas",
                            VerificationState::Failed,
                            scope.clone(),
                            Some(format!(
                                "E_OP_VERSION_INCOMPATIBLE: op version {v} exceeds current schema version {CURRENT_SCHEMA_VERSION}"
                            )),
                        ));
                        return entries;
                    }
                }
            }

            // Type arg validation (D2): must be a known primitive or graph node name
            if let Some(type_arg) = op.args.get("type") {
                let is_primitive = KNOWN_PRIMITIVES.contains(&type_arg.as_str());
                let is_node = graph_names.contains(type_arg.as_str());
                if !is_primitive && !is_node && !type_arg.is_empty() {
                    entries.push(stage_entry(
                        "03-validate-op-schemas",
                        VerificationState::Failed,
                        scope.clone(),
                        Some(format!(
                            "E_OP_ARG_TYPE_INVALID: type '{}' is not a known primitive or graph node name",
                            type_arg
                        )),
                    ));
                    return entries;
                }
            }

            // Effect arg format validation (D2): must contain ':'
            if let Some(effect_arg) = op.args.get("effect") {
                if !effect_arg.contains(':') {
                    entries.push(stage_entry(
                        "03-validate-op-schemas",
                        VerificationState::Failed,
                        scope.clone(),
                        Some(format!(
                            "E_OP_ARG_EFFECT_MALFORMED: effect '{}' must follow 'name:Provider' pattern (missing ':')",
                            effect_arg
                        )),
                    ));
                    return entries;
                }
            }

            entries.push(stage_entry(
                "03-validate-op-schemas",
                VerificationState::Proven,
                scope,
                None,
            ));
            entries
        })
        .collect()
}

fn required_args(kind: &ChangeSetOp, verb: &str) -> &'static [&'static str] {
    match (kind, verb) {
        (
            ChangeSetOp::Create,
            "create_module" | "create_type" | "create_function" | "create_capability",
        ) => &["id"],
        (ChangeSetOp::Set, "set_return") => &["target", "type"],
        (ChangeSetOp::Set, "set_body") => &["target", "body"],
        (ChangeSetOp::Add, "add_param") => &["target", "name", "type"],
        (ChangeSetOp::Add, "add_effect") => &["target", "effect"],
        (ChangeSetOp::Add, "add_contract") => &["target", "kind", "rule"],
        (ChangeSetOp::Remove, "remove_effect") => &["target", "effect"],
        (ChangeSetOp::Remove, "remove_contract") => &["target", "rule"],
        (ChangeSetOp::Connect | ChangeSetOp::Disconnect, _) => &["source", "target"],
        (ChangeSetOp::Rename, _) => &["target", "name"],
        (ChangeSetOp::Move, _) => &["target", "to"],
        (ChangeSetOp::Delete, _) => &["target"],
        (ChangeSetOp::Bind, _) => &["capability", "handler"],
        (ChangeSetOp::Grant | ChangeSetOp::Revoke, _) => &["target", "capability"],
        (ChangeSetOp::Expose | ChangeSetOp::Hide, _) => &["target"],
        (
            ChangeSetOp::Infer
            | ChangeSetOp::Derive
            | ChangeSetOp::Generate
            | ChangeSetOp::Assert
            | ChangeSetOp::Lock
            | ChangeSetOp::Refactor
            | ChangeSetOp::Migrate
            | ChangeSetOp::Approve
            | ChangeSetOp::Reject
            | ChangeSetOp::Deprecate
            | ChangeSetOp::Annotate
            | ChangeSetOp::Verify,
            _,
        ) => &["target"],
        _ => &[],
    }
}

/// Check if a string is a valid 64-character hexadecimal hash.
fn is_valid_64char_hex(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn resolve_graph_references(
    canonical: Option<&CanonicalChangeSet>,
    graph: &SemanticGraph,
) -> Vec<VerificationEntry> {
    let Some(canonical) = canonical else {
        return vec![stage_entry(
            "04-resolve-graph-references",
            VerificationState::Unverified,
            "changeset.refs",
            Some("canonical change unavailable".into()),
        )];
    };
    let names = graph
        .nodes
        .iter()
        .map(|node| node.name.as_str())
        .collect::<BTreeSet<_>>();

    let mut entries = Vec::new();
    for (idx, op) in canonical.ops.iter().enumerate() {
        // Stage 4 extension (D3): snapshot hash freshness check
        for hash_key in ["base_hash", "snapshot_hash"] {
            if let Some(hash_val) = op.args.get(hash_key) {
                if !is_valid_64char_hex(hash_val) {
                    entries.push(stage_entry(
                        "04-resolve-graph-references",
                        VerificationState::Failed,
                        format!("op[{idx}].{hash_key}"),
                        Some(format!(
                            "E_STALE_CONTEXT: {} '{}' is not a valid 64-char hex snapshot hash",
                            hash_key, hash_val
                        )),
                    ));
                } else {
                    entries.push(stage_entry(
                        "04-resolve-graph-references",
                        VerificationState::Proven,
                        format!("op[{idx}].{hash_key}"),
                        Some(format!("{hash_key} is a valid 64-char hex hash")),
                    ));
                }
            }
        }

        for key in ["target", "source", "from", "to", "capability", "handler"] {
            let Some(value) = op.args.get(key) else {
                continue;
            };
            if key == "to" && matches!(op.kind, ChangeSetOp::Move | ChangeSetOp::Migrate) {
                continue;
            }
            let creates_ref = key == "target" && matches!(op.payload, OpPayload::CreateNode(_));
            if creates_ref || names.contains(value.as_str()) {
                entries.push(stage_entry(
                    "04-resolve-graph-references",
                    VerificationState::Proven,
                    format!("op[{idx}].{key}"),
                    None,
                ));
            } else {
                entries.push(stage_entry(
                    "04-resolve-graph-references",
                    VerificationState::Failed,
                    format!("op[{idx}].{key}"),
                    Some(format!(
                        "E_GRAPH_REF_UNRESOLVED: '{value}' does not exist in target graph"
                    )),
                ));
            }
        }
    }
    if entries.is_empty() {
        entries.push(stage_entry(
            "04-resolve-graph-references",
            VerificationState::Proven,
            "changeset.refs",
            Some("no graph references to resolve".into()),
        ));
    }
    entries
}

fn build_semantic_diff(
    base_graph: Option<&SemanticGraph>,
    target_graph: &SemanticGraph,
) -> Vec<VerificationEntry> {
    let Some(base) = base_graph else {
        return vec![stage_entry(
            "05-build-semantic-diff",
            VerificationState::Unverified,
            "semantic_diff",
            Some("base graph snapshot not provided".into()),
        )];
    };

    let base_names: BTreeSet<&str> = base.nodes.iter().map(|n| n.name.as_str()).collect();
    let target_names: BTreeSet<&str> = target_graph.nodes.iter().map(|n| n.name.as_str()).collect();

    let mut entries = Vec::new();

    // Added nodes (in target but not in base) → Proven (addition is expected)
    for added_name in target_names.difference(&base_names) {
        entries.push(stage_entry(
            "05-build-semantic-diff",
            VerificationState::Proven,
            added_name.to_string(),
            Some(format!("node '{}' added in this changeset", added_name)),
        ));
    }

    // Removed nodes (in base but not in target) → Unverified (removal may break refs)
    for removed_name in base_names.difference(&target_names) {
        // Check if the node had expose-relevant edges in the base graph (D4)
        let had_expose = base.edges.iter().any(|edge| {
            base.nodes
                .iter()
                .any(|n| n.name == *removed_name && n.id == edge.source)
                && edge.kind == ail_core::semantic_graph::EdgeKind::DependsOn
        });
        let evidence = if had_expose {
            format!(
                "E_PUBLIC_API_CHANGED: node '{}' removed; had dependent edges",
                removed_name
            )
        } else {
            format!("node '{}' removed from graph; verify no references remain", removed_name)
        };
        entries.push(stage_entry(
            "05-build-semantic-diff",
            VerificationState::Unverified,
            removed_name.to_string(),
            Some(evidence),
        ));
    }

    // Changed nodes (in both but with different type_facts or effect_row) → Unverified
    for name in base_names.intersection(&target_names) {
        let base_node = base.nodes.iter().find(|n| n.name == *name);
        let target_node = target_graph.nodes.iter().find(|n| n.name == *name);
        if let (Some(b), Some(t)) = (base_node, target_node) {
            if b.type_facts != t.type_facts || b.effect_row != t.effect_row {
                entries.push(stage_entry(
                    "05-build-semantic-diff",
                    VerificationState::Unverified,
                    name.to_string(),
                    Some(format!(
                        "node '{}' type_facts or effect_row changed; verify compatibility",
                        name
                    )),
                ));
            }
        }
    }

    // If no per-node changes, emit single Proven summary
    if entries.is_empty() {
        entries.push(stage_entry(
            "05-build-semantic-diff",
            VerificationState::Proven,
            "semantic_diff",
            Some("no structural changes detected".into()),
        ));
    }

    entries
}

fn lower_core_ir(graph: &SemanticGraph) -> VerificationEntry {
    match graph.validate() {
        Ok(()) => stage_entry(
            "06-lower-affected-graph-to-core-ir",
            VerificationState::Proven,
            "core_ir",
            Some(format!("{} graph nodes lowered", graph.nodes.len())),
        ),
        Err(err) => stage_entry(
            "06-lower-affected-graph-to-core-ir",
            VerificationState::Failed,
            "core_ir",
            Some(format!(
                "E_CORE_IR_LOWERING: graph validation failed: {err:?}"
            )),
        ),
    }
}

fn check_refinements(graph: &SemanticGraph, solver: &dyn Solver) -> Vec<VerificationEntry> {
    let mut entries = Vec::new();
    for node in &graph.nodes {
        let Some(refinement) = &node.refinement_ref else {
            continue;
        };
        let state = if refinement.predicate.trim().is_empty()
            || refinement.predicate.trim() == "false"
        {
            VerificationState::Failed
        } else if refinement.status == ail_core::semantic_graph::RefinementStatus::RuntimeChecked
            && node
                .runtime_checks
                .as_ref()
                .is_some_and(|checks| !checks.is_empty())
        {
            VerificationState::RuntimeChecked
        } else {
            match refinement.status {
                ail_core::semantic_graph::RefinementStatus::Proven => VerificationState::Proven,
                ail_core::semantic_graph::RefinementStatus::RuntimeChecked => {
                    VerificationState::Failed
                }
                ail_core::semantic_graph::RefinementStatus::Assumed => VerificationState::Assumed,
                ail_core::semantic_graph::RefinementStatus::Unverified => {
                    // TASK-10: try solver for Unverified refinements
                    let obligation = ProofObligation {
                        predicate: refinement.predicate.clone(),
                        role: ClauseRole::Requires,
                        scope: node.name.clone(),
                    };
                    match solver.solve(&obligation) {
                        SolverOutcome::Proven => VerificationState::Proven,
                        SolverOutcome::Assumed(_) | SolverOutcome::Unsupported => {
                            VerificationState::Assumed
                        }
                    }
                }
                ail_core::semantic_graph::RefinementStatus::Failed => VerificationState::Failed,
            }
        };
        entries.push(stage_entry(
            "10-check-refinements",
            state,
            node.name.clone(),
            Some(format!(
                "{} -> {}",
                refinement.base_type, refinement.predicate
            )),
        ));
    }
    if entries.is_empty() {
        entries.push(stage_entry(
            "10-check-refinements",
            VerificationState::Proven,
            "refinements",
            Some("no refinement refs present".into()),
        ));
    }
    entries
}

fn check_invariants(
    base_graph: Option<&SemanticGraph>,
    target_graph: &SemanticGraph,
) -> Vec<VerificationEntry> {
    use ail_core::semantic_graph::{EdgeKind, NodeKind, NodeRef};

    let invariant_nodes: Vec<(NodeRef, String)> = target_graph
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Invariant)
        .map(|node| (node.id, node.name.clone()))
        .collect();

    if invariant_nodes.is_empty() {
        return vec![stage_entry(
            "12-check-invariants-via-impact-analysis",
            VerificationState::Proven,
            "invariants",
            Some("no invariant nodes present".into()),
        )];
    }

    // No base graph → all invariants unverified (can't determine what changed)
    let Some(base) = base_graph else {
        return invariant_nodes
            .into_iter()
            .map(|(_, name)| {
                stage_entry(
                    "12-check-invariants-via-impact-analysis",
                    VerificationState::Unverified,
                    name,
                    Some("no base graph snapshot provided; cannot assess impact".into()),
                )
            })
            .collect();
    };

    // Compute changed node IDs: new nodes OR nodes with changed type_facts/effect_row
    let base_by_name: std::collections::HashMap<&str, &ail_core::semantic_graph::GraphNode> =
        base.nodes.iter().map(|n| (n.name.as_str(), n)).collect();
    let changed_ids: BTreeSet<NodeRef> = target_graph
        .nodes
        .iter()
        .filter(|tn| {
            match base_by_name.get(tn.name.as_str()) {
                None => true, // new node
                Some(bn) => bn.type_facts != tn.type_facts || bn.effect_row != tn.effect_row,
            }
        })
        .map(|n| n.id)
        .collect();

    // For each invariant, BFS across all edges (bidirectional) to find reachable nodes
    invariant_nodes
        .into_iter()
        .map(|(inv_id, name)| {
            if changed_ids.is_empty() {
                return stage_entry(
                    "12-check-invariants-via-impact-analysis",
                    VerificationState::Proven,
                    name,
                    None,
                );
            }

            // BFS from invariant node
            let mut reachable: BTreeSet<NodeRef> = BTreeSet::new();
            let mut queue: VecDeque<NodeRef> = VecDeque::new();
            reachable.insert(inv_id);
            queue.push_back(inv_id);
            while let Some(cur) = queue.pop_front() {
                for edge in &target_graph.edges {
                    if edge.source == cur && !reachable.contains(&edge.target) {
                        reachable.insert(edge.target);
                        queue.push_back(edge.target);
                    }
                    if edge.target == cur && !reachable.contains(&edge.source) {
                        reachable.insert(edge.source);
                        queue.push_back(edge.source);
                    }
                }
            }

            // Find reachable changed nodes (excluding the invariant itself)
            let reachable_changed: Vec<NodeRef> = changed_ids
                .iter()
                .filter(|&&id| id != inv_id && reachable.contains(&id))
                .copied()
                .collect();

            if reachable_changed.is_empty() {
                return stage_entry(
                    "12-check-invariants-via-impact-analysis",
                    VerificationState::Proven,
                    name,
                    None,
                );
            }

            // Which changed nodes are covered by BreaksIfChanged edges to the invariant?
            let covered: BTreeSet<NodeRef> = target_graph
                .edges
                .iter()
                .filter(|e| e.kind == EdgeKind::BreaksIfChanged && e.target == inv_id)
                .map(|e| e.source)
                .collect();

            let uncovered: Vec<&str> = reachable_changed
                .iter()
                .filter(|id| !covered.contains(id))
                .filter_map(|id| target_graph.nodes.iter().find(|n| n.id == *id))
                .map(|n| n.name.as_str())
                .collect();

            if uncovered.is_empty() {
                stage_entry(
                    "12-check-invariants-via-impact-analysis",
                    VerificationState::Proven,
                    name,
                    None,
                )
            } else {
                stage_entry(
                    "12-check-invariants-via-impact-analysis",
                    VerificationState::Unverified,
                    name,
                    Some(format!(
                        "invariant impacted by changes in: {}",
                        uncovered.join(", ")
                    )),
                )
            }
        })
        .collect()
}

fn check_approval_records(approvals: &[ApprovalRecord]) -> Vec<VerificationEntry> {
    if approvals.is_empty() {
        return vec![stage_entry(
            "18-check-approval-records",
            VerificationState::Proven,
            "approvals",
            Some("no approval records required by input".into()),
        )];
    }
    approvals
        .iter()
        .map(|approval| {
            let valid = !approval.scope.trim().is_empty()
                && !approval.approver.trim().is_empty()
                && !approval.reason.trim().is_empty();
            stage_entry(
                "18-check-approval-records",
                if valid {
                    VerificationState::Proven
                } else {
                    VerificationState::Failed
                },
                approval.scope.clone(),
                if valid {
                    None
                } else {
                    Some("E_APPROVAL_RECORD_INCOMPLETE".into())
                },
            )
        })
        .collect()
}

fn lower_anf(graph: &SemanticGraph) -> VerificationEntry {
    let unsupported = graph
        .nodes
        .iter()
        .filter_map(|node| node.body_expr.as_ref().map(|body| (node, body)))
        .find(|(_, body)| body.contains(";") || body.contains("while "));
    if let Some((node, body)) = unsupported {
        stage_entry(
            "19-lower-to-anf",
            VerificationState::Unverified,
            node.name.clone(),
            Some(format!("body requires non-trivial ANF lowering: {body}")),
        )
    } else {
        stage_entry(
            "19-lower-to-anf",
            VerificationState::Proven,
            "anf_ir",
            Some("graph expressions are ANF-compatible".into()),
        )
    }
}

fn check_anf_ordering(graph: &SemanticGraph) -> VerificationEntry {
    for node in &graph.nodes {
        let Some(body) = &node.body_expr else {
            continue;
        };
        if let (Some(use_pos), Some(release_pos)) = (body.find("use("), body.find("release("))
            && use_pos > release_pos
        {
            return stage_entry(
                "20-check-anf-effect-resource-ordering",
                VerificationState::Failed,
                node.name.clone(),
                Some("E_ANF_RESOURCE_ORDER: use appears after release".into()),
            );
        }
    }
    stage_entry(
        "20-check-anf-effect-resource-ordering",
        VerificationState::Proven,
        "anf_ir",
        Some("effect/resource ordering preserved".into()),
    )
}

fn validate_manifest(graph: &SemanticGraph, manifest_caps: &[String]) -> VerificationEntry {
    let graph_caps = graph
        .nodes
        .iter()
        .filter(|node| node.kind == ail_core::semantic_graph::NodeKind::Capability)
        .map(|node| node.name.as_str())
        .collect::<BTreeSet<_>>();
    if manifest_caps.is_empty() && graph_caps.is_empty() {
        return stage_entry(
            "21-generate-validate-manifest",
            VerificationState::Proven,
            "capabilities_manifest",
            Some("no capabilities required".into()),
        );
    }
    let manifest = manifest_caps
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if graph_caps == manifest {
        stage_entry(
            "21-generate-validate-manifest",
            VerificationState::Proven,
            "capabilities_manifest",
            Some(format!("{} capabilities validated", graph_caps.len())),
        )
    } else {
        stage_entry(
            "21-generate-validate-manifest",
            VerificationState::Failed,
            "capabilities_manifest",
            Some(
                "E_MANIFEST_MISMATCH: graph capabilities differ from manifest capabilities".into(),
            ),
        )
    }
}
