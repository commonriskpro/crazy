// Integration tests for retention policies, GC, and snapshot compaction.
//
// All tests use `MemoryObjectStore` (deterministic, no DB needed).
// Time is injected as `now_ms` so tests are deterministic and instant.
//
// Test layout
// ──────────────────────────────────────────────────────────────────────────
// RetentionPolicy::is_retained
//   retention_keeps_genesis_when_keep_releases_true
//   retention_releases_genesis_when_keep_releases_false
//   retention_keeps_tagged_when_keep_tagged_true
//   retention_releases_tagged_when_keep_tagged_false
//   retention_keeps_young_snapshot_within_max_age
//   retention_removes_old_snapshot_beyond_max_age
//   retention_none_max_age_does_not_protect_by_age
//
// gc_unreferenced
//   gc_empty_store_produces_zero_report
//   gc_retains_all_when_policy_keeps_all
//   gc_removes_all_when_policy_keeps_nothing
//   gc_partial_retention_removes_only_unreferenced
//   gc_report_counts_are_consistent
//
// compact_snapshots
//   compact_empty_range_returns_error
//   compact_out_of_bounds_returns_error
//   compact_single_snapshot_range
//   compact_multiple_snapshots_produces_covering
//   compact_covering_has_correct_graph_root_hash
//   compact_covering_has_correct_parent_id
//   compact_merged_count_matches_range
//   compact_originals_are_removed

#[path = "retention_gc/compaction.rs"]
mod compaction;
#[path = "retention_gc/gc.rs"]
mod gc;
#[path = "retention_gc/helpers.rs"]
mod helpers;
#[path = "retention_gc/holds.rs"]
mod holds;
#[path = "retention_gc/retention.rs"]
mod retention;
