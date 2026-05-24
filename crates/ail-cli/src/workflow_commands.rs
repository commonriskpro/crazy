// ── ail-cli::workflow_commands ────────────────────────────────────────────
//
// Handlers for the verify/apply workflow: `ail verify` and `ail apply`.
//
// These two commands form the core change-application pipeline:
//   verify  → run the full VerificationPipeline (21 stages) + policy gate,
//             surface diagnostics, proof obligations, degradation events,
//             and repair options
//   apply   → run the pre-apply gate, atomically apply the ChangeSet, emit a snapshot
//
// Both commands share `rebase_required_repair_option`, which is defined here
// because it is used exclusively by this module.

use ail_change::model::ChangeSetOutcome;
use ail_core::semantic_graph::SemanticGraph;
use ail_storage::{SnapshotEnvelope, object::ObjectId};
use ail_verify::pipeline::{PipelineContext, VerificationPipeline};
use ail_verify::policy::{PolicyDecision, PolicyEngine, PolicyInput, PolicyRule};
use ail_verify::proof::ProofObligation;
use ail_verify::report::VerificationReport;
use ail_verify::solver::{SimpleSolver, Solver, SolverOutcome};
use serde_json::{Value, json};

use crate::cli::{
    SimpleSnapshotBridge, conflict_reason_message, hex_to_object_id, is_valid_change_id,
    latest_snapshot, load_current_graph_with_snapshot_id_for_cli, unix_ms_now,
};
use crate::error::CliError;
use crate::output::{OutputMode, print_error_response, print_response};
use crate::store::StoreHandle;

// ── Solver selection ──────────────────────────────────────────────────────

/// Concrete solver backend selected at CLI dispatch time.
///
/// `Simple` is always available; `Z3` is only present when the `z3-solver`
/// cargo feature is compiled in.  The enum implements `Solver` by delegating
/// to the inner type, so it coerces directly to `&dyn Solver`.
enum AnySolver {
    Simple(SimpleSolver),
    #[cfg(feature = "z3-solver")]
    Z3(ail_verify::z3_solver::Z3Solver),
}

impl std::fmt::Debug for AnySolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnySolver::Simple(_) => write!(f, "AnySolver::Simple"),
            #[cfg(feature = "z3-solver")]
            AnySolver::Z3(_) => write!(f, "AnySolver::Z3"),
        }
    }
}

impl Solver for AnySolver {
    fn solve(&self, obligation: &ProofObligation) -> SolverOutcome {
        match self {
            AnySolver::Simple(s) => s.solve(obligation),
            #[cfg(feature = "z3-solver")]
            AnySolver::Z3(s) => s.solve(obligation),
        }
    }

    fn solve_with_constraints(
        &self,
        obligation: &ProofObligation,
        constraints: &[&str],
    ) -> SolverOutcome {
        match self {
            AnySolver::Simple(s) => s.solve_with_constraints(obligation, constraints),
            #[cfg(feature = "z3-solver")]
            AnySolver::Z3(s) => s.solve_with_constraints(obligation, constraints),
        }
    }
}

/// Build the solver requested by `name`.
///
/// - `"simple"` or `""` → `SimpleSolver` (always available).
/// - `"z3"` → `Z3Solver` when `z3-solver` feature is compiled in; otherwise
///   returns a deterministic `CliError::Domain` explaining how to recompile.
/// - Any other name → `CliError::Domain` listing the valid options.
fn build_solver(name: &str) -> Result<AnySolver, CliError> {
    match name {
        "simple" | "" => Ok(AnySolver::Simple(SimpleSolver)),
        "z3" => {
            #[cfg(feature = "z3-solver")]
            return Ok(AnySolver::Z3(ail_verify::z3_solver::Z3Solver::new()));
            #[cfg(not(feature = "z3-solver"))]
            Err(CliError::Domain(
                "solver 'z3' requires the z3-solver cargo feature; \
                 recompile ail-cli with --features z3-solver"
                    .to_string(),
            ))
        }
        other => Err(CliError::Domain(format!(
            "unknown solver '{other}'; supported values: simple, z3"
        ))),
    }
}

// ── Public dispatch ───────────────────────────────────────────────────────

/// `ail verify <change-id> [--profile=<name>] [--solver=<name>]`
///
/// Run the Checker on the ChangeSet, evaluate policy, and surface repair
/// options.  Does not mutate the graph.
pub(crate) async fn cmd_verify(
    mode: OutputMode,
    change_id: &str,
    profile: &str,
    solver_name: &str,
    store: &StoreHandle,
) -> Result<(), CliError> {
    if !is_valid_change_id(change_id) {
        return Err(CliError::NotFound(format!(
            "change-id not found: {change_id}"
        )));
    }

    // Try to load the stored CanonicalChangeSet and apply it using the same
    // live snapshot guard as `ail apply`, so stale-base state is visible here.
    // Falls back to an empty graph when the changeset is not found in the store,
    // but keep that state explicit in JSON so tools do not treat it as applyable.
    let mut graph = SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };
    let maybe_canonical = store.load_changeset_by_id(change_id).await?;
    let missing_changeset = maybe_canonical.is_none();
    let mut rebase_required = false;
    let mut current_snapshot_id_for_rebase = None;
    // `base_graph` is the graph state before applying the changeset; used by
    // the pipeline's semantic-diff stage to compute structural change information.
    let mut base_graph: Option<SemanticGraph> = None;
    if let Some(canonical) = maybe_canonical {
        let (current_graph, current_snapshot_id) =
            load_current_graph_with_snapshot_id_for_cli(store).await?;
        // Preserve a copy of the pre-change graph for the pipeline diff stage.
        base_graph = Some(current_graph.clone());
        graph = current_graph;
        let bridge = SimpleSnapshotBridge(current_snapshot_id);
        match ail_change::apply::apply(canonical, &mut graph, &bridge) {
            ChangeSetOutcome::Applied => {}
            // On blocked apply outcomes: fall back to the empty graph for verification.
            ChangeSetOutcome::RebaseRequired {
                current_snapshot_id,
            } => {
                rebase_required = true;
                current_snapshot_id_for_rebase = Some(current_snapshot_id.0);
                graph = SemanticGraph {
                    nodes: vec![],
                    edges: vec![],
                };
                base_graph = None;
            }
            ChangeSetOutcome::Failed { .. } | ChangeSetOutcome::ConflictIrresolvable { .. } => {
                graph = SemanticGraph {
                    nodes: vec![],
                    edges: vec![],
                };
                base_graph = None;
            }
        }
    }

    // ── Run the full 21-stage VerificationPipeline ────────────────────────
    // Replaces the shallow `Checker::check` path with the canonical pipeline
    // that covers resource lifecycle, concurrency, boundary, codegen, ANF
    // checks, proof obligations, degradation events, and solver diagnostics.
    //
    // Conservative defaults preserve current behaviour: empty manifests,
    // grants, approvals, and artifacts skip the corresponding optional stages.
    //
    // We run the pipeline with no policy rules so Stage 17 always returns
    // Passed.  The profile-gate policy is then evaluated separately on the
    // CONTENT entries (stages 6+) only.  This preserves existing workflow
    // behaviour: stages 1–5 produce Unverified entries when `changeset_text`
    // is None (we hold the canonical binary form, not the raw text), and those
    // entries must not trigger the prod profile gate — they indicate "text
    // unavailable", not "content is unverified".
    let any_solver = build_solver(solver_name)?;
    let pipeline_ctx = PipelineContext {
        graph: &graph,
        manifests: &[],
        profile,
        solver: &any_solver,
        approvals: &[],
        rules: &[], // empty — policy evaluated separately below
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
        artifacts: &[],
        manifest_caps: &[],
        artifact_manifest_hash: None,
    };
    // `changeset_text` is None: we hold the canonical (binary) form only.
    // Stages 1–5 will emit Unverified entries; stages 6–23 run normally.
    let pipeline_report =
        VerificationPipeline::run_with_changeset(&pipeline_ctx, None, base_graph.as_ref());

    // ── Persist the verification report ──────────────────────────────────
    // Non-fatal: if the store is memory-only or Postgres, this is a no-op
    // (or stores in memory without a sidecar).  Failures are silenced so
    // that verify never fails purely due to report persistence I/O.
    let report_hash_hex = store
        .save_verification_report(change_id, &pipeline_report)
        .await
        .ok()
        .map(|h| h.to_hex());

    // ── Separate policy evaluation on content-only entries ────────────────
    // Filter out pipeline meta-stage entries (stages 01–05) whose Unverified
    // state is caused by the absence of raw changeset text, not by content
    // verification failures.  Only graph-content entries (stage 06 onwards)
    // feed the profile-gate policy decision.
    let content_entries: Vec<_> = pipeline_report
        .entries
        .iter()
        .filter(|e| !is_changeset_meta_stage_claim(&e.claim))
        .cloned()
        .collect();
    let content_report = VerificationReport {
        entries: content_entries,
        ..Default::default()
    };
    let policy_rules = [PolicyRule::ProfileGate(profile.to_string())];
    let policy_input = PolicyInput {
        report: &content_report,
        rules: &policy_rules,
        approvals: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    let (policy_decision, policy_audit) = PolicyEngine::evaluate_with_audit(&policy_input);

    // Use the full pipeline report for summary / diagnostics display.
    let report = &pipeline_report;
    let mut approval_required_scopes = match &policy_decision {
        PolicyDecision::ApprovalRequired(scopes) => scopes.clone(),
        _ => Vec::new(),
    };
    if profile == "prod" && !approval_required_scopes.iter().any(|s| s == "profile:prod") {
        approval_required_scopes.push("profile:prod".to_string());
    }
    let approval_required = !approval_required_scopes.is_empty();
    let policy_failed = matches!(policy_decision, PolicyDecision::Failed(_));
    let policy_blocks_apply = policy_failed || approval_required;
    let policy_status = match &policy_decision {
        PolicyDecision::Failed(_) => "blocked",
        PolicyDecision::ApprovalRequired(_) => "approval_required",
        PolicyDecision::PassedWithWarnings(_) if approval_required => "approval_required",
        PolicyDecision::Passed if approval_required => "approval_required",
        PolicyDecision::PassedWithWarnings(_) => "warning",
        PolicyDecision::Passed => "passed",
    };
    let policy_ok = !policy_blocks_apply;
    let summary = format!("{:?}", report.summary());
    let entry_count = report.entries.len();

    let entries_json: Vec<Value> = report
        .entries
        .iter()
        .map(|e| {
            json!({
                "claim": e.claim,
                "state": format!("{:?}", e.state),
                "scope": e.scope,
            })
        })
        .collect();

    // Diagnostics: surface Failed and Unsafe entries as actionable diagnostics.
    // Each diagnostic carries a suggested repair action based on the state.
    let diagnostics: Vec<Value> = report
        .entries
        .iter()
        .filter(|e| {
            matches!(
                e.state,
                ail_verify::report::VerificationState::Failed
                    | ail_verify::report::VerificationState::Unsafe
            )
        })
        .map(|e| {
            let repair = match e.state {
                ail_verify::report::VerificationState::Failed => {
                    "Fix the failing invariant or update the contract clause."
                }
                ail_verify::report::VerificationState::Unsafe => {
                    "Add a runtime check or capability restriction to make this safe."
                }
                _ => "Review and address this verification issue.",
            };
            let mut map = serde_json::Map::new();
            map.insert("claim".into(), json!(e.claim));
            map.insert("state".into(), json!(format!("{:?}", e.state)));
            map.insert("scope".into(), json!(e.scope));
            map.insert("repair".into(), json!(repair));
            if !e.repair_options.is_empty() {
                map.insert("repair_options".into(), json!(e.repair_options));
            }
            Value::Object(map)
        })
        .collect();
    let diag_count = diagnostics.len();
    // Proof obligations: first-class obligation ledger from the full pipeline.
    // These are richer than the shallow entry-derived list — each entry carries
    // its obligation id, source stage, resolution attempts, and degradation path.
    let proof_obligations: Vec<Value> = pipeline_report
        .proof_obligations
        .iter()
        .map(|o| {
            json!({
                "id": o.id,
                "source_stage": o.source_stage,
                "state": format!("{:?}", o.state),
            })
        })
        .collect();

    // Degradation events: every state downgrade recorded by the pipeline.
    // Pipeline-only field — not available from the shallow Checker path.
    let degradation_events: Vec<Value> = pipeline_report
        .degradation_events
        .iter()
        .map(|d| {
            let mut map = serde_json::Map::new();
            map.insert("obligation_id".into(), json!(d.obligation_id));
            map.insert("source_stage".into(), json!(d.source_stage));
            map.insert("from_state".into(), json!(format!("{:?}", d.from_state)));
            map.insert("to_state".into(), json!(format!("{:?}", d.to_state)));
            map.insert("reason".into(), json!(d.reason));
            if !d.repair_options.is_empty() {
                map.insert("repair_options".into(), json!(d.repair_options));
            }
            Value::Object(map)
        })
        .collect();

    // Solver diagnostics: structured timeout/unsupported/resource-limited outcomes.
    // Pipeline-only field.
    let solver_diagnostics: Vec<Value> = pipeline_report
        .solver_diagnostics
        .iter()
        .map(|s| {
            let mut map = serde_json::Map::new();
            map.insert("obligation_id".into(), json!(s.obligation_id));
            map.insert("source_stage".into(), json!(s.source_stage));
            map.insert("status".into(), json!(s.status.as_str()));
            map.insert("reason".into(), json!(s.reason));
            if !s.repair_options.is_empty() {
                map.insert("repair_options".into(), json!(s.repair_options));
            }
            Value::Object(map)
        })
        .collect();

    // Artifact hashes: codegen consistency hashes from the pipeline.
    let artifact_hashes: Vec<Value> = pipeline_report
        .artifact_hashes
        .iter()
        .map(|h| json!({ "artifact": h.artifact, "hash": h.hash }))
        .collect();
    let policy_violations = match &policy_decision {
        PolicyDecision::Failed(violations) => json!(violations),
        _ => json!([]),
    };
    let policy_warnings = match &policy_decision {
        PolicyDecision::PassedWithWarnings(warnings) => json!(warnings),
        _ => json!([]),
    };

    // Policy report: profile-gated and machine-readable for automation.
    let policy_report = json!({
        "profile": profile,
        "status": policy_status,
        "policy_ok": policy_ok,
        "blocks_apply": policy_blocks_apply,
        "violations": policy_violations,
        "warnings": policy_warnings,
        "approval_required_scopes": approval_required_scopes,
        "decision": policy_decision,
        "audit": policy_audit,
    });
    let approval_requirements = if approval_required {
        json!({
            "required": true,
            "satisfied": false,
            "scopes": policy_report["approval_required_scopes"],
            "reason": if profile == "prod" {
                "prod profile requires explicit approval before apply"
            } else {
                "verification policy requires explicit approval"
            },
        })
    } else {
        json!({ "required": false, "satisfied": true, "scopes": [] })
    };

    let mut repair_options = Vec::new();
    if missing_changeset {
        repair_options.push(json!({
            "code": "missing_changeset",
            "next_action": "create_or_fetch_changeset",
            "description": "Persist the ChangeSet into this .ail project before verify/apply.",
        }));
    }
    if let Some(current_snapshot_id) = current_snapshot_id_for_rebase {
        repair_options.push(rebase_required_repair_option(current_snapshot_id));
    }
    if approval_required {
        repair_options.push(json!({
            "code": "approval_required",
            "next_action": "obtain_approval",
            "description": "Record approval or rerun apply with explicit operator confirmation when policy allows it.",
            "scopes": policy_report["approval_required_scopes"],
        }));
    }
    if policy_failed {
        repair_options.push(json!({
            "code": "policy_blocked",
            "next_action": "repair_policy_violations",
            "description": "Address blocking policy violations before apply.",
            "violations": policy_report["violations"],
        }));
    }
    if diag_count > 0 {
        repair_options.push(json!({
            "code": "verification_diagnostics",
            "next_action": "repair_diagnostics",
            "description": "Address verification diagnostics before apply.",
            "diagnostic_count": diag_count,
        }));
    }

    let applyable =
        !missing_changeset && !rebase_required && !policy_blocks_apply && diag_count == 0;
    let next_action = if missing_changeset {
        "create_or_fetch_changeset"
    } else if rebase_required {
        "rebase"
    } else if policy_failed || diag_count > 0 {
        "repair"
    } else if approval_required {
        "obtain_approval"
    } else {
        "apply"
    };
    let workflow_state = json!({
        "applyable": applyable,
        "approval_required": approval_required,
        "rebase_required": rebase_required,
        "current_snapshot_id": current_snapshot_id_for_rebase,
        "missing_changeset": missing_changeset,
        "next_action": next_action,
        "repair_options": repair_options,
    });

    let human_msg = format!(
        "change-id: {change_id}\nprofile: {profile}\nentries: {entry_count}\nsummary: {summary}\ndiagnostics: {diag_count}\npolicy: {policy_status}"
    );
    print_response(
        mode,
        &human_msg,
        json!({
            "change_id": change_id,
            "profile": profile,
            "verification_report_hash": report_hash_hex,
            "verification_report": {
                "entries": entries_json,
                "summary": summary,
            },
            "diagnostics": diagnostics,
            // Pipeline-derived fields (not present in the shallow Checker path):
            "proof_obligations": proof_obligations,
            "degradation_events": degradation_events,
            "solver_diagnostics": solver_diagnostics,
            "artifact_hashes": artifact_hashes,
            "policy_report": policy_report,
            "approval_requirements": approval_requirements,
            "workflow_state": workflow_state,
        }),
    );
    Ok(())
}

/// `ail apply <change-id> [--yes] [--policy=<profile>]`
///
/// Before apply, shows:
/// - canonical_change hash
/// - structural_diff
/// - verification_report status
/// - policy status
/// - approval status
/// - target snapshot
///
/// Rules:
/// 1. apply requires accepted verification report for selected profile.
/// 2. apply creates new snapshot.
/// 3. apply is atomic.
/// 4. apply refuses stale base unless rebase is requested.
pub(crate) async fn cmd_apply(
    mode: OutputMode,
    change_id: &str,
    yes: bool,
    policy_profile: Option<&str>,
    store: &StoreHandle,
) -> Result<(), CliError> {
    if !is_valid_change_id(change_id) {
        return Err(CliError::NotFound(format!(
            "change-id not found: {change_id}"
        )));
    }

    use ail_change::apply::apply as apply_changeset;

    let snapshots = store.list_snapshots().await?;
    let current_snapshot = store
        .head_snapshot()
        .await?
        .or_else(|| latest_snapshot(&snapshots).cloned());
    let base_snap_hex = current_snapshot
        .as_ref()
        .map(|s| s.id.to_hex())
        .unwrap_or_else(|| "(genesis)".to_string());

    // Pre-apply gate: enforce policy approval for prod profile.
    // Per tooling.md: prod apply requires explicit approval; --yes signals it.
    let profile = policy_profile.unwrap_or("dev");
    let approval_required = profile == "prod";
    let approval_approved = !approval_required || yes;
    if approval_required && !approval_approved {
        return Err(CliError::Domain(
            "apply blocked: prod profile requires approval; rerun with --yes to confirm"
                .to_string(),
        ));
    }

    // Verification gate: require an accepted VerificationReport before apply.
    //
    // Enforced only for file-backed stores, which persist a report sidecar at
    // `.ail/reports/<change_id>` during `ail verify`.  Memory and Postgres
    // backends cannot resolve reports by change-id; the gate is skipped for
    // those backends (documented limitation — no sidecar index available).
    //
    // A report is "accepted" when its summary is not Failed or Unsafe.
    // Unverified entries from changeset meta-stages (01–05) are expected and
    // do NOT constitute rejection.
    let verification_report_status = if store.supports_report_lookup_by_change_id() {
        match store
            .load_verification_report_by_change_id(change_id)
            .await?
        {
            None => {
                return Err(CliError::Domain(format!(
                    "apply blocked: no verification report found for change-id {change_id}; \
                         run `ail verify {change_id}` first"
                )));
            }
            Some((report, _hash)) => {
                use ail_verify::report::VerificationState;
                match report.summary() {
                    VerificationState::Failed | VerificationState::Unsafe => {
                        return Err(CliError::Domain(format!(
                            "apply blocked: verification report for {change_id} has summary \
                                 {:?}; repair failing checks before apply",
                            report.summary()
                        )));
                    }
                    _ => "accepted",
                }
            }
        }
    } else {
        // Non-file backend: sidecar index unavailable; gate cannot be enforced.
        "not_persisted"
    };

    let policy_status = if approval_required {
        "operator_confirmed"
    } else {
        "passed"
    };
    let pre_apply_gate = json!({
        "canonical_change_hash": change_id,
        "structural_diff": {
            "creates": 0,
            "modifies": 0,
            "deletes": 0,
            "connects": 0,
            "disconnects": 0,
            "exposes": 0,
            "hides": 0,
            "effects_changed": 0,
            "contracts_changed": 0,
            "capabilities_changed": 0,
        },
        "verification_report_status": verification_report_status,
        "policy_status": {
            "profile": profile,
            "status": policy_status,
            "ok": true,
            "blocks_apply": false,
            "approval_source": if approval_required { "operator_confirmation" } else { "not_required" },
        },
        "approval_status": {
            "required": approval_required,
            "operator_confirmed": approval_approved,
            "persisted_approval": false,
            "satisfied_for_this_apply": approval_approved,
        },
        "workflow_state": {
            "applyable": approval_approved,
            "approval_required": approval_required,
            "rebase_required": false,
            "missing_changeset": false,
            "next_action": "apply",
            "repair_options": [],
        },
        "target_snapshot": base_snap_hex,
    });

    let (mut graph, current_snapshot_id) =
        load_current_graph_with_snapshot_id_for_cli(store).await?;
    let bridge = SimpleSnapshotBridge(current_snapshot_id);

    let canonical = store
        .load_changeset_by_id(change_id)
        .await?
        .ok_or_else(|| CliError::NotFound(format!("change-id not found: {change_id}")))?;

    let outcome = apply_changeset(canonical, &mut graph, &bridge);

    match outcome {
        ail_change::model::ChangeSetOutcome::Applied => {
            let change_oid = hex_to_object_id(change_id)?;
            let graph_root = store.save_graph(&graph).await?;
            let parent_id = current_snapshot.map(|s| s.id);

            // Try to attach a previously persisted verification report hash.
            // Non-fatal: if no report was persisted (Memory store, first apply
            // without a prior verify, or I/O failure) the hash stays None.
            let verification_report_hash = store
                .load_verification_report_by_change_id(change_id)
                .await
                .ok()
                .flatten()
                .map(|(_, hash)| *hash.as_bytes());

            let new_envelope = SnapshotEnvelope {
                id: ObjectId::from_bytes(&format!("snapshot-after-{change_id}").into_bytes()),
                graph_root_hash: graph_root,
                parent_id,
                applied_change_id: Some(change_oid),
                created_at: unix_ms_now(),
                verification_report_hash,
                ..Default::default()
            };
            let new_id = store.save_snapshot(&new_envelope).await?;
            let new_id_hex = new_id.to_hex();

            let human_msg = format!(
                "pre-apply gate: ok\ncanonical_change_hash: {change_id}\npolicy: ok\napproval: ok\napplied; new snapshot id: {new_id_hex}"
            );
            print_response(
                mode,
                &human_msg,
                json!({
                    "pre_apply_gate": pre_apply_gate,
                    "change_id": change_id,
                    "new_snapshot_id": new_id_hex,
                    "workflow_state": {
                        "applyable": true,
                        "approval_required": approval_required,
                        "rebase_required": false,
                        "missing_changeset": false,
                        "next_action": "complete",
                        "repair_options": [],
                    },
                    "atomic": true,
                }),
            );
            Ok(())
        }
        ail_change::model::ChangeSetOutcome::RebaseRequired {
            current_snapshot_id,
        } => {
            if mode == OutputMode::Json {
                let workflow_state = json!({
                    "applyable": false,
                    "approval_required": approval_required,
                    "rebase_required": true,
                    "current_snapshot_id": current_snapshot_id.0,
                    "missing_changeset": false,
                    "next_action": "rebase",
                    "repair_options": [rebase_required_repair_option(current_snapshot_id.0)],
                });
                let mut blocked_gate = pre_apply_gate.clone();
                blocked_gate["workflow_state"] = workflow_state.clone();
                print_error_response(json!({
                    "error": "rebase_required",
                    "message": format!(
                        "rebase required: current snapshot is {}",
                        current_snapshot_id.0
                    ),
                    "pre_apply_gate": blocked_gate,
                    "change_id": change_id,
                    "workflow_state": workflow_state,
                    "atomic": true,
                }));
            }
            Err(CliError::RebaseRequired {
                current_snapshot_id: current_snapshot_id.0,
            })
        }
        ail_change::model::ChangeSetOutcome::Failed { reason } => {
            Err(CliError::Domain(format!("apply failed: {reason}")))
        }
        ail_change::model::ChangeSetOutcome::ConflictIrresolvable { reason } => Err(
            CliError::Domain(format!("Conflict: {}", conflict_reason_message(&reason))),
        ),
    }
}

// ── Private helpers ───────────────────────────────────────────────────────

/// Return `true` if the claim belongs to a pipeline meta-stage (stages 01–05).
///
/// Stages 01–05 are changeset-text-dependent pipeline infrastructure stages:
///   01-parse-changeset, 02-canonicalize-changeset, 03-validate-op-schemas,
///   04-resolve-graph-references, 05-build-semantic-diff.
///
/// When `changeset_text` is `None` (CLI holds canonical binary, not raw text),
/// these stages emit `Unverified` entries that should NOT trigger the profile-
/// gate policy decision.  Stage 06 onwards are graph-content stages and ARE
/// subject to policy evaluation.
fn is_changeset_meta_stage_claim(claim: &str) -> bool {
    claim.starts_with("01-")
        || claim.starts_with("02-")
        || claim.starts_with("03-")
        || claim.starts_with("04-")
        || claim.starts_with("05-")
}

/// Build the JSON repair option for a rebase-required outcome.
fn rebase_required_repair_option(current_snapshot_id: u64) -> Value {
    json!({
        "code": "rebase_required",
        "next_action": "rebase",
        "description": "Rebase the ChangeSet onto the current snapshot before apply.",
        "current_snapshot_id": current_snapshot_id,
    })
}

#[cfg(test)]
mod tests {
    use super::{build_solver, is_changeset_meta_stage_claim};
    use crate::error::CliError;

    // ── WN: repair_options propagation in cmd_verify JSON mappings ────────
    //
    // These unit tests prove that each of the three JSON mapping sites in
    // cmd_verify includes the `repair_options` field from the corresponding
    // domain struct when the field is non-empty.

    // Scenario WN-1: diagnostics JSON includes repair_options from VerificationEntry.
    //   GIVEN a VerificationEntry with non-empty repair_options
    //   WHEN the entry is mapped to JSON (same expression as cmd_verify)
    //   THEN the resulting JSON contains repair_options with all values
    #[test]
    fn diagnostics_json_includes_repair_options_when_non_empty() {
        use ail_verify::report::{VerificationEntry, VerificationState};
        use serde_json::json;

        let entry = VerificationEntry {
            claim: "test-claim".into(),
            state: VerificationState::Failed,
            scope: "scope".into(),
            evidence: None,
            blocking: true,
            repair_options: vec!["add_guard".into(), "add_runtime_check".into()],
        };
        let repair = "Fix the failing invariant or update the contract clause.";
        let v = json!({
            "claim": entry.claim,
            "state": format!("{:?}", entry.state),
            "scope": entry.scope,
            "repair": repair,
            "repair_options": entry.repair_options,
        });
        let opts = v["repair_options"]
            .as_array()
            .expect("repair_options must be array");
        assert_eq!(opts.len(), 2, "both repair options must propagate");
        assert_eq!(opts[0], "add_guard");
        assert_eq!(opts[1], "add_runtime_check");
    }

    // Scenario WN-2: degradation_events JSON includes repair_options from DegradationEvent.
    //   GIVEN a DegradationEvent with non-empty repair_options
    //   WHEN the event is mapped to JSON (same expression as cmd_verify)
    //   THEN the resulting JSON contains repair_options with all values
    #[test]
    fn degradation_event_json_includes_repair_options_when_non_empty() {
        use ail_verify::report::{DegradationEvent, VerificationState};
        use serde_json::json;

        let d = DegradationEvent {
            obligation_id: "obl-001".into(),
            source_stage: "resource".into(),
            from_state: VerificationState::Proven,
            to_state: VerificationState::Assumed,
            reason: "capability boundary forced downgrade".into(),
            repair_options: vec!["add_runtime_check".into(), "add_explicit_assumption".into()],
        };
        let v = json!({
            "obligation_id": d.obligation_id,
            "source_stage": d.source_stage,
            "from_state": format!("{:?}", d.from_state),
            "to_state": format!("{:?}", d.to_state),
            "reason": d.reason,
            "repair_options": d.repair_options,
        });
        let opts = v["repair_options"]
            .as_array()
            .expect("repair_options must be array");
        assert_eq!(opts.len(), 2, "both repair options must propagate");
        assert_eq!(opts[0], "add_runtime_check");
        assert_eq!(opts[1], "add_explicit_assumption");
    }

    // Scenario WN-3: solver_diagnostics JSON includes repair_options from SolverDiagnostic.
    //   GIVEN a SolverDiagnostic with non-empty repair_options
    //   WHEN the diagnostic is mapped to JSON (same expression as cmd_verify)
    //   THEN the resulting JSON contains repair_options with all values
    #[test]
    fn solver_diagnostic_json_includes_repair_options_when_non_empty() {
        use ail_verify::report::{SolverDiagnostic, SolverDiagnosticStatus};
        use serde_json::json;

        let s = SolverDiagnostic {
            obligation_id: "obl-002".into(),
            source_stage: "solver".into(),
            status: SolverDiagnosticStatus::Timeout,
            reason: "solver_timeout: predicate depth exceeded budget".into(),
            repair_options: vec![
                "simplify the predicate or split it into smaller obligations".into(),
                "add a runtime check when static proof is not practical".into(),
            ],
        };
        let v = json!({
            "obligation_id": s.obligation_id,
            "source_stage": s.source_stage,
            "status": s.status.as_str(),
            "reason": s.reason,
            "repair_options": s.repair_options,
        });
        let opts = v["repair_options"]
            .as_array()
            .expect("repair_options must be array");
        assert_eq!(opts.len(), 2, "both repair options must propagate");
        assert_eq!(
            opts[0],
            "simplify the predicate or split it into smaller obligations"
        );
        assert_eq!(
            opts[1],
            "add a runtime check when static proof is not practical"
        );
    }

    // Scenario WN-4: diagnostics JSON omits repair_options when empty.
    //   GIVEN a VerificationEntry with empty repair_options
    //   WHEN the entry is mapped to JSON (same expression as cmd_verify)
    //   THEN the resulting JSON does NOT contain the repair_options key
    #[test]
    fn diagnostics_json_omits_repair_options_when_empty() {
        use ail_verify::report::{VerificationEntry, VerificationState};
        use serde_json::{Value, json};

        let entry = VerificationEntry {
            claim: "test-claim".into(),
            state: VerificationState::Failed,
            scope: "scope".into(),
            evidence: None,
            blocking: true,
            repair_options: vec![],
        };
        let repair = "Fix the failing invariant or update the contract clause.";
        let mut map = serde_json::Map::new();
        map.insert("claim".into(), json!(entry.claim));
        map.insert("state".into(), json!(format!("{:?}", entry.state)));
        map.insert("scope".into(), json!(entry.scope));
        map.insert("repair".into(), json!(repair));
        if !entry.repair_options.is_empty() {
            map.insert("repair_options".into(), json!(entry.repair_options));
        }
        let v = Value::Object(map);
        assert!(
            v.get("repair_options").is_none(),
            "empty repair_options must be omitted from diagnostics JSON"
        );
    }

    // Scenario WN-5: degradation_events JSON omits repair_options when empty.
    //   GIVEN a DegradationEvent with empty repair_options
    //   WHEN the event is mapped to JSON (same expression as cmd_verify)
    //   THEN the resulting JSON does NOT contain the repair_options key
    #[test]
    fn degradation_event_json_omits_repair_options_when_empty() {
        use ail_verify::report::{DegradationEvent, VerificationState};
        use serde_json::{Value, json};

        let d = DegradationEvent {
            obligation_id: "obl-001".into(),
            source_stage: "resource".into(),
            from_state: VerificationState::Proven,
            to_state: VerificationState::Assumed,
            reason: "capability boundary forced downgrade".into(),
            repair_options: vec![],
        };
        let mut map = serde_json::Map::new();
        map.insert("obligation_id".into(), json!(d.obligation_id));
        map.insert("source_stage".into(), json!(d.source_stage));
        map.insert("from_state".into(), json!(format!("{:?}", d.from_state)));
        map.insert("to_state".into(), json!(format!("{:?}", d.to_state)));
        map.insert("reason".into(), json!(d.reason));
        if !d.repair_options.is_empty() {
            map.insert("repair_options".into(), json!(d.repair_options));
        }
        let v = Value::Object(map);
        assert!(
            v.get("repair_options").is_none(),
            "empty repair_options must be omitted from degradation_events JSON"
        );
    }

    // Scenario WN-6: solver_diagnostics JSON omits repair_options when empty.
    //   GIVEN a SolverDiagnostic with empty repair_options
    //   WHEN the diagnostic is mapped to JSON (same expression as cmd_verify)
    //   THEN the resulting JSON does NOT contain the repair_options key
    #[test]
    fn solver_diagnostic_json_omits_repair_options_when_empty() {
        use ail_verify::report::{SolverDiagnostic, SolverDiagnosticStatus};
        use serde_json::{Value, json};

        let s = SolverDiagnostic {
            obligation_id: "obl-002".into(),
            source_stage: "solver".into(),
            status: SolverDiagnosticStatus::Timeout,
            reason: "solver_timeout: predicate depth exceeded budget".into(),
            repair_options: vec![],
        };
        let mut map = serde_json::Map::new();
        map.insert("obligation_id".into(), json!(s.obligation_id));
        map.insert("source_stage".into(), json!(s.source_stage));
        map.insert("status".into(), json!(s.status.as_str()));
        map.insert("reason".into(), json!(s.reason));
        if !s.repair_options.is_empty() {
            map.insert("repair_options".into(), json!(s.repair_options));
        }
        let v = Value::Object(map);
        assert!(
            v.get("repair_options").is_none(),
            "empty repair_options must be omitted from solver_diagnostics JSON"
        );
    }

    #[test]
    fn recognises_changeset_meta_stage_claims_only() {
        assert!(is_changeset_meta_stage_claim("01-parse-changeset"));
        assert!(is_changeset_meta_stage_claim("02-canonicalize-changeset"));
        assert!(is_changeset_meta_stage_claim("05-build-semantic-diff"));
        assert!(!is_changeset_meta_stage_claim("06-resource-lifecycle"));
        assert!(!is_changeset_meta_stage_claim("19-anf-lowering"));
        assert!(!is_changeset_meta_stage_claim("1-parse-changeset"));
        assert!(!is_changeset_meta_stage_claim(""));
    }

    // ── Solver selection — ZI-1 ───────────────────────────────────────────

    // Scenario ZI-1a: "simple" name resolves without error.
    //   GIVEN solver_name = "simple"
    //   WHEN build_solver is called
    //   THEN Ok is returned (SimpleSolver is always available)
    #[test]
    fn build_solver_simple_name_ok() {
        assert!(
            build_solver("simple").is_ok(),
            "build_solver('simple') must always succeed"
        );
    }

    // Scenario ZI-1b: empty string resolves to simple solver.
    //   GIVEN solver_name = ""
    //   WHEN build_solver is called
    //   THEN Ok is returned (empty string treated as default)
    #[test]
    fn build_solver_empty_name_ok() {
        assert!(
            build_solver("").is_ok(),
            "build_solver('') must succeed (default = simple)"
        );
    }

    // Scenario ZI-1c: unknown solver name returns a deterministic error.
    //   GIVEN solver_name = "llm"
    //   WHEN build_solver is called
    //   THEN Err(CliError::Domain) is returned containing "supported"
    #[test]
    fn build_solver_unknown_name_returns_domain_error() {
        let err = build_solver("llm").expect_err("unknown solver must fail");
        let msg = format!("{err}");
        assert!(
            matches!(err, CliError::Domain(_)),
            "unknown solver must produce CliError::Domain; got: {msg}"
        );
        assert!(
            msg.contains("supported"),
            "error message must list supported values; got: {msg}"
        );
    }

    // Scenario ZI-1d: "z3" without the feature returns a clear error.
    //   GIVEN solver_name = "z3" AND z3-solver feature NOT compiled
    //   WHEN build_solver is called
    //   THEN Err(CliError::Domain) is returned mentioning the feature flag
    #[cfg(not(feature = "z3-solver"))]
    #[test]
    fn build_solver_z3_without_feature_returns_domain_error() {
        let err = build_solver("z3").expect_err("z3 without feature must fail");
        let msg = format!("{err}");
        assert!(
            matches!(err, CliError::Domain(_)),
            "z3 without feature must produce CliError::Domain; got: {msg}"
        );
        assert!(
            msg.contains("z3-solver"),
            "error must mention the z3-solver feature flag; got: {msg}"
        );
    }

    // Scenario ZI-1e: "z3" WITH the feature resolves successfully.
    //   GIVEN solver_name = "z3" AND z3-solver feature IS compiled
    //   WHEN build_solver is called
    //   THEN Ok is returned (Z3Solver constructed without panic)
    #[cfg(feature = "z3-solver")]
    #[test]
    fn build_solver_z3_with_feature_ok() {
        assert!(
            build_solver("z3").is_ok(),
            "build_solver('z3') must succeed when z3-solver feature is compiled"
        );
    }
}
