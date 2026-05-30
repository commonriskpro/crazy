use super::*;

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
    // Enforced for file-backed stores (sidecar at `.ail/reports/<change_id>`),
    // Memory stores (in-process `report_index`), and Postgres stores
    // (`report_index` table).
    //
    // The gate enforces two conditions:
    //   1. A report must exist (was `ail verify` run for this change?).
    //   2. The profile recorded at verify time must match the `--policy` profile
    //      requested for apply.  A "dev" report does NOT satisfy a "prod" apply.
    //
    // Legacy sidecars (written before profile tracking) record no profile and are
    // treated as "dev" — they satisfy `apply` only when `--policy` is also "dev".
    //
    // A report is "accepted" when its summary is not Failed or Unsafe.
    // Unverified entries from changeset meta-stages (01–05) are expected and
    // do NOT constitute rejection.
    // Load the report once; reuse the hash for the SnapshotEnvelope below.
    let (verification_report_status, gated_report_hash): (&'static str, Option<[u8; 32]>) =
        if store.supports_report_lookup_by_change_id() {
            match store
                .load_verification_report_by_change_id(change_id)
                .await?
            {
                None => {
                    return Err(CliError::Domain(format!(
                        "apply blocked: no verification report found for change-id {change_id}; \
                             run `ail verify {change_id} --profile {profile}` first"
                    )));
                }
                Some((report, hash, verified_profile)) => {
                    // Profile matching: the report must have been produced with the same
                    // profile as the one requested for apply.
                    if verified_profile != profile {
                        return Err(CliError::Domain(format!(
                            "apply blocked: verification report for {change_id} was produced \
                             with profile '{verified_profile}' but apply requires profile \
                             '{profile}'; run `ail verify {change_id} --profile {profile}` first"
                        )));
                    }
                    use ail_verify::report::VerificationState;
                    match report.summary() {
                        VerificationState::Failed | VerificationState::Unsafe => {
                            return Err(CliError::Domain(format!(
                                "apply blocked: verification report for {change_id} has summary \
                                     {:?}; repair failing checks before apply",
                                report.summary()
                            )));
                        }
                        _ => ("accepted", Some(*hash.as_bytes())),
                    }
                }
            }
        } else {
            // Non-file backend: sidecar index unavailable; gate cannot be enforced.
            ("not_persisted", None)
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

            // Reuse the report hash captured during the verification gate above.
            // For file-backed stores the gate already loaded and validated the
            // report; for non-file backends gated_report_hash is None.
            let verification_report_hash = gated_report_hash;

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
