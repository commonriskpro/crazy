// ── ail-context::freshness ────────────────────────────────────────────────
//
// Freshness detection and repair-option construction.
//
// Responsible for:
// - Resolving `FreshnessStatus` from explicit overrides or snapshot-id comparison.
// - Building the `repair_options` list attached to stale/truncated/index-stale
//   responses.

use ail_storage::object::ObjectId;

use crate::dto::{ContextQuery, FreshnessStatus, RepairOption};

// ── resolve_freshness_status ──────────────────────────────────────────────

/// Determine the freshness status for a context response.
///
/// Priority:
/// 1. If `explicit` is `Some`, use it directly (caller already knows).
/// 2. If `latest_snapshot_id` is `Some`, compare with `snapshot_id`:
///    - equal   → `Fresh`
///    - differs → `Stale`
/// 3. Otherwise → `Fresh` (no staleness information available).
pub(crate) fn resolve_freshness_status(
    explicit: Option<FreshnessStatus>,
    latest_snapshot_id: Option<&ObjectId>,
    snapshot_id: ObjectId,
) -> FreshnessStatus {
    explicit.unwrap_or_else(|| match latest_snapshot_id {
        None => FreshnessStatus::Fresh,
        Some(latest_id) => {
            if *latest_id == snapshot_id {
                FreshnessStatus::Fresh
            } else {
                FreshnessStatus::Stale
            }
        }
    })
}

// ── build_repair_options ──────────────────────────────────────────────────

/// Build the `repair_options` list for a context response.
///
/// Adds:
/// - `"query_latest"` when the response is `Stale` or `Unknown`.
/// - `"narrow_scope"` when the response was truncated by the byte budget.
/// - `"rebuild_index"` when at least one derived index is stale.
pub(crate) fn build_repair_options(
    freshness_status: FreshnessStatus,
    truncated: bool,
    query: &ContextQuery,
    has_stale_index: bool,
) -> Vec<RepairOption> {
    let mut repair_options: Vec<RepairOption> = Vec::new();

    if matches!(
        freshness_status,
        FreshnessStatus::Stale | FreshnessStatus::Unknown
    ) {
        repair_options.push(RepairOption {
            option_id: "query_latest".to_string(),
            description: match freshness_status {
                FreshnessStatus::Stale => "Re-issue the query at the latest snapshot",
                FreshnessStatus::Unknown => {
                    "Retry freshness resolution against the latest snapshot"
                }
                FreshnessStatus::Fresh => unreachable!(),
            }
            .to_string(),
            suggested_query: query
                .target()
                .map(|t| format!("context {:?} snapshot=latest", t)),
        });
    }

    if truncated {
        repair_options.push(RepairOption {
            option_id: "narrow_scope".to_string(),
            description: "Narrow the query scope or increase the budget".to_string(),
            suggested_query: None,
        });
    }

    if has_stale_index {
        repair_options.push(RepairOption {
            option_id: "rebuild_index".to_string(),
            description: "Rebuild stale derived indexes and retry".to_string(),
            suggested_query: None,
        });
    }

    repair_options
}
