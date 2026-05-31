// ── ail-compiler::cache ───────────────────────────────────────────────────
//
// Content-addressed artifact cache for compiled pipeline outputs.
//
// # Design
//
// `ArtifactCache` is a trait (object-safe) so callers can swap backends
// (memory, disk, remote) without changing the incremental pipeline logic.
//
// `MemoryArtifactCache` is the Phase 15 reference implementation:
// `HashMap<[u8; 32], ArtifactEntry>` behind an `Arc<Mutex<…>>` so it can be
// shared across threads.  The `Arc` wrapping matches the workspace pattern
// used by `ObjectBackedGraphStore` (`Arc<Mutex<HashMap<…>>>`).
//
// # Key contract
//
// Cache keys are 32-byte BLAKE3 content hashes of the serialised `GraphNode`
// (not its `NodeRef` position).  Content-addressed keys are resilient to node
// reordering across snapshots.
//
// # Thread safety
//
// `MemoryArtifactCache` implements `Send + Sync`; `Mutex` ensures interior
// mutability without data races.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::core_ir::StageHashes;

// ── Incremental cache validation ─────────────────────────────────────────

/// Current schema identifier for incremental cache sidecar entries.
pub const INCREMENTAL_CACHE_SCHEMA_VERSION: &str = "incremental-cache/1.0";

/// Stable issue code for cache entries written by an unsupported schema.
pub const E_INCREMENTAL_CACHE_STALE_SCHEMA: &str = "E_INCREMENTAL_CACHE_STALE_SCHEMA";

/// Stable issue code for cache entries whose provenance hash does not match
/// the graph snapshot they are being reused for.
pub const E_INCREMENTAL_CACHE_HASH_MISMATCH: &str = "E_INCREMENTAL_CACHE_HASH_MISMATCH";

/// Stable issue code for persisted cache indexes containing the same key more
/// than once.
pub const E_INCREMENTAL_CACHE_DUPLICATE_KEY: &str = "E_INCREMENTAL_CACHE_DUPLICATE_KEY";

/// One persisted cache-index entry plus the validation context needed before
/// reusing it for an incremental compile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IncrementalCacheValidationEntry<'a> {
    /// Content-addressed cache key for the lowered graph node.
    pub key: [u8; 32],
    /// Schema id recorded by the cache sidecar.
    pub schema_version: &'a str,
    /// Graph snapshot hash expected for this cache reuse decision.
    pub expected_graph_snapshot_hash: [u8; 32],
    /// Cached artifact entry being validated.
    pub entry: &'a ArtifactEntry,
}

impl<'a> IncrementalCacheValidationEntry<'a> {
    /// Build a validation entry for one persisted incremental-cache record.
    pub fn new(
        key: [u8; 32],
        schema_version: &'a str,
        expected_graph_snapshot_hash: [u8; 32],
        entry: &'a ArtifactEntry,
    ) -> Self {
        Self {
            key,
            schema_version,
            expected_graph_snapshot_hash,
            entry,
        }
    }
}

/// Machine-readable incremental cache validation issue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncrementalCacheValidationIssue {
    /// Stable issue code for build gates and cache pruning tools.
    pub code: String,
    /// Hex-encoded content-addressed cache key.
    pub key: String,
    /// Cache sidecar or artifact field that failed validation.
    pub field: String,
    /// Human-readable explanation for logs and reports.
    pub message: String,
}

impl IncrementalCacheValidationIssue {
    fn new(
        code: &'static str,
        key: [u8; 32],
        field: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.to_string(),
            key: cache_key_hex(&key),
            field: field.to_string(),
            message: message.into(),
        }
    }

    fn sort_key(&self) -> (&str, &str, &str, &str) {
        (&self.code, &self.key, &self.field, &self.message)
    }
}

/// Validate persisted incremental-cache entries before reuse.
///
/// This gate catches production cache-index drift that an in-memory `HashMap`
/// cannot represent directly: stale schema sidecars, provenance hash mismatch
/// against the current graph snapshot, and duplicate persisted keys.  Returned
/// issues are sorted by stable machine fields so invalidation/pruning order does
/// not depend on filesystem or manifest traversal order.
pub fn validate_incremental_cache_entries(
    entries: &[IncrementalCacheValidationEntry<'_>],
) -> Vec<IncrementalCacheValidationIssue> {
    use std::collections::BTreeMap;

    let mut issues = Vec::new();
    let mut key_counts: BTreeMap<[u8; 32], usize> = BTreeMap::new();

    for entry in entries {
        *key_counts.entry(entry.key).or_default() += 1;

        if entry.schema_version != INCREMENTAL_CACHE_SCHEMA_VERSION {
            issues.push(IncrementalCacheValidationIssue::new(
                E_INCREMENTAL_CACHE_STALE_SCHEMA,
                entry.key,
                "schema_version",
                format!(
                    "expected schema {INCREMENTAL_CACHE_SCHEMA_VERSION}, found {}",
                    entry.schema_version
                ),
            ));
        }

        if entry.entry.stage_hashes.graph_snapshot_hash != entry.expected_graph_snapshot_hash {
            issues.push(IncrementalCacheValidationIssue::new(
                E_INCREMENTAL_CACHE_HASH_MISMATCH,
                entry.key,
                "stage_hashes.graph_snapshot_hash",
                "cached graph snapshot hash does not match current graph snapshot",
            ));
        }
    }

    for (key, count) in key_counts {
        if count > 1 {
            issues.push(IncrementalCacheValidationIssue::new(
                E_INCREMENTAL_CACHE_DUPLICATE_KEY,
                key,
                "cache_key",
                format!("cache key appears {count} times in persisted index"),
            ));
        }
    }

    issues.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    issues
}

fn cache_key_hex(key: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in key {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

// ── ArtifactEntry ─────────────────────────────────────────────────────────

/// A cached output for one compiled `GraphNode`.
///
/// Carries the pipeline's `StageHashes` accumulator (for provenance
/// verification by callers) and the count of nodes lowered in the same batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactEntry {
    /// BLAKE3 hash chain accumulated through the pipeline stage that produced
    /// this entry.
    pub stage_hashes: StageHashes,
    /// Number of nodes lowered in the compilation batch that produced this
    /// entry (for provenance; not used for cache keying).
    pub node_count: usize,
}

// ── ArtifactCache ─────────────────────────────────────────────────────────

/// Content-addressed cache for compiled pipeline artifacts.
///
/// Implementations MUST be `Send + Sync` so the cache can be shared across
/// the incremental compilation pipeline, which may iterate nodes concurrently
/// in future phases.
///
/// # Key semantics
///
/// - `get` returns `None` on a cache miss (key not present).
/// - `put` with an existing key overwrites the prior entry (latest write wins).
pub trait ArtifactCache: Send + Sync {
    /// Retrieve the entry for `key`, or `None` on a miss.
    fn get(&self, key: &[u8; 32]) -> Option<ArtifactEntry>;

    /// Store `entry` under `key`, overwriting any prior value.
    fn put(&self, key: [u8; 32], entry: ArtifactEntry);
}

// ── MemoryArtifactCache ───────────────────────────────────────────────────

/// In-memory `ArtifactCache` backed by a `HashMap` behind an `Arc<Mutex<…>>`.
///
/// Cloning this struct is cheap (clones the `Arc` only); all clones share the
/// same underlying map — consistent with the workspace pattern for shared
/// in-memory stores.
#[derive(Clone, Default)]
pub struct MemoryArtifactCache(Arc<Mutex<HashMap<[u8; 32], ArtifactEntry>>>);

impl MemoryArtifactCache {
    /// Create a new, empty `MemoryArtifactCache`.
    pub fn new() -> Self {
        Self::default()
    }
}

impl ArtifactCache for MemoryArtifactCache {
    fn get(&self, key: &[u8; 32]) -> Option<ArtifactEntry> {
        self.0
            .lock()
            .expect("MemoryArtifactCache mutex poisoned")
            .get(key)
            .cloned()
    }

    fn put(&self, key: [u8; 32], entry: ArtifactEntry) {
        self.0
            .lock()
            .expect("MemoryArtifactCache mutex poisoned")
            .insert(key, entry);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core_ir::StageHashes;

    fn make_stage_hashes(seed: u8) -> StageHashes {
        StageHashes {
            graph_snapshot_hash: [seed; 32],
            verification_report_hash: [seed + 1; 32],
            core_ir_hash: [seed + 2; 32],
            anf_ir_hash: None,
            wasm_hash: None,
            native_hash: None,
            source_map_hash: None,
            artifact_manifest_hash: None,
        }
    }

    fn make_entry(seed: u8, node_count: usize) -> ArtifactEntry {
        ArtifactEntry {
            stage_hashes: make_stage_hashes(seed),
            node_count,
        }
    }

    // ── Spec scenario: Cache hit after put ────────────────────────────────
    // GIVEN an ArtifactCache with no prior entries
    // WHEN put(key, entry) is called followed by get(key)
    // THEN get returns Some(entry) equal to the stored value
    #[test]
    fn cache_hit_after_put() {
        let cache = MemoryArtifactCache::new();
        let key = [1u8; 32];
        let entry = make_entry(10, 5);

        cache.put(key, entry.clone());
        let result = cache.get(&key);

        assert_eq!(result, Some(entry), "get must return the stored entry");
    }

    // ── Spec scenario: Cache miss for unknown key ─────────────────────────
    // GIVEN an ArtifactCache with no prior entries
    // WHEN get(unknown_key) is called
    // THEN get returns None
    #[test]
    fn cache_miss_for_unknown_key() {
        let cache = MemoryArtifactCache::new();
        let unknown_key = [99u8; 32];

        assert_eq!(
            cache.get(&unknown_key),
            None,
            "unknown key must return None"
        );
    }

    // ── Spec scenario: Overwrite with same key ────────────────────────────
    // GIVEN an ArtifactCache containing key → entry_a
    // WHEN put(key, entry_b) is called with the same key
    // THEN get(key) returns Some(entry_b) (latest write wins)
    #[test]
    fn overwrite_with_same_key_returns_latest() {
        let cache = MemoryArtifactCache::new();
        let key = [2u8; 32];
        let entry_a = make_entry(10, 1);
        let entry_b = make_entry(20, 2);

        cache.put(key, entry_a);
        cache.put(key, entry_b.clone());

        assert_eq!(
            cache.get(&key),
            Some(entry_b),
            "latest write must win on overwrite"
        );
    }

    // ── Spec scenario: Entry preserves stage hashes and node count ────────
    // GIVEN an ArtifactEntry constructed with stage_hashes and node_count = 5
    // WHEN the entry is retrieved from the cache
    // THEN entry.stage_hashes equals the original and entry.node_count == 5
    #[test]
    fn entry_preserves_stage_hashes_and_node_count() {
        let cache = MemoryArtifactCache::new();
        let key = [3u8; 32];
        let hashes = make_stage_hashes(42);
        let entry = ArtifactEntry {
            stage_hashes: hashes.clone(),
            node_count: 5,
        };

        cache.put(key, entry);
        let retrieved = cache.get(&key).expect("must be a hit");

        assert_eq!(retrieved.stage_hashes, hashes);
        assert_eq!(retrieved.node_count, 5);
    }

    // ── TRIANGULATE: clone shares underlying map ──────────────────────────
    #[test]
    fn clone_shares_underlying_map() {
        let cache_a = MemoryArtifactCache::new();
        let cache_b = cache_a.clone();
        let key = [7u8; 32];
        let entry = make_entry(7, 3);

        cache_a.put(key, entry.clone());
        assert_eq!(
            cache_b.get(&key),
            Some(entry),
            "clone must share the same backing map"
        );
    }

    #[test]
    fn validation_reports_stale_schema_hash_mismatch_and_duplicate_keys() {
        let key = [0xabu8; 32];
        let expected_graph_snapshot_hash = [1u8; 32];
        let stale_entry = make_entry(9, 1);

        let issues = validate_incremental_cache_entries(&[
            IncrementalCacheValidationEntry::new(
                key,
                "incremental-cache/0.9",
                expected_graph_snapshot_hash,
                &stale_entry,
            ),
            IncrementalCacheValidationEntry::new(
                key,
                INCREMENTAL_CACHE_SCHEMA_VERSION,
                expected_graph_snapshot_hash,
                &make_entry(1, 1),
            ),
        ]);

        let issue_keys: Vec<(&str, &str)> = issues
            .iter()
            .map(|issue| (issue.code.as_str(), issue.field.as_str()))
            .collect();

        assert_eq!(
            issue_keys,
            vec![
                (E_INCREMENTAL_CACHE_DUPLICATE_KEY, "cache_key"),
                (
                    E_INCREMENTAL_CACHE_HASH_MISMATCH,
                    "stage_hashes.graph_snapshot_hash",
                ),
                (E_INCREMENTAL_CACHE_STALE_SCHEMA, "schema_version"),
            ]
        );
    }

    #[test]
    fn validation_orders_issues_deterministically() {
        let expected_graph_snapshot_hash = [7u8; 32];
        let alpha = [1u8; 32];
        let zeta = [9u8; 32];
        let bad_alpha = make_entry(2, 1);
        let bad_zeta = make_entry(3, 1);

        let issues = validate_incremental_cache_entries(&[
            IncrementalCacheValidationEntry::new(
                zeta,
                "incremental-cache/0.8",
                expected_graph_snapshot_hash,
                &bad_zeta,
            ),
            IncrementalCacheValidationEntry::new(
                alpha,
                INCREMENTAL_CACHE_SCHEMA_VERSION,
                expected_graph_snapshot_hash,
                &bad_alpha,
            ),
            IncrementalCacheValidationEntry::new(
                alpha,
                "incremental-cache/0.7",
                expected_graph_snapshot_hash,
                &bad_alpha,
            ),
        ]);
        let reversed_issues = validate_incremental_cache_entries(&[
            IncrementalCacheValidationEntry::new(
                alpha,
                "incremental-cache/0.7",
                expected_graph_snapshot_hash,
                &bad_alpha,
            ),
            IncrementalCacheValidationEntry::new(
                alpha,
                INCREMENTAL_CACHE_SCHEMA_VERSION,
                expected_graph_snapshot_hash,
                &bad_alpha,
            ),
            IncrementalCacheValidationEntry::new(
                zeta,
                "incremental-cache/0.8",
                expected_graph_snapshot_hash,
                &bad_zeta,
            ),
        ]);

        let sorted_issue_keys = issues
            .windows(2)
            .all(|pair| pair[0].sort_key() <= pair[1].sort_key());

        assert!(
            sorted_issue_keys,
            "incremental cache validation issues must be sorted: {issues:?}"
        );
        assert_eq!(
            issues, reversed_issues,
            "validation order must not depend on cache index traversal order"
        );
    }
}
