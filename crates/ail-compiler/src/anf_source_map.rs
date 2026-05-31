// ── ail-compiler::anf — source map types ──────────────────────────────────
//
// Declared from anf.rs as:
//   #[path = "anf_source_map.rs"]
//   mod anf_source_map;

use ail_core::semantic_graph::{
    BlockRef, ContractRef, EffectRef, NodeRef, ProofObligationRef, RuntimeCheckRef,
    Span as GraphSpan,
};
use serde::{Deserialize, Serialize};

use crate::error::{CompileError, SourceMapDiagnostic};

use super::AnfBinding;

// ── SourceMapSpan ─────────────────────────────────────────────────────────

/// Redacted, deterministic source-map span record.
///
/// `file_id` is an opaque source or generated-artifact identifier. Validation
/// diagnostics never echo it; they report only whether the id is present.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceMapSpan {
    /// Opaque source file, view, or generated artifact identifier.
    pub file_id: String,
    /// Byte offset of the start of the span (inclusive).
    pub start: u64,
    /// Byte offset of the end of the span (exclusive).
    pub end: u64,
}

impl SourceMapSpan {
    pub fn new(file_id: impl Into<String>, start: u64, end: u64) -> Self {
        Self {
            file_id: file_id.into(),
            start,
            end,
        }
    }

    pub fn from_graph_span(span: &GraphSpan) -> Self {
        Self::new(
            span.source.clone(),
            u64::from(span.start),
            u64::from(span.end),
        )
    }
}

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
    /// Authored source span, when the originating graph node carried one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_span: Option<SourceMapSpan>,
    /// Generated artifact span populated by backends when offsets are known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_span: Option<SourceMapSpan>,
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
                source_span: None,
                generated_span: None,
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

    /// Validate source and generated span records for tooling-quality diagnostics.
    ///
    /// The validator is intentionally metadata-focused: source spans are
    /// optional, but any span that exists must have a non-empty file id and a
    /// forward byte range. Generated spans must also be non-overlapping within
    /// each generated artifact. Issues are returned in deterministic order and
    /// descriptors are redacted so raw file ids never leak into diagnostics.
    pub fn validate_tooling_quality(&self) -> Result<(), CompileError> {
        let issues = self.validation_issues();
        if issues.is_empty() {
            Ok(())
        } else {
            Err(CompileError::InvalidSourceMap { issues })
        }
    }

    pub fn validation_issues(&self) -> Vec<SourceMapDiagnostic> {
        let mut issues = Vec::new();

        for (index, entry) in self.entries.iter().enumerate() {
            if let Some(span) = &entry.source_span {
                validate_span_record(&mut issues, index, entry.node_id, span, SpanKind::Source);
            }
            if let Some(span) = &entry.generated_span {
                validate_span_record(&mut issues, index, entry.node_id, span, SpanKind::Generated);
            }
        }

        add_generated_overlap_issues(&mut issues, &self.entries);
        issues.sort_by_key(|issue| (issue.entry_index, issue.code_rank()));
        issues
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

#[derive(Clone, Copy)]
enum SpanKind {
    Source,
    Generated,
}

impl SpanKind {
    fn label(self) -> &'static str {
        match self {
            SpanKind::Source => "source-span",
            SpanKind::Generated => "generated-span",
        }
    }

    fn invalid_range_code(self) -> &'static str {
        match self {
            SpanKind::Source => "AIL-SM-001",
            SpanKind::Generated => "AIL-SM-004",
        }
    }
}

fn validate_span_record(
    issues: &mut Vec<SourceMapDiagnostic>,
    entry_index: usize,
    node_id: NodeRef,
    span: &SourceMapSpan,
    kind: SpanKind,
) {
    if span.file_id.trim().is_empty() {
        issues.push(SourceMapDiagnostic::new(
            "AIL-SM-002",
            "span.file_id",
            entry_index,
            node_id,
            format!("{}:file-id=missing", kind.label()),
        ));
    }

    if span.start >= span.end {
        let shape = if span.start == span.end {
            "empty"
        } else {
            "reversed"
        };
        issues.push(SourceMapDiagnostic::new(
            kind.invalid_range_code(),
            "span.range",
            entry_index,
            node_id,
            format!(
                "{}:file-id={}:range={shape}",
                kind.label(),
                file_id_shape(span),
            ),
        ));
    }
}

fn add_generated_overlap_issues(issues: &mut Vec<SourceMapDiagnostic>, entries: &[SourceMapEntry]) {
    let mut spans: Vec<(usize, NodeRef, &SourceMapSpan)> = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let span = entry.generated_span.as_ref()?;
            if span.file_id.trim().is_empty() || span.start >= span.end {
                return None;
            }
            Some((index, entry.node_id, span))
        })
        .collect();

    spans.sort_by(|(left_index, _, left), (right_index, _, right)| {
        left.file_id
            .cmp(&right.file_id)
            .then(left.start.cmp(&right.start))
            .then(left.end.cmp(&right.end))
            .then(left_index.cmp(right_index))
    });

    let mut previous: Option<(usize, NodeRef, &SourceMapSpan)> = None;
    for current in spans {
        if let Some((_, _, prev_span)) = previous
            && prev_span.file_id == current.2.file_id
            && current.2.start < prev_span.end
        {
            issues.push(SourceMapDiagnostic::new(
                "AIL-SM-003",
                "generated.overlap",
                current.0,
                current.1,
                "generated-span:file-id=present:range=overlap",
            ));
        }
        previous = Some(current);
    }
}

fn file_id_shape(span: &SourceMapSpan) -> &'static str {
    if span.file_id.trim().is_empty() {
        "missing"
    } else {
        "present"
    }
}
