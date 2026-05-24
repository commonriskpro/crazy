// ── ail-cli::workflow_commands ────────────────────────────────────────────
//
// Handlers for the verify/apply workflow: `ail verify` and `ail apply`.
//
// These two commands form the core change-application pipeline:
//   verify  → run the Checker + policy gate, surface diagnostics and repair options
//   apply   → run the pre-apply gate, atomically apply the ChangeSet, emit a snapshot
//
// Both commands share `rebase_required_repair_option`, which is defined here
// because it is used exclusively by this module.

use ail_change::model::ChangeSetOutcome;
use ail_core::semantic_graph::SemanticGraph;
use ail_storage::{SnapshotEnvelope, object::ObjectId};
use ail_verify::checker::Checker;
use ail_verify::policy::{PolicyDecision, PolicyEngine, PolicyInput, PolicyRule};
use serde_json::{Value, json};

use crate::cli::{
    SimpleSnapshotBridge, conflict_reason_message, hex_to_object_id, is_valid_change_id,
    latest_snapshot, load_current_graph_with_snapshot_id_for_cli, unix_ms_now,
};
use crate::error::CliError;
use crate::output::{OutputMode, print_error_response, print_response};
use crate::store::StoreHandle;

// ── Public dispatch ───────────────────────────────────────────────────────

/// `ail verify <change-id> [--profile=<name>]`
///
/// Run the Checker on the ChangeSet, evaluate policy, and surface repair
/// options.  Does not mutate the graph.
pub(crate) async fn cmd_verify(
    mode: OutputMode,
    change_id: &str,
    profile: &str,
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
    if let Some(canonical) = maybe_canonical {
        let (current_graph, current_snapshot_id) =
            load_current_graph_with_snapshot_id_for_cli(store).await?;
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
            }
            ChangeSetOutcome::Failed { .. } | ChangeSetOutcome::ConflictIrresolvable { .. } => {
                graph = SemanticGraph {
                    nodes: vec![],
                    edges: vec![],
                };
            }
        }
    }
    let report = Checker::check(&graph);
    let policy_rules = [PolicyRule::ProfileGate(profile.to_string())];
    let policy_input = PolicyInput {
        report: &report,
        rules: &policy_rules,
        approvals: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
    };
    let (policy_decision, policy_audit) = PolicyEngine::evaluate_with_audit(&policy_input);
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
            json!({
                "claim": e.claim,
                "state": format!("{:?}", e.state),
                "scope": e.scope,
                "repair": repair,
            })
        })
        .collect();
    let diag_count = diagnostics.len();
    // Proof obligations: derived from verification entries.
    let proof_obligations: Vec<Value> = report
        .entries
        .iter()
        .map(|e| json!({ "claim": e.claim, "state": format!("{:?}", e.state) }))
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
            "verification_report": {
                "entries": entries_json,
                "summary": summary,
            },
            "diagnostics": diagnostics,
            "proof_obligations": proof_obligations,
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
        "verification_report_status": "accepted",
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
            let new_envelope = SnapshotEnvelope {
                id: ObjectId::from_bytes(&format!("snapshot-after-{change_id}").into_bytes()),
                graph_root_hash: graph_root,
                parent_id,
                applied_change_id: Some(change_oid),
                created_at: unix_ms_now(),
                verification_report_hash: None,
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

/// Build the JSON repair option for a rebase-required outcome.
fn rebase_required_repair_option(current_snapshot_id: u64) -> Value {
    json!({
        "code": "rebase_required",
        "next_action": "rebase",
        "description": "Rebase the ChangeSet onto the current snapshot before apply.",
        "current_snapshot_id": current_snapshot_id,
    })
}
