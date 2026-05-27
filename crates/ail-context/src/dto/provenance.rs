use serde::{Deserialize, Serialize};

// ── IndexInfo ─────────────────────────────────────────────────────────────

/// Metadata about a derived index used when building this response.
///
/// Each response lists index versions/hashes used so consumers can detect
/// stale indexes without re-querying.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexInfo {
    /// Index kind (e.g., `"call_graph"`, `"effect_graph"`, `"proof_obligation"`).
    pub kind: String,
    /// Content hash of the index at the time of this response.
    pub hash: [u8; 32],
    /// `true` when the index is behind the current snapshot.
    pub stale: bool,
}

// ── ProvenanceBlock ───────────────────────────────────────────────────────

/// Provenance information attached to every `ContextResponse`.
///
/// Mirrors the `provenance` block in the context-server protocol doc.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProvenanceBlock {
    /// Sources consulted (e.g., `"semantic_graph"`, `"verification_reports"`, `"runtime_profiles"`).
    pub sources: Vec<String>,
    /// Derived indexes used, with their versions/hashes.
    pub indexes: Vec<IndexInfo>,
    /// Verification/audit report hashes incorporated into this response.
    pub reports: Vec<[u8; 32]>,
}

// ── Schema constant ───────────────────────────────────────────────────────

/// Schema version string for `ContextResponse`, stable for the lifetime of
/// this wire-format generation.
pub const CONTEXT_SCHEMA_V1: &str = "context/1.0";
