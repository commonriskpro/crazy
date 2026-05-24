// ── ail-cli::remote_commands ─────────────────────────────────────────────
//
// Handlers and business logic for the `ail remote` command surface.
//
// Dispatch entry point: `cmd_remote`.  All sub-command handlers live here,
// together with helpers for push, pull, submit, and bundle operations.
// The ephemeral in-process exchange model means no real network transport
// is used; see constants below for the authoritative notes.

use std::path::PathBuf;

use ail_change::model::SnapshotId;
use ail_coordinator::Coordinator;
use ail_core::semantic_graph::SemanticGraph;
use ail_remote::{
    AgentKeypair, FileBundleStore, ObjectBundle, RemoteChangeSet, RemoteExchangeRequest,
    RemoteExchangeResponse, RemoteSignerPolicy, RemoteSubmissionOutcome, SignerTrustTier,
    TrustedRemoteSigner,
};
use ail_storage::{error::StorageError, object::ObjectId};
use serde_json::{Value, json};

use crate::cli::{
    RemoteCmd, bytes_to_hex, hex_to_object_id, is_valid_change_id,
    load_current_graph_with_snapshot_id_for_cli,
};
use crate::error::CliError;
use crate::output::{OutputMode, print_response};
use crate::store::StoreHandle;

// ── Transport / bundle constants ─────────────────────────────────────────

const REMOTE_SUBMIT_TRANSPORT: &str = "in_process";
const REMOTE_SUBMIT_KEY_SOURCE: &str = "ephemeral_in_process";
const REMOTE_SUBMIT_NOTE: &str = "local in-process exchange only; no network transport is used; remote config is validated but not applied to the ephemeral signer policy";
const REMOTE_BUNDLE_TRANSPORT: &str = "local_file_bundle_store+in_process";
const REMOTE_BUNDLE_SCOPE_SINGLE_ROOT: &str = "single_root_object";
const REMOTE_BUNDLE_SCOPE_SNAPSHOT_DEPENDENCIES: &str = "root_with_snapshot_envelope_dependencies";
const REMOTE_BUNDLE_NOTE: &str = "local file bundle store only; snapshot envelope roots include available directly referenced stored objects; raw graph traversal remains opaque; no network transport is used and remote config is not consulted";

// ── Public dispatch ───────────────────────────────────────────────────────

pub(crate) async fn cmd_remote(
    mode: OutputMode,
    cmd: RemoteCmd,
    store: &StoreHandle,
) -> Result<(), CliError> {
    match cmd {
        RemoteCmd::Submit { change_id, signer } => {
            cmd_remote_submit(mode, &change_id, &signer, store).await
        }
        RemoteCmd::Push { root } => cmd_remote_push(mode, &root, store).await,
        RemoteCmd::Pull { root } => cmd_remote_pull(mode, &root, store).await,
    }
}

// ── Sub-command handlers ──────────────────────────────────────────────────

async fn cmd_remote_push(
    mode: OutputMode,
    root_hex: &str,
    store: &StoreHandle,
) -> Result<(), CliError> {
    let root = hex_to_object_id(root_hex)?;
    let (bundle_store, bundle_store_path) = remote_file_bundle_store(store)?;
    let bundle = build_remote_bundle(store, root).await?;
    bundle
        .verify_integrity()
        .map_err(|e| CliError::Domain(format!("remote bundle invalid: {e}")))?;

    let coordinator = Coordinator::new(
        SnapshotId(0),
        SemanticGraph {
            nodes: vec![],
            edges: vec![],
        },
    );
    let response = coordinator
        .handle_remote_exchange(RemoteExchangeRequest::PushBundle(bundle.clone()))
        .await;
    let RemoteExchangeResponse::BundleAccepted { object_count, .. } = response else {
        return Err(CliError::Domain(remote_exchange_error_message(response)));
    };
    bundle_store
        .try_put_bundle(&bundle)
        .map_err(|e| CliError::Domain(format!("remote bundle store failed: {e}")))?;

    let bundle_store_path = bundle_store_path.display().to_string();
    let bundle_scope = remote_bundle_scope(&bundle);
    print_response(
        mode,
        &format!(
            "remote push: {root}\ntransport: {REMOTE_BUNDLE_TRANSPORT}\nbundle store: {bundle_store_path}\nbundle scope: {bundle_scope}\nobject count: {object_count}\nnote: {REMOTE_BUNDLE_NOTE}"
        ),
        json!({
            "request": "PushBundle",
            "root": root.to_hex(),
            "transport": REMOTE_BUNDLE_TRANSPORT,
            "bundle_store_path": bundle_store_path,
            "bundle_scope": bundle_scope,
            "object_count": object_count,
            "note": REMOTE_BUNDLE_NOTE,
        }),
    );
    Ok(())
}

async fn cmd_remote_pull(
    mode: OutputMode,
    root_hex: &str,
    store: &StoreHandle,
) -> Result<(), CliError> {
    let root = hex_to_object_id(root_hex)?;
    let (bundle_store, bundle_store_path) = remote_file_bundle_store(store)?;
    let bundle = bundle_store
        .try_get_bundle(&root)
        .map_err(|e| CliError::Domain(format!("remote bundle store failed: {e}")))?
        .ok_or_else(|| CliError::NotFound(format!("remote bundle not found: {root}")))?;

    let coordinator = Coordinator::new(
        SnapshotId(0),
        SemanticGraph {
            nodes: vec![],
            edges: vec![],
        },
    );
    let accepted = coordinator
        .handle_remote_exchange(RemoteExchangeRequest::PushBundle(bundle))
        .await;
    if !matches!(accepted, RemoteExchangeResponse::BundleAccepted { .. }) {
        return Err(CliError::Domain(remote_exchange_error_message(accepted)));
    }
    let response = coordinator
        .handle_remote_exchange(RemoteExchangeRequest::PullBundle { root })
        .await;
    let RemoteExchangeResponse::Bundle(bundle) = response else {
        return Err(CliError::Domain(remote_exchange_error_message(response)));
    };

    let object_count = bundle.objects.len();
    let bundle_scope = remote_bundle_scope(&bundle);
    for (object_id, bytes) in bundle.objects {
        store.save_raw_object(&object_id, bytes).await?;
    }

    let bundle_store_path = bundle_store_path.display().to_string();
    print_response(
        mode,
        &format!(
            "remote pull: {root}\ntransport: {REMOTE_BUNDLE_TRANSPORT}\nbundle store: {bundle_store_path}\nbundle scope: {bundle_scope}\nobject count: {object_count}\nnote: {REMOTE_BUNDLE_NOTE}"
        ),
        json!({
            "request": "PullBundle",
            "root": root.to_hex(),
            "transport": REMOTE_BUNDLE_TRANSPORT,
            "bundle_store_path": bundle_store_path,
            "bundle_scope": bundle_scope,
            "object_count": object_count,
            "note": REMOTE_BUNDLE_NOTE,
        }),
    );
    Ok(())
}

async fn cmd_remote_submit(
    mode: OutputMode,
    change_id: &str,
    signer_ref: &str,
    store: &StoreHandle,
) -> Result<(), CliError> {
    if !is_valid_change_id(change_id) {
        return Err(CliError::NotFound(format!(
            "change-id not found: {change_id}"
        )));
    }
    if signer_ref.trim().is_empty() {
        return Err(CliError::ParseError(
            "--signer must be a non-empty key reference".to_string(),
        ));
    }

    let canonical = store
        .load_changeset_by_id(change_id)
        .await?
        .ok_or_else(|| CliError::NotFound(format!("change-id not found: {change_id}")))?;

    let project = crate::project::ProjectContext::from_cwd()?;
    let remote_config_path = crate::remote_config::remote_config_path(&project.ail_dir);
    let remote_config_source = if remote_config_path.exists() {
        "project_file"
    } else {
        "missing_default_deny_all"
    };
    let loaded_remote_policy = crate::remote_config::load_remote_signer_policy(&project.ail_dir)
        .map_err(|e| CliError::Domain(e.to_string()))?;
    let configured_allowed_signers = loaded_remote_policy.allowed_signers.len();

    let keypair = AgentKeypair::generate();
    let identity = keypair.identity();
    let policy =
        RemoteSignerPolicy::from_allowed_signers(vec![TrustedRemoteSigner::from_identity(
            &identity,
            SignerTrustTier::Trusted,
            Some(signer_ref.to_string()),
        )]);
    let (graph, current_snapshot_id) = load_current_graph_with_snapshot_id_for_cli(store).await?;
    let coordinator = Coordinator::with_remote_signer_policy(current_snapshot_id, graph, policy);
    let mut remote_changeset = RemoteChangeSet::sign(canonical, &keypair)
        .map_err(|e| CliError::Domain(format!("remote signing failed: {e}")))?;
    remote_changeset.agent.label = Some(signer_ref.to_string());

    let response = coordinator
        .handle_remote_exchange(RemoteExchangeRequest::SubmitChangeSet(Box::new(
            remote_changeset,
        )))
        .await;
    let RemoteExchangeResponse::Submission(outcome) = response else {
        return Err(CliError::Domain(remote_exchange_error_message(response)));
    };

    let human_msg = remote_submit_human(
        change_id,
        signer_ref,
        &identity.public_key,
        remote_config_source,
        configured_allowed_signers,
        &outcome,
    );
    print_response(
        mode,
        &human_msg,
        json!({
            "change_id": change_id,
            "request": "SubmitChangeSet",
            "transport": REMOTE_SUBMIT_TRANSPORT,
            "key_source": REMOTE_SUBMIT_KEY_SOURCE,
            "signer": {
                "key_ref": signer_ref,
                "public_key": bytes_to_hex(&identity.public_key),
            },
            "remote_config": {
                "path": remote_config_path.display().to_string(),
                "source": remote_config_source,
                "allowed_signers": configured_allowed_signers,
                "applied_to_submit_policy": false,
            },
            "outcome": remote_submission_outcome_json(&outcome),
            "note": REMOTE_SUBMIT_NOTE,
        }),
    );
    Ok(())
}

// ── Private helpers ───────────────────────────────────────────────────────

fn remote_file_bundle_store(store: &StoreHandle) -> Result<(FileBundleStore, PathBuf), CliError> {
    let StoreHandle::File { ail_dir, .. } = store else {
        return Err(CliError::Domain(
            "remote push/pull currently require an initialized file-backed .ail project; no network transport or remote config is used".to_string(),
        ));
    };
    let path = ail_dir.join("remote").join("bundles");
    let bundle_store = FileBundleStore::new(path.clone())
        .map_err(|e| CliError::Domain(format!("remote bundle store failed: {e}")))?;
    Ok((bundle_store, path))
}

fn remote_bundle_scope(bundle: &ObjectBundle) -> &'static str {
    if bundle.includes_snapshot_envelope_dependencies() {
        REMOTE_BUNDLE_SCOPE_SNAPSHOT_DEPENDENCIES
    } else {
        REMOTE_BUNDLE_SCOPE_SINGLE_ROOT
    }
}

async fn build_remote_bundle(
    store: &StoreHandle,
    root: ObjectId,
) -> Result<ObjectBundle, CliError> {
    let StoreHandle::File { objects, .. } = store else {
        return Err(CliError::Domain(
            "remote push currently requires an initialized file-backed .ail project".to_string(),
        ));
    };
    match ObjectBundle::from_store_with_snapshot_dependencies(root, objects).await {
        Ok(bundle) => Ok(bundle),
        Err(StorageError::NotFound) => {
            Err(CliError::NotFound(format!("object root not found: {root}")))
        }
        Err(err) => Err(CliError::Storage(err)),
    }
}

fn remote_exchange_error_message(response: RemoteExchangeResponse) -> String {
    match response {
        RemoteExchangeResponse::Error { code, message } => {
            format!("remote exchange failed ({code}): {message}")
        }
        other => format!("unexpected remote exchange response: {other:?}"),
    }
}

fn remote_submit_human(
    change_id: &str,
    signer_ref: &str,
    public_key: &[u8; 32],
    remote_config_source: &str,
    configured_allowed_signers: usize,
    outcome: &RemoteSubmissionOutcome,
) -> String {
    format!(
        "remote submit: {change_id}\nsigner: {signer_ref}\npublic key: {}\ntransport: {REMOTE_SUBMIT_TRANSPORT}\nkey source: {REMOTE_SUBMIT_KEY_SOURCE}\nremote config: {remote_config_source} ({configured_allowed_signers} allowed signers, not applied to ephemeral submit policy)\noutcome: {}\nnote: {REMOTE_SUBMIT_NOTE}",
        bytes_to_hex(public_key),
        remote_submission_outcome_label(outcome),
    )
}

fn remote_submission_outcome_label(outcome: &RemoteSubmissionOutcome) -> &'static str {
    match outcome {
        RemoteSubmissionOutcome::Applied { .. } => "applied",
        RemoteSubmissionOutcome::RebaseApplied { .. } => "rebase_applied",
        RemoteSubmissionOutcome::ConflictIrresolvable { .. } => "conflict_irresolvable",
        RemoteSubmissionOutcome::StaleBase { .. } => "stale_base",
        RemoteSubmissionOutcome::Failed { .. } => "failed",
    }
}

fn remote_submission_outcome_json(outcome: &RemoteSubmissionOutcome) -> Value {
    match outcome {
        RemoteSubmissionOutcome::Applied {
            applied_snapshot_id,
        } => json!({
            "status": "Applied",
            "applied_snapshot_id": applied_snapshot_id.0,
        }),
        RemoteSubmissionOutcome::RebaseApplied {
            rebased_onto,
            applied_snapshot_id,
        } => json!({
            "status": "RebaseApplied",
            "rebased_onto": rebased_onto.0,
            "applied_snapshot_id": applied_snapshot_id.0,
        }),
        RemoteSubmissionOutcome::ConflictIrresolvable { reason } => json!({
            "status": "ConflictIrresolvable",
            "reason": format!("{reason:?}"),
        }),
        RemoteSubmissionOutcome::StaleBase {
            current_snapshot_id,
        } => json!({
            "status": "StaleBase",
            "current_snapshot_id": current_snapshot_id.0,
        }),
        RemoteSubmissionOutcome::Failed { reason } => json!({
            "status": "Failed",
            "reason": reason,
        }),
    }
}
