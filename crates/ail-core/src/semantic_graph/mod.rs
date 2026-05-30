// ── ail-core::semantic_graph ──────────────────────────────────────────────
//
// Canonical typed graph representation for the AIL program model.
//
// # Identity contract
//
// `NodeRef(u32)` is the intra-graph identity for nodes within one
// `SemanticGraph`.  It is NOT a storage identity; that role belongs to
// `ail_storage::object::ObjectId`.  A `NodeRef` must never cross the storage
// boundary.
//
// # Determinism contract
//
// All serializable fields use `Vec` or `BTreeMap` — never `HashMap` — to
// guarantee CBOR output determinism with `ciborium`.  Validation helpers may
// build transient `BTreeSet` / `BTreeMap` structures internally, but those
// collections are never part of the serialized layout.
//
// # Module layout
//
// - `types`      — all data type definitions (NodeKind, EdgeKind, GraphNode, …)
// - `validation` — GraphValidationError, DanglingRole, SemanticGraph::validate*
// - `refs`       — typed newtype wrappers (BlockRef, ContractRef, …)

mod refs;
mod types;
mod validation;

// ── Re-exports ────────────────────────────────────────────────────────────
//
// All items below were previously defined directly in this file.
// Re-exporting them here keeps every existing `ail_core::semantic_graph::Foo`
// path valid — downstream crates require zero changes.

pub use refs::{BlockRef, ContractRef, EffectRef, ProofObligationRef, RuntimeCheckRef};

pub use types::{
    Assertion, AssociatedTypeBinding, Binding, CapabilityArgBinding, CapabilityReqs, ConstraintSet,
    ContentHash, ContractClauses, EdgeKind, EffectArgBinding, EffectRow, GeneratedArtifact,
    GenericParamDecl, GenericParamKind, GraphEdge, GraphNode, HandlerMeta, InferredFact,
    InterfaceImplMeta, NodeKind, NodeRef, ParamDecl, Provenance, RefinementRef, RefinementStatus,
    RuntimeCheckMeta, SchemaRef, SemanticGraph, Span, TrustLevel, TrustMetadata, TypeArgBinding,
    TypeFacts, Visibility, WhereConstraint, WorkflowState,
};

pub use validation::{DanglingRole, GraphValidationError};

// ── Tests ─────────────────────────────────────────────────────────────────

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
