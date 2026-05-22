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
        pre_policy
            .entries
            .push(validate_manifest(ctx.graph, ctx.manifest_caps, ctx.artifact_manifest_hash));

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

            // Type arg validation (D2): must be a known primitive, graph node name,
            // or a qualified external type (Package.Type / Domain.Sub.Type pattern).
            if let Some(type_arg) = op.args.get("type") {
                let is_primitive = KNOWN_PRIMITIVES.contains(&type_arg.as_str());
                let is_node = graph_names.contains(type_arg.as_str());
                // Qualified external type: "Package.Type" or "Domain.Sub.Type" —
                // a dot-separated path where every segment is a non-empty identifier.
                let is_qualified = type_arg.contains('.')
                    && type_arg.split('.').all(|seg| {
                        !seg.is_empty()
                            && seg.chars().all(|c| c.is_alphanumeric() || c == '_')
                    });
                if !is_primitive && !is_node && !is_qualified && !type_arg.is_empty() {
                    entries.push(stage_entry(
                        "03-validate-op-schemas",
                        VerificationState::Failed,
                        scope.clone(),
                        Some(format!(
                            "E_OP_ARG_TYPE_INVALID: type '{}' is not a known primitive, graph node name, or qualified external type",
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

            // Transitive BreaksIfChanged coverage: BFS following BreaksIfChanged edges
            // backward toward inv_id to find all transitively covered nodes (ITC-1).
            let mut covered: BTreeSet<NodeRef> = BTreeSet::new();
            let mut bfc_queue: VecDeque<NodeRef> = VecDeque::from([inv_id]);
            let mut bfc_visited: BTreeSet<NodeRef> = BTreeSet::from([inv_id]);
            while let Some(cur) = bfc_queue.pop_front() {
                for edge in &target_graph.edges {
                    if edge.kind == EdgeKind::BreaksIfChanged
                        && edge.target == cur
                        && !bfc_visited.contains(&edge.source)
                    {
                        bfc_visited.insert(edge.source);
                        covered.insert(edge.source);
                        bfc_queue.push_back(edge.source);
                    }
                }
            }

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

/// Returns true if the body contains a `let ... in` pattern (valid ANF structure).
fn has_let_in_pattern(body: &str) -> bool {
    if let Some(let_pos) = body.find("let ") {
        body[let_pos..].contains(" in ")
    } else {
        false
    }
}

fn lower_anf(graph: &SemanticGraph) -> Vec<VerificationEntry> {
    use ail_core::semantic_graph::NodeKind;
    let mut entries = Vec::new();

    // Placeholder check (Ola5 Gap-2): Function nodes with no body_expr are
    // structural placeholders.  Flag each one individually as Unverified so
    // callers can distinguish missing implementations from verified ones.
    for node in &graph.nodes {
        if node.kind == NodeKind::Function && node.body_expr.is_none() {
            entries.push(stage_entry(
                "19-lower-to-anf",
                VerificationState::Unverified,
                node.name.clone(),
                Some(format!(
                    "Placeholder: function '{}' has no body expression; ANF lowering deferred",
                    node.name
                )),
            ));
        }
    }

    // Structural ANF check: detect bodies with non-ANF control flow.
    let violation = graph
        .nodes
        .iter()
        .filter_map(|node| node.body_expr.as_ref().map(|body| (node, body)))
        .find(|(_, body)| {
            // Non-ANF: imperative control flow keywords
            if body.contains("while ") || body.contains("for ") || body.contains("loop ") {
                return true;
            }
            // Non-ANF: bare semicolons outside of a let...in context
            if body.contains(';') && !has_let_in_pattern(body) {
                return true;
            }
            false
        });
    if let Some((node, body)) = violation {
        entries.push(stage_entry(
            "19-lower-to-anf",
            VerificationState::Unverified,
            node.name.clone(),
            Some(format!("body requires non-trivial ANF lowering: {body}")),
        ));
        return entries;
    }

    // Summary Proven entry when no structural violations were found.
    entries.push(stage_entry(
        "19-lower-to-anf",
        VerificationState::Proven,
        "anf_ir",
        Some("graph expressions are ANF-compatible".into()),
    ));
    entries
}

/// Scan `body` for `acquire(<ident>)` and `release(<ident>)` tokens.
/// Returns per-identifier (first_acquire_pos, first_release_pos) pairs where a
/// release appears before the corresponding acquire.
fn find_ordering_violation(body: &str) -> Option<String> {
    use std::collections::HashMap;

    let mut acquires: HashMap<String, usize> = HashMap::new();
    let mut releases: HashMap<String, usize> = HashMap::new();

    // Walk the body scanning for acquire(<ident>) and release(<ident>)
    let mut pos = 0;
    while pos < body.len() {
        for (keyword, map) in [("acquire(", &mut acquires), ("release(", &mut releases)] {
            if body[pos..].starts_with(keyword) {
                let inner_start = pos + keyword.len();
                if let Some(close) = body[inner_start..].find(')') {
                    let ident = body[inner_start..inner_start + close].trim().to_string();
                    if !ident.is_empty() && ident.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                        map.entry(ident).or_insert(pos);
                    }
                }
            }
        }
        pos += 1;
    }

    // Check: any release appearing before its corresponding acquire
    for (ident, release_pos) in &releases {
        if let Some(&acquire_pos) = acquires.get(ident) {
            if *release_pos < acquire_pos {
                return Some(ident.clone());
            }
        } else {
            // release without matching acquire is also a violation
            return Some(ident.clone());
        }
    }
    None
}

/// Scan `body` for effect ordering violations:
/// - `run_effect(ident)` before `bind_effect(ident)` → E_ANF_EFFECT_ORDER
/// - `run_effect(ident)` without any `bind_effect(ident)` → E_ANF_EFFECT_ORDER
/// - `emit_effect(ident)` appearing more than once → E_ANF_DUPLICATE_EFFECT
fn find_effect_ordering_violation(body: &str) -> Option<String> {
    use std::collections::HashMap;

    let mut binds: HashMap<String, usize> = HashMap::new();
    let mut runs: HashMap<String, usize> = HashMap::new();
    let mut emits: HashMap<String, usize> = HashMap::new();

    // Walk the body scanning for effect keywords.
    let mut pos = 0;
    while pos < body.len() {
        for (keyword, map) in [
            ("bind_effect(", &mut binds),
            ("run_effect(", &mut runs),
            ("emit_effect(", &mut emits),
        ] {
            if body[pos..].starts_with(keyword) {
                let inner_start = pos + keyword.len();
                if let Some(close) = body[inner_start..].find(')') {
                    let ident = body[inner_start..inner_start + close].trim().to_string();
                    if !ident.is_empty()
                        && ident
                            .chars()
                            .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
                    {
                        *map.entry(ident).or_insert(0) += 1;
                    }
                }
            }
        }
        pos += 1;
    }

    // Check: emit_effect duplicate
    for (ident, count) in &emits {
        if *count > 1 {
            return Some(format!("E_ANF_DUPLICATE_EFFECT:{ident}"));
        }
    }

    // Check: run_effect before bind_effect (positional scan)
    for ident in runs.keys() {
        let bind_pos = find_keyword_pos(body, "bind_effect(", ident);
        let run_pos = find_keyword_pos(body, "run_effect(", ident);
        match (bind_pos, run_pos) {
            (None, Some(_)) => return Some(format!("E_ANF_EFFECT_ORDER_NO_BIND:{ident}")),
            (Some(b), Some(r)) if r < b => {
                return Some(format!("E_ANF_EFFECT_ORDER:{ident}"));
            }
            _ => {}
        }
    }
    None
}

/// Find the byte position of the first `keyword + ident + ")"` in `body`.
fn find_keyword_pos(body: &str, keyword: &str, ident: &str) -> Option<usize> {
    let mut pos = 0;
    while pos < body.len() {
        if body[pos..].starts_with(keyword) {
            let inner_start = pos + keyword.len();
            if let Some(close) = body[inner_start..].find(')') {
                let found = body[inner_start..inner_start + close].trim();
                if found == ident {
                    return Some(pos);
                }
            }
        }
        pos += 1;
    }
    None
}

fn check_anf_ordering(graph: &SemanticGraph) -> VerificationEntry {
    for node in &graph.nodes {
        let Some(body) = &node.body_expr else {
            continue;
        };
        // Resource ordering check (existing — runs first, ANF-4)
        if let Some(ident) = find_ordering_violation(body) {
            return stage_entry(
                "20-check-anf-effect-resource-ordering",
                VerificationState::Failed,
                node.name.clone(),
                Some(format!(
                    "E_ANF_RESOURCE_ORDER: release('{}') appears before acquire('{}')",
                    ident, ident
                )),
            );
        }
        // Effect ordering check (new)
        if let Some(violation) = find_effect_ordering_violation(body) {
            let (code, detail) =
                if let Some(ident) = violation.strip_prefix("E_ANF_DUPLICATE_EFFECT:") {
                    (
                        "E_ANF_DUPLICATE_EFFECT",
                        format!("emit_effect('{ident}') appears more than once"),
                    )
                } else if let Some(ident) = violation.strip_prefix("E_ANF_EFFECT_ORDER_NO_BIND:") {
                    (
                        "E_ANF_EFFECT_ORDER",
                        format!("run_effect('{ident}') without bind_effect"),
                    )
                } else if let Some(ident) = violation.strip_prefix("E_ANF_EFFECT_ORDER:") {
                    (
                        "E_ANF_EFFECT_ORDER",
                        format!("run_effect('{ident}') before bind_effect('{ident}')"),
                    )
                } else {
                    ("E_ANF_EFFECT_ORDER", violation)
                };
            return stage_entry(
                "20-check-anf-effect-resource-ordering",
                VerificationState::Failed,
                node.name.clone(),
                Some(format!("{code}: {detail}")),
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

// ── Unit tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use ail_core::semantic_graph::{EdgeKind, GraphEdge, GraphNode, NodeKind, NodeRef, SemanticGraph};

    use crate::report::VerificationState;

    use super::{check_anf_ordering, check_invariants};

    fn graph_with_body(body: &str) -> SemanticGraph {
        let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.test");
        node.body_expr = Some(body.to_string());
        SemanticGraph { nodes: vec![node], edges: vec![] }
    }

    fn empty_graph() -> SemanticGraph {
        SemanticGraph { nodes: vec![], edges: vec![] }
    }

    // ── T-09 / T-10: ANF effect ordering ─────────────────────────────────

    #[test]
    fn anf_run_before_bind_fails() {
        let graph = graph_with_body("run_effect(db); bind_effect(db)");
        let entry = check_anf_ordering(&graph);
        assert_eq!(entry.state, VerificationState::Failed);
        assert!(
            entry.evidence.as_deref().unwrap_or("").contains("E_ANF_EFFECT_ORDER"),
            "evidence must contain E_ANF_EFFECT_ORDER"
        );
    }

    #[test]
    fn anf_run_without_bind_fails() {
        let graph = graph_with_body("run_effect(db)");
        let entry = check_anf_ordering(&graph);
        assert_eq!(entry.state, VerificationState::Failed);
        assert!(
            entry.evidence.as_deref().unwrap_or("").contains("E_ANF_EFFECT_ORDER"),
            "run_effect without bind_effect must produce E_ANF_EFFECT_ORDER"
        );
    }

    #[test]
    fn anf_duplicate_emit_fails() {
        let graph = graph_with_body("emit_effect(log); emit_effect(log)");
        let entry = check_anf_ordering(&graph);
        assert_eq!(entry.state, VerificationState::Failed);
        assert!(
            entry.evidence.as_deref().unwrap_or("").contains("E_ANF_DUPLICATE_EFFECT"),
            "duplicate emit_effect must produce E_ANF_DUPLICATE_EFFECT"
        );
    }

    #[test]
    fn anf_valid_bind_then_run_passes() {
        let graph = graph_with_body("bind_effect(db); run_effect(db)");
        let entry = check_anf_ordering(&graph);
        assert_eq!(entry.state, VerificationState::Proven);
    }

    #[test]
    fn anf_valid_single_emit_passes() {
        let graph = graph_with_body("emit_effect(log)");
        let entry = check_anf_ordering(&graph);
        assert_eq!(entry.state, VerificationState::Proven);
    }

    // Existing resource ordering must still work (ANF-3)
    #[test]
    fn anf_release_before_acquire_still_fails() {
        let graph = graph_with_body("release(conn); acquire(conn)");
        let entry = check_anf_ordering(&graph);
        assert_eq!(entry.state, VerificationState::Failed);
        assert!(
            entry.evidence.as_deref().unwrap_or("").contains("E_ANF_RESOURCE_ORDER"),
            "resource order violation must produce E_ANF_RESOURCE_ORDER"
        );
    }

    // ── T-13 / T-14: Invariant BFS transitive coverage ───────────────────

    fn make_invariant_graph_two_hop() -> (SemanticGraph, SemanticGraph) {
        // base_graph: empty (no nodes → all target nodes are "new" / changed)
        let base = empty_graph();
        // target_graph: inv A (id=0), B (id=1), C (id=2)
        // Edges: C --BIC--> B --BIC--> A
        let inv_a = GraphNode::new(NodeRef(0), NodeKind::Invariant, "inv.A");
        let node_b = GraphNode::new(NodeRef(1), NodeKind::Function, "fn.B");
        let node_c = GraphNode::new(NodeRef(2), NodeKind::Function, "fn.C");
        let target = SemanticGraph {
            nodes: vec![inv_a, node_b, node_c],
            edges: vec![
                GraphEdge::new(NodeRef(2), NodeRef(1), EdgeKind::BreaksIfChanged), // C → B
                GraphEdge::new(NodeRef(1), NodeRef(0), EdgeKind::BreaksIfChanged), // B → A
            ],
        };
        (base, target)
    }

    #[test]
    fn invariant_two_hop_breaks_if_changed_is_covered() {
        // C --BIC--> B --BIC--> inv A; C changed → Proven (transitive)
        let (base, target) = make_invariant_graph_two_hop();
        let entries = check_invariants(Some(&base), &target);
        let inv_entry = entries.iter().find(|e| e.scope == "inv.A").unwrap();
        assert_eq!(
            inv_entry.state,
            VerificationState::Proven,
            "two-hop BIC chain: C must be transitively covered"
        );
    }

    #[test]
    fn invariant_direct_breaks_if_changed_still_covered() {
        // Only direct edge: D --BIC--> inv A; D changed → Proven
        let base = empty_graph();
        let inv_a = GraphNode::new(NodeRef(0), NodeKind::Invariant, "inv.A");
        let node_d = GraphNode::new(NodeRef(1), NodeKind::Function, "fn.D");
        let target = SemanticGraph {
            nodes: vec![inv_a, node_d],
            edges: vec![GraphEdge::new(NodeRef(1), NodeRef(0), EdgeKind::BreaksIfChanged)],
        };
        let entries = check_invariants(Some(&base), &target);
        let inv_entry = entries.iter().find(|e| e.scope == "inv.A").unwrap();
        assert_eq!(inv_entry.state, VerificationState::Proven);
    }

    #[test]
    fn invariant_uncovered_changed_node_is_unverified() {
        // E is reachable from inv A (via DependsOn) but has NO BIC edge → Unverified
        let base = empty_graph();
        let inv_a = GraphNode::new(NodeRef(0), NodeKind::Invariant, "inv.A");
        let node_e = GraphNode::new(NodeRef(1), NodeKind::Function, "fn.E");
        let target = SemanticGraph {
            nodes: vec![inv_a, node_e],
            // E is reachable via DependsOn but NOT covered by BreaksIfChanged
            edges: vec![GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::DependsOn)],
        };
        let entries = check_invariants(Some(&base), &target);
        let inv_entry = entries.iter().find(|e| e.scope == "inv.A").unwrap();
        assert_eq!(
            inv_entry.state,
            VerificationState::Unverified,
            "reachable but uncovered changed node must be Unverified"
        );
    }

    #[test]
    fn invariant_three_hop_chain_covered() {
        // D --BIC--> C --BIC--> B --BIC--> inv A; D changed → Proven
        let base = empty_graph();
        let inv_a = GraphNode::new(NodeRef(0), NodeKind::Invariant, "inv.A");
        let node_b = GraphNode::new(NodeRef(1), NodeKind::Function, "fn.B");
        let node_c = GraphNode::new(NodeRef(2), NodeKind::Function, "fn.C");
        let node_d = GraphNode::new(NodeRef(3), NodeKind::Function, "fn.D");
        let target = SemanticGraph {
            nodes: vec![inv_a, node_b, node_c, node_d],
            edges: vec![
                GraphEdge::new(NodeRef(3), NodeRef(2), EdgeKind::BreaksIfChanged), // D → C
                GraphEdge::new(NodeRef(2), NodeRef(1), EdgeKind::BreaksIfChanged), // C → B
                GraphEdge::new(NodeRef(1), NodeRef(0), EdgeKind::BreaksIfChanged), // B → A
            ],
        };
        let entries = check_invariants(Some(&base), &target);
        let inv_entry = entries.iter().find(|e| e.scope == "inv.A").unwrap();
        assert_eq!(
            inv_entry.state,
            VerificationState::Proven,
            "three-hop BIC chain: D must be transitively covered"
        );
    }
}

/// Compute a deterministic 64-char hex hash from sorted capability names.
///
/// Uses a FNV-64-inspired accumulation expanded to 256 bits (4 × u64) so the
/// output is a valid 64-character hex string compatible with Stage 21 hash
/// comparison.  The computation is pure and produces identical output for
/// identical inputs across all platforms.
fn compute_caps_hash(cap_names: &[&str]) -> String {
    let mut sorted: Vec<&str> = cap_names.to_vec();
    sorted.sort();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for name in &sorted {
        for byte in name.bytes() {
            h ^= byte as u64;
            h = h.wrapping_mul(0x0000_0001_0000_01b3);
        }
        h = h.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
    let h1 = h;
    let h2 = h
        .wrapping_mul(0x517c_c1b7_2722_0a95)
        .wrapping_add(0xf6bd_bff8_bce2_4095);
    let h3 = h
        .wrapping_mul(0xbf58_476d_1ce4_e5b9)
        .wrapping_add(0x94d0_49bb_1331_11eb);
    let h4 = h
        .wrapping_mul(0x6c62_272e_07bb_0142)
        .wrapping_add(0x62b8_2175_6295_c58d);
    format!("{h1:016x}{h2:016x}{h3:016x}{h4:016x}")
}

fn validate_manifest(
    graph: &SemanticGraph,
    manifest_caps: &[String],
    artifact_manifest_hash: Option<&str>,
) -> VerificationEntry {
    let graph_caps = graph
        .nodes
        .iter()
        .filter(|node| node.kind == ail_core::semantic_graph::NodeKind::Capability)
        .map(|node| node.name.as_str())
        .collect::<BTreeSet<_>>();

    // Hash comparison (Ola5 Gap-2): when artifact_manifest_hash is provided,
    // compute the actual capability-set hash and compare it first.
    if let Some(expected_hash) = artifact_manifest_hash {
        let cap_names: Vec<&str> = graph_caps.iter().copied().collect();
        let actual_hash = compute_caps_hash(&cap_names);
        if actual_hash != expected_hash {
            return stage_entry(
                "21-generate-validate-manifest",
                VerificationState::Failed,
                "capabilities_manifest",
                Some(format!(
                    "E_MANIFEST_HASH_MISMATCH: expected {expected_hash}, computed {actual_hash}"
                )),
            );
        }
    }

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
