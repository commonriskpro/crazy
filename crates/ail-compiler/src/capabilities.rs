// ── ail-compiler::capabilities ────────────────────────────────────────────
//
// Shared capability manifest types used by both the native and WASM backends.
//
// # Schema
//
// One `CapabilityEntry` is emitted per `AnfBinding`. The `source_ref` field
// traces each entry back to its originating `SemanticGraph` node, enabling
// runtime capability attribution and offline security audits.
//
// # Determinism
//
// The manifest is built from `AnfIr.bindings` in binding order.  Because ANF
// lowering is deterministic, the manifest is byte-identical for the same input.

use ail_core::semantic_graph::NodeRef;
use serde::{Deserialize, Serialize};

// ── CapabilityEntry ───────────────────────────────────────────────────────

/// One entry in the capability manifest — one per `AnfBinding`.
///
/// Mirrors the native and WASM backend capability manifest schema so that
/// both targets produce interchangeable sidecar manifests.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityEntry {
    /// Binding name, copied from `AnfBinding.name`.
    pub name: String,
    /// Provenance back to the originating `SemanticGraph` node.
    pub source_ref: NodeRef,
}

// ── CapabilitiesManifest ──────────────────────────────────────────────────

/// Side-car capability manifest for compiled artifacts (WASM or native).
///
/// Generated from `AnfIr.bindings` — one `CapabilityEntry` per binding.
/// Serialises to the same JSON schema for both backends, so CLI consumers
/// can parse `capabilities_manifest.entries` uniformly.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitiesManifest {
    /// One entry per `AnfBinding` in source traversal order.
    pub entries: Vec<CapabilityEntry>,
}

impl CapabilitiesManifest {
    /// Build a manifest from a slice of ANF bindings.
    ///
    /// The manifest will have exactly one entry per binding, preserving
    /// binding order and `source_ref` provenance.
    pub fn from_bindings(bindings: &[crate::anf::AnfBinding]) -> Self {
        Self {
            entries: bindings
                .iter()
                .map(|b| CapabilityEntry {
                    name: b.name.clone(),
                    source_ref: b.source_ref,
                })
                .collect(),
        }
    }
}
