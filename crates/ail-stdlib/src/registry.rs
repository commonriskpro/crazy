// ── ail-stdlib::registry ──────────────────────────────────────────────────
//
// Core types for the canonical v1 standard-library registry.
//
// # Determinism contract
//
// `StdlibRegistry` uses `Vec<StdlibEntry>` (insertion order) to guarantee
// deterministic CBOR output with `ciborium`.  The BLAKE3 hash is computed
// over those bytes.  Consumers must never reorder entries and must treat
// the registry as an append-only structure.
//
// # Dependency isolation
//
// This module depends only on `ail-core` (for graph node types), `serde`,
// `ciborium`, and `blake3`.  It MUST NOT import `ail-verify`, `ail-compiler`,
// or `ail-runtime`.

use ail_core::semantic_graph::{
    CapabilityReqs, ContractClauses, EffectRow, GraphNode, NodeKind, NodeRef, TypeFacts,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

// ── StabilityTier ─────────────────────────────────────────────────────────

/// Stability classification for a stdlib entry.
///
/// All five variants are stable API surface; the discriminant order is
/// intentional and must not change once the v1 registry ships.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StabilityTier {
    /// Generally available; breaking changes require a major version bump.
    Stable,
    /// Available but the API may change without a major bump.
    Experimental,
    /// Present for backwards compatibility; callers should migrate away.
    Deprecated,
    /// Requires an explicit opt-in; violates normal safety guarantees.
    Unsafe,
    /// Implementation detail; not part of the public API contract.
    Internal,
}

// ── StdlibId ─────────────────────────────────────────────────────────────

/// Opaque string identifier for a stdlib entry (e.g. `"std.core"`, `"std.option"`).
///
/// Serialized transparently as a plain CBOR text string.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StdlibId(pub String);

// ── StdlibError ───────────────────────────────────────────────────────────

/// Errors produced by `StdlibRegistry` operations.
#[derive(Debug, PartialEq, Eq)]
pub enum StdlibError {
    /// Two entries share the same `StdlibId`.
    DuplicateId(String),
    /// CBOR serialization or deserialization failed.
    SerializationError(String),
}

impl std::fmt::Display for StdlibError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StdlibError::DuplicateId(id) => write!(f, "duplicate stdlib ID: {id}"),
            StdlibError::SerializationError(msg) => write!(f, "serialization error: {msg}"),
        }
    }
}

impl std::error::Error for StdlibError {}

// ── StdlibEntry ───────────────────────────────────────────────────────────

/// A single entry in the stdlib registry.
///
/// Required fields (`id`, `module_path`, `name`, `kind`, `stability`) are
/// always present in the CBOR encoding.  Optional semantic-fact fields
/// (`type_facts`, `effect_row`, `capability_reqs`, `contract_clauses`) are
/// omitted from the encoding when absent, keeping the wire format compact and
/// backward-compatible with Phase 1–5 CBOR byte sequences.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StdlibEntry {
    /// Stable dot-separated identifier, e.g. `StdlibId("std.core")`.
    pub id: StdlibId,
    /// Rust-style module path, e.g. `"std::core"`.
    pub module_path: String,
    /// Short display name, e.g. `"core"`.
    pub name: String,
    /// Semantic category of this entry.
    pub kind: NodeKind,
    /// Stability classification.
    pub stability: StabilityTier,
    /// Resolved type information, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_facts: Option<TypeFacts>,
    /// Declared effect row, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect_row: Option<EffectRow>,
    /// Declared capability requirements, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_reqs: Option<CapabilityReqs>,
    /// Contract clauses (requires/ensures), if declared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_clauses: Option<ContractClauses>,
}

// ── StdlibRegistry ────────────────────────────────────────────────────────

/// Ordered collection of `StdlibEntry` values forming the canonical stdlib.
///
/// Entry order is significant: it determines `NodeRef` assignment during
/// projection and the byte layout of the CBOR-encoded hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StdlibRegistry {
    /// Entries in declaration (insertion) order.
    pub entries: Vec<StdlibEntry>,
}

impl StdlibRegistry {
    // ── validate ─────────────────────────────────────────────────────────

    /// Check that all `StdlibId` values are unique.
    ///
    /// Returns `Ok(())` when every entry has a distinct ID; returns
    /// `Err(StdlibError::DuplicateId(…))` on the first duplicate found.
    pub fn validate(&self) -> Result<(), StdlibError> {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for entry in &self.entries {
            if !seen.insert(entry.id.0.as_str()) {
                return Err(StdlibError::DuplicateId(entry.id.0.clone()));
            }
        }
        Ok(())
    }

    // ── cbor_bytes ────────────────────────────────────────────────────────

    /// Serialize this registry to deterministic CBOR bytes.
    ///
    /// Uses `ciborium`; all `Vec`-based fields guarantee byte-identical
    /// output for the same input (no `HashMap` nondeterminism).
    pub fn cbor_bytes(&self) -> Result<Vec<u8>, StdlibError> {
        let mut buf = Vec::new();
        ciborium::ser::into_writer(self, &mut buf)
            .map_err(|e| StdlibError::SerializationError(e.to_string()))?;
        Ok(buf)
    }

    // ── from_cbor_bytes ───────────────────────────────────────────────────

    /// Deserialize a registry from CBOR bytes produced by `cbor_bytes()`.
    pub fn from_cbor_bytes(bytes: &[u8]) -> Result<Self, StdlibError> {
        ciborium::de::from_reader(bytes).map_err(|e| StdlibError::SerializationError(e.to_string()))
    }

    // ── hash ──────────────────────────────────────────────────────────────

    /// Compute a stable `[u8; 32]` BLAKE3 hash of the CBOR encoding.
    ///
    /// The returned bytes are the raw hash.  Callers that need a hex digest
    /// for display or storage should convert at their own boundary; this
    /// method never performs hex conversion internally.
    pub fn hash(&self) -> Result<[u8; 32], StdlibError> {
        let bytes = self.cbor_bytes()?;
        Ok(*blake3::hash(&bytes).as_bytes())
    }

    // ── to_graph_nodes ────────────────────────────────────────────────────

    /// Project all entries into `Vec<GraphNode>` for insertion into a
    /// `SemanticGraph`.
    ///
    /// # Contracts
    ///
    /// * `NodeRef` is assigned as `NodeRef(index as u32)` — deterministic and
    ///   unique within the returned slice.
    /// * `GraphNode::name` is `entry.id.0` (the full dot-separated stdlib ID).
    /// * `kind`, `type_facts`, `effect_row`, `capability_reqs`, and
    ///   `contract_clauses` are propagated verbatim from the entry.
    /// * `runtime_checks` is always `None` — that field belongs to the
    ///   compiler/verify layer.
    ///
    /// Inserting the returned nodes into an otherwise-empty `SemanticGraph`
    /// (no edges) is guaranteed to pass `SemanticGraph::validate()`.
    pub fn to_graph_nodes(&self) -> Vec<GraphNode> {
        self.entries
            .iter()
            .enumerate()
            .map(|(index, entry)| GraphNode {
                id: NodeRef(index as u32),
                kind: entry.kind,
                name: entry.id.0.clone(),
                type_facts: entry.type_facts.clone(),
                effect_row: entry.effect_row.clone(),
                capability_reqs: entry.capability_reqs.clone(),
                contract_clauses: entry.contract_clauses.clone(),
                runtime_checks: None,
                content_hash: None,
                provenance: None,
                schema: None,
                trust_metadata: None,
            })
            .collect()
    }
}
