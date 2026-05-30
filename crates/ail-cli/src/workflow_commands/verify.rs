use super::*;

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
    let manifest_caps: Vec<String> = graph
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Capability)
        .map(|node| node.name.clone())
        .collect();
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
        manifest_caps: &manifest_caps,
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
    // The profile is persisted in the sidecar so `ail apply` can enforce
    // that the same profile was used during verification.
    let report_hash_hex = store
        .save_verification_report(change_id, profile, &pipeline_report)
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
