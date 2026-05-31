// ── ail-compiler::anf — source map types ──────────────────────────────────
//
// Declared from anf.rs as:
//   #[path = "anf_source_map.rs"]
//   mod anf_source_map;

use ail_core::semantic_graph::{
    BlockRef, ContractRef, EffectRef, NodeRef, ProofObligationRef, RuntimeCheckRef,
};
use serde::{Deserialize, Serialize};

use crate::error::CompileError;

use super::AnfBinding;

// ── SourceMapEntry ────────────────────────────────────────────────────────

/// One entry in the semantic source map — maps an ANF node back to its
/// origin in the semantic graph with full provenance.
///
/// Corresponds to the `semantic_source_map` fields in `docs/compiler.md §
/// Semantic source maps`.
///
/// `wasm_offset` and `native_offset` are filled in by the backend stage;
/// they are `None` in the ANF IR before backend emission.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceMapEntry {
    /// ANF binding name this entry refers to.
    pub binding_name: String,
    /// The `NodeRef` this binding was lowered from — from the `SemanticGraph`.
    pub node_id: NodeRef,
    /// The `BlockRef` (block identity) in the semantic graph, if applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_ref: Option<BlockRef>,
    /// The `ChangeSet` provenance identifier (opaque string).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_set: Option<String>,
    /// The `ContractRef` for the contract that governs this node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_ref: Option<ContractRef>,
    /// The `EffectRef` for the effect associated with this node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect_ref: Option<EffectRef>,
    /// The `ProofObligationRef` for the proof obligation at this node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_obligation_ref: Option<ProofObligationRef>,
    /// The `RuntimeCheckRef` for any runtime check inserted at this node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_check_ref: Option<RuntimeCheckRef>,
    /// Byte offset in the emitted WASM binary (code section), if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wasm_offset: Option<u32>,
    /// Byte offset in the emitted native binary (code section), if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_offset: Option<u64>,
}

/// Semantic source map for an `AnfIr`.
///
/// Maps ANF nodes back to their origin in the semantic graph.  Backends
/// populate `wasm_offset` / `native_offset` as they emit code.
///
/// Preserved through every pipeline stage — SSA, WASM, native — per the
/// compiler.md rules ("Every lowering preserves provenance/source maps").
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceMap {
    /// One entry per ANF binding, in binding order.
    pub entries: Vec<SourceMapEntry>,
}

impl SourceMap {
    /// Build a `SourceMap` from an `AnfIr`'s bindings.
    ///
    /// Each binding contributes one entry with `node_id` set to
    /// `binding.source_ref`.  All optional provenance fields are `None`
    /// at ANF stage; backends fill in offsets later.
    pub fn from_bindings(bindings: &[AnfBinding]) -> Self {
        let entries = bindings
            .iter()
            .map(|b| SourceMapEntry {
                binding_name: b.name.clone(),
                node_id: b.source_ref,
                block_ref: None,
                change_set: None,
                contract_ref: None,
                effect_ref: None,
                proof_obligation_ref: None,
                runtime_check_ref: None,
                wasm_offset: None,
                native_offset: None,
            })
            .collect();
        SourceMap { entries }
    }

    /// Return all entries lowered from `node_id` in stable source-map order.
    ///
    /// A single semantic node can lower into multiple ANF bindings (for
    /// example synthetic temporaries). Diagnostics must keep every matching
    /// span instead of collapsing duplicates, and callers get the same order as
    /// `entries` so repeated report generation is deterministic.
    pub fn entries_for_node(&self, node_id: NodeRef) -> Vec<&SourceMapEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.node_id == node_id)
            .collect()
    }

    /// Validate audit provenance required by production-like compiler profiles.
    ///
    /// The current implemented policy is intentionally small: `prod`,
    /// `production`, and `critical` artifacts must retain the originating
    /// `change_set` for every emitted binding. Other semantic references are
    /// optional because not every graph node has a contract, effect, or runtime
    /// check. The source map must also cover every binding exactly once in
    /// binding order so malformed external ANF cannot hide missing provenance.
    pub fn validate_required_provenance(
        &self,
        profile: &str,
        bindings: &[AnfBinding],
    ) -> Result<(), CompileError> {
        if !matches!(profile, "prod" | "production" | "critical") {
            return Ok(());
        }

        if self.entries.len() != bindings.len() {
            let binding = bindings.get(self.entries.len()).or_else(|| bindings.last());
            return Err(CompileError::MissingProvenanceMetadata {
                profile: profile.to_string(),
                binding_name: binding
                    .map(|binding| binding.name.clone())
                    .unwrap_or_else(|| "<extra-source-map-entry>".to_string()),
                node_id: binding
                    .map(|binding| binding.source_ref)
                    .unwrap_or(NodeRef(0)),
                field: "source_map_coverage",
            });
        }

        for (entry, binding) in self.entries.iter().zip(bindings.iter()) {
            if entry.binding_name != binding.name {
                return Err(CompileError::MissingProvenanceMetadata {
                    profile: profile.to_string(),
                    binding_name: binding.name.clone(),
                    node_id: binding.source_ref,
                    field: "binding_name",
                });
            }

            if entry.node_id != binding.source_ref {
                return Err(CompileError::MissingProvenanceMetadata {
                    profile: profile.to_string(),
                    binding_name: binding.name.clone(),
                    node_id: binding.source_ref,
                    field: "node_id",
                });
            }

            if entry.change_set.as_deref().is_none_or(str::is_empty) {
                return Err(CompileError::MissingProvenanceMetadata {
                    profile: profile.to_string(),
                    binding_name: entry.binding_name.clone(),
                    node_id: entry.node_id,
                    field: "change_set",
                });
            }
        }

        Ok(())
    }
}
