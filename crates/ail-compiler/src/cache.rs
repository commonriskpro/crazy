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
}
