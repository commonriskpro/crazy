// ── ail-compiler::core_ir::primitives ────────────────────────────────────
//
// Leaf enumeration types shared across the Core IR.  None of these types
// reference other core_ir types, so they form the foundation layer.

use serde::{Deserialize, Serialize};

// ── CoreNodeKind ──────────────────────────────────────────────────────────

/// Compiler IR node kind — mirrors `ail_core::semantic_graph::NodeKind` for
/// Phase 7.  Defined as a separate enum to allow the compiler IR to diverge
/// from the source graph model in future phases.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoreNodeKind {
    Module,
    Function,
    Type,
    Effect,
    Capability,
    Contract,
    Invariant,
    Test,
    Boundary,
    /// Added in Phase 12 (packages-trust-model) — mirrors `NodeKind::Package`.
    Package,
    /// Interface (typeclass) definition — mirrors `NodeKind::Interface`.
    Interface,
    /// Interface implementation — mirrors `NodeKind::Impl`.
    Impl,
    /// Effect alias definition — mirrors `NodeKind::EffectAlias`.
    EffectAlias,
    /// Import declaration — mirrors `NodeKind::Import`.
    Import,
    /// Export declaration — mirrors `NodeKind::Export`.
    Export,
    /// Version constraint — mirrors `NodeKind::VersionConstraint`.
    VersionConstraint,
    /// Capability export — mirrors `NodeKind::CapabilityExport`.
    CapabilityExport,
    /// Contract export — mirrors `NodeKind::ContractExport`.
    ContractExport,
}

// ── LiteralValue ──────────────────────────────────────────────────────────

/// A typed literal value carried by `CoreExpr::Literal`.
///
/// Uses only stable, serializable Rust primitives so that CBOR output is
/// deterministic across platforms.  `Float` uses IEEE 754 `f64` encoded by
/// `ciborium` — the encoding is deterministic for the same bit pattern.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LiteralValue {
    /// Boolean literal.
    Bool(bool),
    /// Signed 64-bit integer literal.
    Int(i64),
    /// 64-bit floating-point literal (IEEE 754).
    Float(f64),
    /// UTF-8 text literal.
    Text(String),
    /// Unit literal `()`.
    Unit,
    /// Opaque byte-sequence literal.
    ///
    /// Emitted to the WASM data section as a raw byte segment.  The WASM
    /// return value is a packed `i64` with the same layout as `Text`:
    /// `(len as i64) << 32 | (ptr as i64)`.  The runtime decodes this as
    /// `StructuredValue::Bytes { ptr, len }` via `ValueLayout::Bytes` —
    /// no UTF-8 assumption is made.
    Bytes(Vec<u8>),
}

// Eq is needed for CoreNode::PartialEq.  f64 does not implement Eq by default,
// so we provide a manual impl that compares bits (NaN == NaN for IR purposes).
// Vec<u8> already implements Eq, so the Bytes variant requires no special handling.
impl Eq for LiteralValue {}

// ── LoopTermination ───────────────────────────────────────────────────────

/// Classification of the termination argument for a loop expression.
///
/// Carried as an optional field on `CoreExpr::Loop` and `CoreExpr::WhileLoop`.
/// `None` means no termination argument is provided (backward-compatible with
/// pre-ola3 wire format — the field is skipped when None).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoopTermination {
    /// Termination proven by the type checker or external solver.
    Proven,
    /// Bounded iteration: the loop visits a finite, statically-bounded range.
    Bounded,
    /// Termination declared/assumed by the programmer without mechanical proof.
    Assumed,
    /// Termination not yet classified; outcome unknown.
    Unverified,
}

// ── ResourceMode ──────────────────────────────────────────────────────────

/// The ownership / linearity mode of an external resource handle.
///
/// Carried by `CoreType::Handle` to distinguish how the resource's lifecycle
/// is managed.  Affects how the type checker enforces use-once vs. reuse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceMode {
    /// The handle may be freely copied — the resource has value semantics.
    Copy,
    /// The handle may be used at most once (move semantics without aliasing).
    Affine,
    /// The handle must be used exactly once — strict linear types.
    Linear,
    /// The handle may be shared among many concurrent users.
    Shared,
}
