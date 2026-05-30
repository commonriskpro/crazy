use super::*;

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
pub(super) fn is_changeset_meta_stage_claim(claim: &str) -> bool {
    claim.starts_with("01-")
        || claim.starts_with("02-")
        || claim.starts_with("03-")
        || claim.starts_with("04-")
        || claim.starts_with("05-")
}

/// Build the JSON repair option for a rebase-required outcome.
pub(super) fn rebase_required_repair_option(current_snapshot_id: u64) -> Value {
    json!({
        "code": "rebase_required",
        "next_action": "rebase",
        "description": "Rebase the ChangeSet onto the current snapshot before apply.",
        "current_snapshot_id": current_snapshot_id,
    })
}
