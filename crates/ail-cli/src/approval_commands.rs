// ── ail-cli::approval_commands ────────────────────────────────────────────
//
// Handlers for `ail approve` and `ail reject`.
//
// Both commands produce immutable approval decision records that reference
// the canonical change hash.  Records are persisted to `.ail/approvals/`
// when a file store is active; silently no-op for in-memory / Postgres stores.
//
// Rules:
// - Approval references `canonical_change_hash` (change-id IS the hash).
// - Approval expires if the canonical diff changes.
// - Records are immutable once written.

use serde_json::json;

use crate::cli::{ail_dir_for_store, bytes_to_hex, is_valid_change_id, unix_ms_now};
use crate::error::CliError;
use crate::output::{OutputMode, print_response};
use crate::store::StoreHandle;

// ── Private types and helpers ─────────────────────────────────────────────

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct ApprovalDecisionRecord {
    record_id: String,
    change_id: String,
    canonical_hash: String,
    decision: String,
    reason: String,
    role: Option<String>,
    created_at: u64,
}

fn save_approval_record(
    store: &StoreHandle,
    record: &ApprovalDecisionRecord,
) -> Result<(), CliError> {
    if !matches!(store, StoreHandle::File { .. }) {
        let _ = record;
        return Ok(());
    }
    let dir = ail_dir_for_store(store)?.join("approvals");
    std::fs::create_dir_all(&dir)?;
    let mut bytes = Vec::new();
    ciborium::into_writer(record, &mut bytes)
        .map_err(|e| CliError::Domain(format!("approval encoding failed: {e}")))?;
    std::fs::write(dir.join(format!("{}.cbor", record.record_id)), bytes)?;
    Ok(())
}

// ── Command handlers ──────────────────────────────────────────────────────

/// `ail approve <change-id> [--for <reason>] [--role <role>]`
///
/// Rules:
/// - approval references canonical_change_hash
/// - approval expires if canonical diff changes
/// - approval records are immutable
pub(crate) fn cmd_approve(
    mode: OutputMode,
    change_id: &str,
    for_reason: Option<&str>,
    role: Option<&str>,
    store: &StoreHandle,
) -> Result<(), CliError> {
    if !is_valid_change_id(change_id) {
        return Err(CliError::NotFound(format!(
            "change-id not found: {change_id}"
        )));
    }

    let reason = for_reason.unwrap_or("(unspecified)");
    let approver_role = role.unwrap_or("owner");
    let canonical_hash = change_id; // The change-id IS the canonical hash.
    let record_id = {
        let hash = blake3::hash(format!("approve:{change_id}:{reason}:{approver_role}").as_bytes());
        bytes_to_hex(hash.as_bytes())
    };
    let record = ApprovalDecisionRecord {
        record_id: record_id.clone(),
        change_id: change_id.to_string(),
        canonical_hash: canonical_hash.to_string(),
        decision: "approved".to_string(),
        reason: reason.to_string(),
        role: Some(approver_role.to_string()),
        created_at: unix_ms_now(),
    };
    save_approval_record(store, &record)?;

    let human_msg = format!(
        "approved {change_id}\nfor: {reason}\nrole: {approver_role}\nrecord_id: {record_id}\nimmutable: true\nexpires_on_diff_change: true"
    );
    print_response(
        mode,
        &human_msg,
        json!({
            "approved": true,
            "change_id": change_id,
            "canonical_hash": canonical_hash,
            "for": reason,
            "role": approver_role,
            "record_id": record_id,
            "immutable": true,
            "expires_on_canonical_diff_change": true,
        }),
    );
    Ok(())
}

/// `ail reject <change-id> --reason <text>`
///
/// Rules:
/// - rejection records are immutable
/// - approval expires if canonical diff changes
pub(crate) fn cmd_reject(
    mode: OutputMode,
    change_id: &str,
    reason: &str,
    store: &StoreHandle,
) -> Result<(), CliError> {
    if !is_valid_change_id(change_id) {
        return Err(CliError::NotFound(format!(
            "change-id not found: {change_id}"
        )));
    }

    let record_id = {
        let hash = blake3::hash(format!("reject:{change_id}:{reason}").as_bytes());
        bytes_to_hex(hash.as_bytes())
    };
    let record = ApprovalDecisionRecord {
        record_id: record_id.clone(),
        change_id: change_id.to_string(),
        canonical_hash: change_id.to_string(),
        decision: "rejected".to_string(),
        reason: reason.to_string(),
        role: None,
        created_at: unix_ms_now(),
    };
    save_approval_record(store, &record)?;

    let human_msg =
        format!("rejected {change_id}\nreason: {reason}\nrecord_id: {record_id}\nimmutable: true");
    print_response(
        mode,
        &human_msg,
        json!({
            "approved": false,
            "change_id": change_id,
            "reason": reason,
            "record_id": record_id,
            "immutable": true,
        }),
    );
    Ok(())
}
