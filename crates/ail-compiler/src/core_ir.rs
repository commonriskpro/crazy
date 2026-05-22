// ── ail-compiler::core_ir ─────────────────────────────────────────────────
//
// Core IR value types — the first lowering stage output.
//
// # Design constraints
//
// - `Vec` and `BTreeMap` only (no `HashMap`) — workspace determinism contract.
// - All types `#[derive(Serialize, Deserialize)]` for CBOR hash sealing and
//   round-trip determinism.
// - `CoreIr` owns the `StageHashes` accumulator so later stages can read
//   predecessor hashes without re-computing them.
//
// # G2 scope (core-ir-full)
//
// Extends `CoreNode` with two optional fields:
// - `ty: Option<CoreType>` — the ~20 type primitives from `docs/core-ir.md §3`.
// - `expr: Option<CoreExpr>` — the ~15 expression primitives from §5.
//
// Both fields use `#[serde(default, skip_serializing_if = "Option::is_none")]`
// to keep pre-G2 CBOR wire format byte-identical when the fields are absent.

use ail_core::semantic_graph::NodeRef;
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
}

// Eq is needed for CoreNode::PartialEq.  f64 does not implement Eq by default,
// so we provide a manual impl that compares bits (NaN == NaN for IR purposes).
impl Eq for LiteralValue {}

// ── MatchArm ──────────────────────────────────────────────────────────────

/// One arm of a `CoreExpr::Match` expression.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchArm {
    /// Pattern string (e.g. `"Ok(x)"`, `"None"`, `"_"`).
    pub pattern: String,
    /// Body expression evaluated when the pattern matches.
    pub body: CoreExpr,
}

// ── SelectClause ──────────────────────────────────────────────────────────

/// One arm of a `CoreExpr::Select` expression.
///
/// Represents a channel-receive case; whichever channel becomes ready first
/// wins and its `body` is evaluated with `binding` in scope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectClause {
    /// Channel expression to receive from.
    ///
    /// Must be atomic (a `Var`) after ANF lowering.
    pub channel: Box<CoreExpr>,
    /// Name to bind the received value to within `body`.
    pub binding: String,
    /// Body expression evaluated when this arm wins the race.
    pub body: CoreExpr,
}

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

// ── CoreExpr ──────────────────────────────────────────────────────────────

/// Pure expression primitives of the Semantic Core IR.
///
/// Corresponds to `docs/core-ir.md §5 — Expresiones puras`.
///
/// All variants must be serializable for CBOR determinism.  Recursive
/// sub-expressions use `Box<CoreExpr>` to keep the type `Sized`.
/// Collections use `Vec` (never `HashMap`) per the workspace determinism
/// contract.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoreExpr {
    /// A typed constant value.
    Literal(LiteralValue),
    /// A reference to a local variable by name.
    Var(String),
    /// An immutable let-binding: `let <name> = <value> in <body>`.
    Let {
        name: String,
        value: Box<CoreExpr>,
        body: Box<CoreExpr>,
    },
    /// A boolean branch: `if <cond> then <then_> else <else_>`.
    If {
        cond: Box<CoreExpr>,
        then_: Box<CoreExpr>,
        else_: Box<CoreExpr>,
    },
    /// Pattern matching over a variant (or any scrutinee).
    Match {
        scrutinee: Box<CoreExpr>,
        arms: Vec<MatchArm>,
    },
    /// A call to a named function or capability.
    Call { func: String, args: Vec<CoreExpr> },
    /// Integer addition.
    Add(Box<CoreExpr>, Box<CoreExpr>),
    /// Integer subtraction.
    Sub(Box<CoreExpr>, Box<CoreExpr>),
    /// Integer multiplication.
    Mul(Box<CoreExpr>, Box<CoreExpr>),
    /// Signed integer division.
    Div(Box<CoreExpr>, Box<CoreExpr>),
    /// Signed integer remainder.
    Mod(Box<CoreExpr>, Box<CoreExpr>),
    /// Integer equality comparison.
    Eq(Box<CoreExpr>, Box<CoreExpr>),
    /// Signed integer less-than comparison.
    Lt(Box<CoreExpr>, Box<CoreExpr>),
    /// Signed integer greater-than comparison.
    Gt(Box<CoreExpr>, Box<CoreExpr>),
    /// An anonymous pure or effectful function.
    Lambda {
        params: Vec<String>,
        body: Box<CoreExpr>,
    },
    /// Construct a record value from named field expressions.
    ///
    /// Fields are in declaration order.  Callers must sort for CBOR
    /// determinism when field order is not semantically significant.
    RecordNew { fields: Vec<(String, CoreExpr)> },
    /// Read one field from a record expression.
    FieldGet {
        record: Box<CoreExpr>,
        field: String,
    },
    /// Immutable field update — returns a new record with one field replaced.
    FieldUpdate {
        record: Box<CoreExpr>,
        field: String,
        value: Box<CoreExpr>,
    },
    /// Construct a tuple from positional expressions.
    TupleNew(Vec<CoreExpr>),
    /// Construct a variant case, optionally carrying a payload.
    VariantNew {
        tag: String,
        payload: Option<Box<CoreExpr>>,
    },
    /// Construct a list from element expressions.
    ListNew(Vec<CoreExpr>),

    /// Infinite loop expression. Exits through `Break`.
    Loop {
        body: Box<CoreExpr>,
        /// Optional termination argument — `None` preserves backward compat.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        termination: Option<LoopTermination>,
    },
    /// Exit the nearest enclosing loop with a value.
    Break { value: Box<CoreExpr> },
    /// Continue at the nearest enclosing loop header.
    Continue,
    /// Structured while loop: `while cond { body }`.
    WhileLoop {
        cond: Box<CoreExpr>,
        body: Box<CoreExpr>,
        /// Optional termination argument — `None` preserves backward compat.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        termination: Option<LoopTermination>,
    },

    // ── Semantic effect / concurrency / runtime-check variants ────────────
    /// Short-circuit boolean AND: `left && right`.
    ///
    /// MUST lower to conditional branching in ANF — `right` is NOT evaluated
    /// when `left` is false.  Both operands are `Box<CoreExpr>`.
    And {
        left: Box<CoreExpr>,
        right: Box<CoreExpr>,
    },

    /// Short-circuit boolean OR: `left || right`.
    ///
    /// MUST lower to conditional branching in ANF — `right` is NOT evaluated
    /// when `left` is true.  Both operands are `Box<CoreExpr>`.
    Or {
        left: Box<CoreExpr>,
        right: Box<CoreExpr>,
    },

    /// An effect-ordered function call through a named capability.
    ///
    /// `capability` names the effect (e.g. `"database.read"`).
    /// `func` names the operation (e.g. `"Cart"`).
    /// `args` are the operands — may be non-atomic; atomized during ANF lowering.
    EffectCall {
        capability: String,
        func: String,
        args: Vec<CoreExpr>,
    },

    /// Dynamic dispatch through a handler/capability dispatch table.
    ///
    /// `handler` names the dispatch target (e.g. `"PaymentProvider"`).
    /// `method` names the operation.
    /// `args` are the operands.
    Dispatch {
        handler: String,
        method: String,
        args: Vec<CoreExpr>,
    },

    /// Spawn a concurrent task.
    ///
    /// `func` is the task entry-point name.
    /// `args` are the arguments passed to the task.
    TaskSpawn { func: String, args: Vec<CoreExpr> },

    /// Send a value on a channel.
    ///
    /// `channel` is the channel variable name (must be atomic).
    /// `value` is the message expression.
    ChannelSend {
        channel: Box<CoreExpr>,
        value: Box<CoreExpr>,
    },

    /// Receive a value from a channel.
    ///
    /// `channel` is the channel variable name (must be atomic).
    ChannelReceive { channel: Box<CoreExpr> },

    /// Insert a runtime check (assertion) before continuing.
    ///
    /// `check_ref` identifies the contract/proof obligation being checked.
    /// `cond` is the condition expression; `msg` is the failure message.
    RuntimeCheck {
        check_ref: String,
        cond: Box<CoreExpr>,
        msg: String,
    },

    /// Acquire a named resource.
    ///
    /// `resource` identifies the resource type.
    /// `args` are the acquisition arguments.
    ResourceAcquire {
        resource: String,
        args: Vec<CoreExpr>,
    },

    /// Release a previously acquired resource handle.
    ///
    /// `handle` is the variable holding the acquired resource (must be atomic).
    ResourceRelease { handle: Box<CoreExpr> },

    // ── G23: missing concurrency and cell primitives ──────────────────────
    /// Await a previously spawned task, blocking until it completes.
    ///
    /// `task` is the task handle expression; must be atomic in ANF.
    /// Returns the task's result value.
    TaskAwait { task: Box<CoreExpr> },

    /// Cancel a previously spawned task.
    ///
    /// `task` is the task handle expression; must be atomic in ANF.
    TaskCancel { task: Box<CoreExpr> },

    /// A scoped task group — all tasks spawned inside the body are tracked
    /// and awaited (or cancelled) before the scope exits.
    ///
    /// `body` is the expression body in which tasks may be spawned.
    TaskGroup { body: Box<CoreExpr> },

    /// Create a new channel with an optional bounded capacity.
    ///
    /// `capacity` is `None` for unbounded channels, or `Some(n)` for a
    /// channel that blocks senders when `n` items are queued.
    ChannelNew { capacity: Option<u64> },

    /// Select over multiple channel-receive cases; the first ready wins.
    ///
    /// Corresponds to `docs/core-ir.md §12 — Select`.
    /// `branches` must be non-empty; the winning arm's `body` is evaluated
    /// with its `binding` in scope.
    Select { branches: Vec<SelectClause> },

    /// Time-bound execution — evaluates `body` but errors/cancels after
    /// `duration` elapses.
    ///
    /// `duration` is the timeout value expression; must be atomic in ANF.
    /// `body` is the expression whose completion is time-bounded.
    Timeout {
        duration: Box<CoreExpr>,
        body: Box<CoreExpr>,
    },

    /// Create a new mutable cell initialised to `init`.
    ///
    /// Corresponds to `docs/core-ir.md §6 — CellNew<T>`.
    /// `init` is the initial value expression.
    CellNew { init: Box<CoreExpr> },

    /// Read the current value of a cell.
    ///
    /// Corresponds to `docs/core-ir.md §6 — CellGet<T>`.
    /// `cell` is the cell expression; must be atomic in ANF.
    CellGet { cell: Box<CoreExpr> },

    /// Write a new value to a cell.
    ///
    /// Corresponds to `docs/core-ir.md §6 — CellSet<T>`.
    /// `cell` is the cell expression; must be atomic in ANF.
    /// `value` is the new value expression; must be atomic in ANF.
    CellSet {
        cell: Box<CoreExpr>,
        value: Box<CoreExpr>,
    },

    /// Placeholder for nodes that have no expression body yet.
    ///
    /// Used by `lower_to_core_ir` for nodes that carry only type/kind
    /// information at this stage.
    Placeholder,

    // ── ola3-core-ir-types: new expression primitives ─────────────────────
    /// Structured iteration over a finite collection.
    ///
    /// `binding` is the loop variable name; `collection` must be finite
    /// (structurally bounded — no termination proof required).
    ForEach {
        binding: String,
        collection: Box<CoreExpr>,
        body: Box<CoreExpr>,
    },

    /// Left fold (reduce) over a collection.
    ///
    /// `init` is the initial accumulator; `list` is the collection to fold;
    /// `func` is the combining function (must be a `CoreExpr::Var` or `Lambda`).
    Fold {
        init: Box<CoreExpr>,
        list: Box<CoreExpr>,
        func: Box<CoreExpr>,
    },

    /// Explicit return from the current function with a value.
    Return { value: Box<CoreExpr> },

    /// Construct a map from a list of key-value expression pairs.
    ///
    /// Keys must be unique after evaluation; pairs are in declaration order.
    MapNew { entries: Vec<(CoreExpr, CoreExpr)> },

    /// Construct a set from a list of element expressions.
    ///
    /// Elements must be unique after evaluation.
    SetNew { elements: Vec<CoreExpr> },

    /// Indexed read from a collection (array or map).
    ///
    /// `collection` must evaluate to an indexable value; `index` must be
    /// in-bounds (checked at runtime or proven at compile time).
    IndexGet {
        collection: Box<CoreExpr>,
        index: Box<CoreExpr>,
    },

    /// An explicit FFI / external call with a declared trust boundary.
    ///
    /// `boundary` identifies the trust-level boundary (e.g., `"payments.stripe"`).
    /// `func` names the operation within that boundary.
    /// `args` are the arguments passed across the boundary.
    BoundaryCall {
        boundary: String,
        func: String,
        args: Vec<CoreExpr>,
    },

    /// An explicit proof assumption documented in the IR.
    ///
    /// `predicate` is the logical predicate being assumed (e.g., `"x > 0"`).
    /// `reason` documents why the assumption is justified.
    Assume { predicate: String, reason: String },

    /// Explicit panic / abort with a message.
    ///
    /// Represents an impossible-branch terminal — the control flow never
    /// returns from this expression.  Used where `Never` is expected.
    Abort { message: String },
}

// ── CoreType ──────────────────────────────────────────────────────────────

/// Type primitives of the Semantic Core IR.
///
/// Corresponds to `docs/core-ir.md §3 — Sistema de tipos`.
///
/// All variants are unit-like at this stage; parameterised types (e.g.
/// `List<T>`, `Option<T>`) will carry sub-`CoreType` payloads in a future
/// phase once type-parameter resolution is wired through the semantic graph.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoreType {
    /// `()` — the unit type; returned by functions with no meaningful value.
    Unit,
    /// `Never` — uninhabited type; represents divergence or impossible branches.
    Never,
    /// Boolean type.
    Bool,
    /// Signed integer type (platform-default width at this stage).
    Int,
    /// Unsigned integer type.
    UInt,
    /// Floating-point type (IEEE 754 double).
    Float,
    /// UTF-8 text / string type.
    Text,
    /// Opaque byte sequence.
    Bytes,
    /// Nominal product type (`{ field: Type, ... }`).
    Record,
    /// Nominal sum type (`CaseA | CaseB(Payload)`).
    Variant,
    /// Structural product type (positional: `(A, B, C)`).
    Tuple,
    /// Homogeneous ordered collection with element type.
    ///
    /// `List(Box::new(CoreType::Int))` represents `List<Int>`.
    List(Box<CoreType>),
    /// Key-value association (ordered by key for determinism).
    ///
    /// `Map(key_type, value_type)` — e.g., `Map<Text, Int>`.
    Map(Box<CoreType>, Box<CoreType>),
    /// Unordered unique-element collection.
    ///
    /// `Set(Box::new(CoreType::Int))` represents `Set<Int>`.
    Set(Box<CoreType>),
    /// Optional value — `Some(T) | None`.
    ///
    /// `Option(Box::new(CoreType::Bool))` represents `Option<Bool>`.
    Option(Box<CoreType>),
    /// Fallible value — `Ok(T) | Err(E)`.
    ///
    /// `Result(ok_type, err_type)` — e.g., `Result<Int, Text>`.
    Result(Box<CoreType>, Box<CoreType>),
    /// Function type `(Params) -> Return` with optional effect row.
    Function {
        /// Ordered parameter types.
        params: Vec<CoreType>,
        /// Return type.
        ret: Box<CoreType>,
        /// Named effects (e.g., `["IO", "State"]`).
        effects: Vec<String>,
    },
    /// External resource handle with an ownership mode.
    Handle {
        /// The resource type being wrapped.
        resource: Box<CoreType>,
        /// The ownership / linearity mode.
        mode: ResourceMode,
    },
    /// A base type refined by a logical predicate.
    Refinement {
        /// The base type being refined.
        base: Box<CoreType>,
        /// The predicate expression string.
        predicate: String,
    },
    /// Generic/unknown type — used as a fallback when the nominal is
    /// unrecognised or when type parameters have not been resolved yet.
    Generic,

    // ── ola3-core-ir-types: new flat numeric and Unicode variants ─────────
    /// Arbitrary-precision decimal number type.
    Decimal,
    /// Existential type — a value whose type is hidden behind an interface.
    Existential,
    /// Unicode code point (scalar value, U+0000..U+10FFFF).
    CodePoint,
    /// A single user-perceived character cluster (grapheme cluster).
    Grapheme,
    /// Unicode normalized text with an explicit normalization form.
    ///
    /// The `String` payload carries the form name: `"NFC"`, `"NFD"`,
    /// `"NFKC"`, or `"NFKD"`.
    NormalizedText(String),
    /// Signed 32-bit integer (fixed-width platform machine type).
    Int32,
    /// Signed 64-bit integer (fixed-width platform machine type).
    Int64,
    /// Unsigned 32-bit integer.
    UInt32,
    /// Unsigned 64-bit integer.
    UInt64,
    /// Type representing a group of concurrent tasks (mirrors `CoreExpr::TaskGroup`).
    TaskGroup,

    // ── ola3-core-ir-types: new parameterized collection and boundary types ─
    /// Partial-update field type.
    ///
    /// `PatchField(Box::new(CoreType::Text))` represents `PatchField<Text>`.
    PatchField(Box<CoreType>),
    /// Fixed-capacity vector (size is a separate ConstParam, not carried here).
    ///
    /// `Vector(Box::new(CoreType::Float))` represents `Vector<Float, N>`.
    Vector(Box<CoreType>),
    /// Ordered (sorted) unique-element set.
    ///
    /// `OrderedSet(Box::new(CoreType::Int))` represents `OrderedSet<Int>`.
    OrderedSet(Box<CoreType>),
    /// Ordered (sorted) key-value map.
    ///
    /// `OrderedMap(key_type, value_type)` — e.g., `OrderedMap<Int, Text>`.
    OrderedMap(Box<CoreType>, Box<CoreType>),
    /// Fixed-length array.
    ///
    /// `Array(Box::new(CoreType::Int))` represents `Array<Int, N>`.
    Array(Box<CoreType>),
    /// Asynchronous task returning a value of the given type.
    ///
    /// `Task(Box::new(CoreType::Bool))` represents `Task<Bool>`.
    Task(Box<CoreType>),
    /// Asynchronous message channel.
    ///
    /// `Channel(Box::new(CoreType::Text))` represents `Channel<Text>`.
    Channel(Box<CoreType>),
    /// Opaque external (foreign) type, identified by name.
    ForeignType(String),
    /// An encoded representation of a value (e.g., `Encoded<Json>`).
    Encoded(String),
    /// A decoded/parsed value of the given type.
    Decoded(Box<CoreType>),
}

// ── CoreNode ──────────────────────────────────────────────────────────────

/// One node in the Core IR, with full provenance back to the source graph.
///
/// In Phase 7 there is a 1-to-1 mapping from `SemanticGraph` nodes to
/// `CoreNode`s; the `source_ref` field preserves that mapping.
///
/// The `ty` and `expr` fields were added in G2 (core-ir-full).  Both are
/// serialized only when `Some` to preserve CBOR wire-format compatibility
/// with pre-G2 artifacts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreNode {
    /// The `NodeRef` this `CoreNode` was lowered from.
    pub source_ref: NodeRef,
    /// Compiler IR node kind (mirrors the source `NodeKind`).
    pub kind: CoreNodeKind,
    /// Node name, copied verbatim from the source `GraphNode`.
    pub name: String,
    /// Resolved Core IR type, when available.
    ///
    /// Populated by `lower_to_core_ir` from `GraphNode.type_facts` when
    /// present; `None` for nodes without type information.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<CoreType>,
    /// Core IR expression body, when available.
    ///
    /// `None` for nodes that carry only structural information (modules,
    /// types, capabilities, etc.) at this stage.  Expression bodies will
    /// be populated in a future expression-lowering phase.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expr: Option<CoreExpr>,
}

// ── StageHashes ───────────────────────────────────────────────────────────

/// Accumulates BLAKE3 hashes as the pipeline advances through its stages.
///
/// `graph_snapshot_hash` and `verification_report_hash` are computed from
/// the pipeline inputs.  `core_ir_hash`, `anf_ir_hash`, and `wasm_hash` are
/// filled in by successive stages.  Optional fields are `None` until the
/// corresponding stage completes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageHashes {
    /// BLAKE3 hash of the serialised `SemanticGraph` (pipeline input).
    pub graph_snapshot_hash: [u8; 32],
    /// BLAKE3 hash of the serialised `VerificationReport` (pipeline input).
    pub verification_report_hash: [u8; 32],
    /// `blake3(graph_snapshot_hash || core_ir_bytes)` — set by `lower_to_core_ir`.
    pub core_ir_hash: [u8; 32],
    /// `blake3(core_ir_hash || anf_ir_bytes)` — set by `lower_to_anf`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anf_ir_hash: Option<[u8; 32]>,
    /// `blake3(anf_ir_hash || wasm_binary)` — set by `emit_wasm`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wasm_hash: Option<[u8; 32]>,
    /// `blake3(anf_ir_hash || native_binary)` — set by `emit_native`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_hash: Option<[u8; 32]>,
    /// `blake3(source_map_cbor_bytes)` — set by backend stages after populating offsets.
    ///
    /// Any change to the semantic source map (offsets, provenance fields)
    /// causes this hash to change, invalidating downstream manifests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_map_hash: Option<[u8; 32]>,
    /// `blake3(artifact_manifest_cbor_bytes)` — set by artifact manifest emission.
    ///
    /// Covers profile, compiler version, and all upstream artifact hashes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_manifest_hash: Option<[u8; 32]>,
}

// ── CoreIr ────────────────────────────────────────────────────────────────

/// Output of the first pipeline stage: a flat list of typed Core IR nodes
/// with full provenance and a sealed hash chain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreIr {
    /// Lowered nodes in source graph traversal order.
    pub nodes: Vec<CoreNode>,
    /// Hash chain sealed through the Core IR stage.
    pub stage_hashes: StageHashes,
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::stable_cbor_bytes;
    #[allow(unused_imports)]
    use ciborium;

    // ── Task 1.5 — RED: tests written before types existed. ───────────────

    // Scenario: CoreIr is constructible with one CoreNode.
    // Base case — proves the struct and its fields accept the right types.
    #[test]
    fn core_ir_is_constructible_with_one_node() {
        let node = CoreNode {
            source_ref: NodeRef(0),
            kind: CoreNodeKind::Module,
            name: "core_mod".to_string(),
            ty: None,
            expr: None,
        };
        let ir = CoreIr {
            nodes: vec![node],
            stage_hashes: StageHashes {
                graph_snapshot_hash: [0u8; 32],
                verification_report_hash: [0u8; 32],
                core_ir_hash: [1u8; 32],
                anf_ir_hash: None,
                wasm_hash: None,
                native_hash: None,
                source_map_hash: None,
                artifact_manifest_hash: None,
            },
        };
        assert_eq!(ir.nodes.len(), 1);
        assert_eq!(ir.nodes[0].source_ref, NodeRef(0));
        assert_eq!(ir.nodes[0].kind, CoreNodeKind::Module);
    }

    // Scenario: CoreNode preserves its source_ref provenance.
    // Proves the provenance contract: source_ref is not dropped or mutated.
    #[test]
    fn core_node_preserves_source_ref() {
        let node = CoreNode {
            source_ref: NodeRef(99),
            kind: CoreNodeKind::Function,
            name: "fn_with_high_ref".to_string(),
            ty: None,
            expr: None,
        };
        assert_eq!(node.source_ref, NodeRef(99));
    }

    // TRIANGULATE: stable_cbor_bytes on Vec<CoreNode> is deterministic.
    // Proves that the Serialize impl produces stable bytes for the node list
    // — the actual content used for hash sealing in lower_to_core_ir (PR 2).
    #[test]
    fn core_node_list_cbor_is_deterministic() {
        let nodes = vec![
            CoreNode {
                source_ref: NodeRef(0),
                kind: CoreNodeKind::Function,
                name: "fn_a".to_string(),
                ty: None,
                expr: None,
            },
            CoreNode {
                source_ref: NodeRef(1),
                kind: CoreNodeKind::Module,
                name: "mod_b".to_string(),
                ty: None,
                expr: None,
            },
            CoreNode {
                source_ref: NodeRef(2),
                kind: CoreNodeKind::Effect,
                name: "eff_c".to_string(),
                ty: None,
                expr: None,
            },
        ];
        let b1 = stable_cbor_bytes(&nodes).expect("first encode");
        let b2 = stable_cbor_bytes(&nodes).expect("second encode");
        assert_eq!(
            b1, b2,
            "Vec<CoreNode> must produce identical CBOR bytes across calls"
        );
    }

    // TRIANGULATE: different CoreNode lists produce different CBOR bytes.
    // Proves the encoding is not constant (real content affects output).
    #[test]
    fn different_core_node_lists_produce_different_cbor() {
        let list_a = vec![CoreNode {
            source_ref: NodeRef(0),
            kind: CoreNodeKind::Module,
            name: "a".to_string(),
            ty: None,
            expr: None,
        }];
        let list_b = vec![CoreNode {
            source_ref: NodeRef(0),
            kind: CoreNodeKind::Module,
            name: "b".to_string(),
            ty: None,
            expr: None,
        }];
        let b_a = stable_cbor_bytes(&list_a).expect("encode a");
        let b_b = stable_cbor_bytes(&list_b).expect("encode b");
        assert_ne!(
            b_a, b_b,
            "different CoreNode lists must produce different CBOR"
        );
    }

    // Scenario: StageHashes optional fields are None by default.
    #[test]
    fn stage_hashes_optional_fields_default_none() {
        let h = StageHashes {
            graph_snapshot_hash: [0u8; 32],
            verification_report_hash: [0u8; 32],
            core_ir_hash: [42u8; 32],
            anf_ir_hash: None,
            wasm_hash: None,
            native_hash: None,
            source_map_hash: None,
            artifact_manifest_hash: None,
        };
        assert!(h.anf_ir_hash.is_none());
        assert!(h.wasm_hash.is_none());
        assert!(h.native_hash.is_none());
        assert_eq!(h.core_ir_hash, [42u8; 32]);
    }

    // TRIANGULATE: all CoreNodeKind variants are constructible.
    // Ensures no variant is accidentally omitted from the enum.
    #[test]
    fn all_core_node_kinds_are_constructible() {
        let kinds = [
            CoreNodeKind::Module,
            CoreNodeKind::Function,
            CoreNodeKind::Type,
            CoreNodeKind::Effect,
            CoreNodeKind::Capability,
            CoreNodeKind::Contract,
            CoreNodeKind::Invariant,
            CoreNodeKind::Test,
            CoreNodeKind::Boundary,
            CoreNodeKind::Package,
        ];
        assert_eq!(
            kinds.len(),
            10,
            "all 10 CoreNodeKind variants must be reachable"
        );
    }

    // ── G2: CoreType tests ────────────────────────────────────────────────

    // S2: All original CoreType variants are constructible without panic.
    // Updated for ola3-core-ir-types: parameterized variants now carry inner types.
    #[test]
    fn all_core_type_variants_are_constructible() {
        // Original unit-like variants (unchanged).
        let _unit = CoreType::Unit;
        let _never = CoreType::Never;
        let _bool = CoreType::Bool;
        let _int = CoreType::Int;
        let _uint = CoreType::UInt;
        let _float = CoreType::Float;
        let _text = CoreType::Text;
        let _bytes = CoreType::Bytes;
        let _record = CoreType::Record;
        let _variant = CoreType::Variant;
        let _tuple = CoreType::Tuple;
        let _generic = CoreType::Generic;
        // Parameterized variants (now carry inner types).
        let _list = CoreType::List(Box::new(CoreType::Int));
        let _map = CoreType::Map(Box::new(CoreType::Text), Box::new(CoreType::Int));
        let _set = CoreType::Set(Box::new(CoreType::Bool));
        let _option = CoreType::Option(Box::new(CoreType::Int));
        let _result = CoreType::Result(Box::new(CoreType::Int), Box::new(CoreType::Text));
        let _function = CoreType::Function {
            params: vec![CoreType::Int],
            ret: Box::new(CoreType::Bool),
            effects: vec![],
        };
        let _handle = CoreType::Handle {
            resource: Box::new(CoreType::Text),
            mode: ResourceMode::Copy,
        };
        let _refinement = CoreType::Refinement {
            base: Box::new(CoreType::Int),
            predicate: "x > 0".to_string(),
        };
        // All constructed without panic — test passes.
    }

    // S1: CoreType::Bool is constructible and serializable (deterministic CBOR).
    #[test]
    fn core_type_bool_cbor_is_deterministic() {
        let ty = CoreType::Bool;
        let b1 = stable_cbor_bytes(&ty).expect("first encode");
        let b2 = stable_cbor_bytes(&ty).expect("second encode");
        assert_eq!(b1, b2, "CoreType::Bool CBOR must be deterministic");
    }

    // TRIANGULATE: different CoreType variants produce different CBOR bytes.
    #[test]
    fn different_core_types_produce_different_cbor() {
        let b_int = stable_cbor_bytes(&CoreType::Int).expect("encode Int");
        let b_text = stable_cbor_bytes(&CoreType::Text).expect("encode Text");
        assert_ne!(b_int, b_text, "Int and Text must produce different CBOR");
    }

    // ── G2: CoreExpr tests ────────────────────────────────────────────────

    // S3: All CoreExpr variants are constructible without panic.
    #[test]
    fn all_core_expr_variants_are_constructible() {
        let _literal = CoreExpr::Literal(LiteralValue::Int(42));
        let _var = CoreExpr::Var("x".to_string());
        let _let = CoreExpr::Let {
            name: "y".to_string(),
            value: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
            body: Box::new(CoreExpr::Var("y".to_string())),
        };
        let _if = CoreExpr::If {
            cond: Box::new(CoreExpr::Literal(LiteralValue::Bool(true))),
            then_: Box::new(CoreExpr::Literal(LiteralValue::Int(1))),
            else_: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
        };
        let _match = CoreExpr::Match {
            scrutinee: Box::new(CoreExpr::Var("v".to_string())),
            arms: vec![MatchArm {
                pattern: "Some(x)".to_string(),
                body: CoreExpr::Var("x".to_string()),
            }],
        };
        let _call = CoreExpr::Call {
            func: "fn.add".to_string(),
            args: vec![
                CoreExpr::Var("a".to_string()),
                CoreExpr::Var("b".to_string()),
            ],
        };
        let _lambda = CoreExpr::Lambda {
            params: vec!["x".to_string()],
            body: Box::new(CoreExpr::Var("x".to_string())),
        };
        let _record = CoreExpr::RecordNew {
            fields: vec![(
                "amount".to_string(),
                CoreExpr::Literal(LiteralValue::Int(10)),
            )],
        };
        let _field_get = CoreExpr::FieldGet {
            record: Box::new(CoreExpr::Var("order".to_string())),
            field: "total".to_string(),
        };
        let _field_update = CoreExpr::FieldUpdate {
            record: Box::new(CoreExpr::Var("order".to_string())),
            field: "status".to_string(),
            value: Box::new(CoreExpr::Literal(LiteralValue::Text("Paid".to_string()))),
        };
        let _tuple = CoreExpr::TupleNew(vec![
            CoreExpr::Literal(LiteralValue::Int(1)),
            CoreExpr::Literal(LiteralValue::Bool(false)),
        ]);
        let _variant = CoreExpr::VariantNew {
            tag: "Ok".to_string(),
            payload: Some(Box::new(CoreExpr::Literal(LiteralValue::Unit))),
        };
        let _list = CoreExpr::ListNew(vec![CoreExpr::Literal(LiteralValue::Int(1))]);
        let _loop = CoreExpr::Loop {
            body: Box::new(CoreExpr::Break {
                value: Box::new(CoreExpr::Literal(LiteralValue::Int(10))),
            }),
            termination: None,
        };
        let _break = CoreExpr::Break {
            value: Box::new(CoreExpr::Literal(LiteralValue::Int(1))),
        };
        let _continue = CoreExpr::Continue;
        let _while_loop = CoreExpr::WhileLoop {
            cond: Box::new(CoreExpr::Literal(LiteralValue::Bool(false))),
            body: Box::new(CoreExpr::Continue),
            termination: None,
        };
        let _placeholder = CoreExpr::Placeholder;
        // All constructed without panic — test passes.
    }

    // S4: CoreNode with CoreType payload round-trips through CBOR.
    #[test]
    fn core_node_with_type_payload_cbor_round_trip() {
        let node = CoreNode {
            source_ref: NodeRef(5),
            kind: CoreNodeKind::Type,
            name: "amount".to_string(),
            ty: Some(CoreType::Int),
            expr: Some(CoreExpr::Var("amount_var".to_string())),
        };
        let bytes = stable_cbor_bytes(&node).expect("encode must succeed");
        let decoded: CoreNode =
            ciborium::from_reader(bytes.as_slice()).expect("decode must succeed");
        assert_eq!(
            decoded, node,
            "CoreNode with ty+expr must survive CBOR round-trip"
        );
    }

    // S5: CoreNode without type/expr fields skips those fields in CBOR.
    // Verifies backward-compat: a node with None fields produces fewer bytes
    // than a node with populated ty/expr (the optional fields are absent).
    #[test]
    fn core_node_without_type_fields_omits_them_from_cbor() {
        let node_minimal = CoreNode {
            source_ref: NodeRef(0),
            kind: CoreNodeKind::Module,
            name: "m".to_string(),
            ty: None,
            expr: None,
        };
        let node_rich = CoreNode {
            source_ref: NodeRef(0),
            kind: CoreNodeKind::Module,
            name: "m".to_string(),
            ty: Some(CoreType::Bool),
            expr: Some(CoreExpr::Placeholder),
        };
        let bytes_minimal = stable_cbor_bytes(&node_minimal).expect("encode minimal");
        let bytes_rich = stable_cbor_bytes(&node_rich).expect("encode rich");
        // The rich node must produce strictly more bytes (it has extra fields).
        assert!(
            bytes_minimal.len() < bytes_rich.len(),
            "node with ty+expr must encode to more bytes than node without them: {} vs {}",
            bytes_minimal.len(),
            bytes_rich.len()
        );
        // Round-trip the minimal node to confirm no extra keys sneak in.
        let decoded: CoreNode =
            ciborium::from_reader(bytes_minimal.as_slice()).expect("decode minimal");
        assert_eq!(decoded.ty, None, "decoded ty must be None");
        assert_eq!(decoded.expr, None, "decoded expr must be None");
    }

    // LiteralValue: all 5 variants constructible and Eq is sound.
    #[test]
    fn literal_value_variants_eq() {
        assert_eq!(LiteralValue::Bool(true), LiteralValue::Bool(true));
        assert_ne!(LiteralValue::Bool(true), LiteralValue::Bool(false));
        assert_eq!(LiteralValue::Int(42), LiteralValue::Int(42));
        assert_eq!(LiteralValue::Float(1.0), LiteralValue::Float(1.0));
        assert_eq!(
            LiteralValue::Text("hello".to_string()),
            LiteralValue::Text("hello".to_string())
        );
        assert_eq!(LiteralValue::Unit, LiteralValue::Unit);
    }

    // G23: all new concurrency + cell CoreExpr variants are constructible.
    #[test]
    fn all_new_concurrency_cell_core_expr_variants_are_constructible() {
        let _task_await = CoreExpr::TaskAwait {
            task: Box::new(CoreExpr::Var("task_handle".to_string())),
        };
        let _task_cancel = CoreExpr::TaskCancel {
            task: Box::new(CoreExpr::Var("task_handle".to_string())),
        };
        let _task_group = CoreExpr::TaskGroup {
            body: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
        };
        let _channel_new_unbounded = CoreExpr::ChannelNew { capacity: None };
        let _channel_new_bounded = CoreExpr::ChannelNew { capacity: Some(16) };
        let _select = CoreExpr::Select {
            branches: vec![SelectClause {
                channel: Box::new(CoreExpr::Var("ch".to_string())),
                binding: "msg".to_string(),
                body: CoreExpr::Var("msg".to_string()),
            }],
        };
        let _timeout = CoreExpr::Timeout {
            duration: Box::new(CoreExpr::Var("dur_ms".to_string())),
            body: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
        };
        let _cell_new = CoreExpr::CellNew {
            init: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
        };
        let _cell_get = CoreExpr::CellGet {
            cell: Box::new(CoreExpr::Var("counter".to_string())),
        };
        let _cell_set = CoreExpr::CellSet {
            cell: Box::new(CoreExpr::Var("counter".to_string())),
            value: Box::new(CoreExpr::Literal(LiteralValue::Int(1))),
        };
        // All constructed without panic — test passes.
    }

    // TRIANGULATE: SelectClause is constructible and fields are accessible.
    #[test]
    fn select_clause_is_constructible_with_correct_fields() {
        let clause = SelectClause {
            channel: Box::new(CoreExpr::Var("inbox".to_string())),
            binding: "item".to_string(),
            body: CoreExpr::Var("item".to_string()),
        };
        assert_eq!(clause.binding, "item");
        assert_eq!(*clause.channel, CoreExpr::Var("inbox".to_string()));
        assert_eq!(clause.body, CoreExpr::Var("item".to_string()));
    }

    // G23: new concurrency variants round-trip through CBOR.
    #[test]
    fn new_concurrency_cell_variants_cbor_round_trip() {
        let variants: Vec<CoreExpr> = vec![
            CoreExpr::TaskAwait {
                task: Box::new(CoreExpr::Var("t".to_string())),
            },
            CoreExpr::TaskCancel {
                task: Box::new(CoreExpr::Var("t".to_string())),
            },
            CoreExpr::TaskGroup {
                body: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
            },
            CoreExpr::ChannelNew { capacity: None },
            CoreExpr::ChannelNew { capacity: Some(8) },
            CoreExpr::Select {
                branches: vec![SelectClause {
                    channel: Box::new(CoreExpr::Var("ch".to_string())),
                    binding: "v".to_string(),
                    body: CoreExpr::Var("v".to_string()),
                }],
            },
            CoreExpr::Timeout {
                duration: Box::new(CoreExpr::Var("d".to_string())),
                body: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
            },
            CoreExpr::CellNew {
                init: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
            },
            CoreExpr::CellGet {
                cell: Box::new(CoreExpr::Var("c".to_string())),
            },
            CoreExpr::CellSet {
                cell: Box::new(CoreExpr::Var("c".to_string())),
                value: Box::new(CoreExpr::Literal(LiteralValue::Int(42))),
            },
        ];
        for expr in &variants {
            let bytes = stable_cbor_bytes(expr).expect("encode must succeed");
            let decoded: CoreExpr =
                ciborium::from_reader(bytes.as_slice()).expect("decode must succeed");
            assert_eq!(
                &decoded, expr,
                "CoreExpr::{expr:?} must survive CBOR round-trip"
            );
        }
    }

    // MatchArm is constructible and Eq.
    #[test]
    fn match_arm_is_constructible() {
        let arm = MatchArm {
            pattern: "None".to_string(),
            body: CoreExpr::Placeholder,
        };
        assert_eq!(arm.pattern, "None");
        assert_eq!(arm.body, CoreExpr::Placeholder);
    }

    // ── Task C1 (RED): LoopTermination on Loop/WhileLoop ─────────────────

    // S-C1a: Loop with termination=Some(Proven) round-trips through CBOR.
    #[test]
    fn loop_with_proven_termination_cbor_round_trip() {
        let expr = CoreExpr::Loop {
            body: Box::new(CoreExpr::Break {
                value: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
            }),
            termination: Some(LoopTermination::Proven),
        };
        let bytes = stable_cbor_bytes(&expr).expect("encode");
        let decoded: CoreExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(decoded, expr, "Loop with Proven termination must survive CBOR round-trip");
        if let CoreExpr::Loop { termination, .. } = &decoded {
            assert_eq!(termination.as_ref(), Some(&LoopTermination::Proven));
        } else {
            panic!("expected Loop variant");
        }
    }

    // S-C1b: Loop with termination=None is backward-compatible.
    // A loop without termination must produce the same bytes as before this change
    // (serde skips None via skip_serializing_if).
    #[test]
    fn loop_without_termination_is_backward_compat() {
        let expr_with_none = CoreExpr::Loop {
            body: Box::new(CoreExpr::Continue),
            termination: None,
        };
        // The legacy form (before termination field) is equivalent to termination: None.
        // Verify round-trip preserves None.
        let bytes = stable_cbor_bytes(&expr_with_none).expect("encode");
        let decoded: CoreExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(decoded, expr_with_none);
        if let CoreExpr::Loop { termination, .. } = decoded {
            assert!(termination.is_none(), "termination must be None after round-trip");
        }
    }

    // S-C1c: WhileLoop with termination=Some(Bounded) round-trips.
    // Triangulation: WhileLoop termination works independently from Loop.
    #[test]
    fn while_loop_with_bounded_termination_cbor_round_trip() {
        let expr = CoreExpr::WhileLoop {
            cond: Box::new(CoreExpr::Literal(LiteralValue::Bool(true))),
            body: Box::new(CoreExpr::Continue),
            termination: Some(LoopTermination::Bounded),
        };
        let bytes = stable_cbor_bytes(&expr).expect("encode");
        let decoded: CoreExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(decoded, expr, "WhileLoop with Bounded termination must round-trip");
    }

    // S-C1d: All LoopTermination variants are constructible.
    #[test]
    fn all_loop_termination_variants_are_constructible() {
        let _proven = LoopTermination::Proven;
        let _bounded = LoopTermination::Bounded;
        let _assumed = LoopTermination::Assumed;
        let _unverified = LoopTermination::Unverified;
        // All constructed without panic — test passes.
    }

    // ── Task B1 (RED): parameterized CoreType variants ────────────────────

    // S-B1a: List(Box<CoreType::Int>) round-trips through CBOR.
    #[test]
    fn list_with_inner_type_cbor_round_trip() {
        let ty = CoreType::List(Box::new(CoreType::Int));
        let bytes = stable_cbor_bytes(&ty).expect("encode");
        let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(decoded, ty, "List<Int> must survive CBOR round-trip");
        // Verify inner type is preserved.
        if let CoreType::List(inner) = decoded {
            assert_eq!(*inner, CoreType::Int);
        } else {
            panic!("expected List variant");
        }
    }

    // S-B1b: Map(Text, Int) round-trips — both key and value types preserved.
    #[test]
    fn map_with_key_and_value_types_cbor_round_trip() {
        let ty = CoreType::Map(Box::new(CoreType::Text), Box::new(CoreType::Int));
        let bytes = stable_cbor_bytes(&ty).expect("encode");
        let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(decoded, ty, "Map<Text, Int> must survive CBOR round-trip");
    }

    // S-B1c: Option(Bool) round-trips.
    #[test]
    fn option_with_inner_type_cbor_round_trip() {
        let ty = CoreType::Option(Box::new(CoreType::Bool));
        let bytes = stable_cbor_bytes(&ty).expect("encode");
        let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(decoded, ty, "Option<Bool> must survive CBOR round-trip");
    }

    // S-B1d: Result(Int, Text) round-trips — Ok and Err types preserved.
    #[test]
    fn result_with_ok_and_err_types_cbor_round_trip() {
        let ty = CoreType::Result(Box::new(CoreType::Int), Box::new(CoreType::Text));
        let bytes = stable_cbor_bytes(&ty).expect("encode");
        let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(decoded, ty, "Result<Int, Text> must survive CBOR round-trip");
    }

    // S-B1e: Handle { resource: Text, mode: ResourceMode::Linear } round-trips.
    #[test]
    fn handle_with_resource_and_mode_cbor_round_trip() {
        let ty = CoreType::Handle {
            resource: Box::new(CoreType::Text),
            mode: ResourceMode::Linear,
        };
        let bytes = stable_cbor_bytes(&ty).expect("encode");
        let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(decoded, ty, "Handle<Text, Linear> must survive CBOR round-trip");
        if let CoreType::Handle { resource, mode } = decoded {
            assert_eq!(*resource, CoreType::Text);
            assert_eq!(mode, ResourceMode::Linear);
        } else {
            panic!("expected Handle variant");
        }
    }

    // S-B1f: PatchField(Text) round-trips — new parameterized variant.
    #[test]
    fn patch_field_with_inner_type_cbor_round_trip() {
        let ty = CoreType::PatchField(Box::new(CoreType::Text));
        let bytes = stable_cbor_bytes(&ty).expect("encode");
        let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(decoded, ty, "PatchField<Text> must survive CBOR round-trip");
    }

    // S-B1g: Vector(Float) round-trips.
    #[test]
    fn vector_with_inner_type_cbor_round_trip() {
        let ty = CoreType::Vector(Box::new(CoreType::Float));
        let bytes = stable_cbor_bytes(&ty).expect("encode");
        let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(decoded, ty, "Vector<Float> must survive CBOR round-trip");
    }

    // S-B1h: OrderedSet(Int) round-trips.
    #[test]
    fn ordered_set_with_inner_type_cbor_round_trip() {
        let ty = CoreType::OrderedSet(Box::new(CoreType::Int));
        let bytes = stable_cbor_bytes(&ty).expect("encode");
        let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(decoded, ty, "OrderedSet<Int> must survive CBOR round-trip");
    }

    // S-B1i: Task(Bool) round-trips — concurrency type with inner.
    #[test]
    fn task_with_inner_type_cbor_round_trip() {
        let ty = CoreType::Task(Box::new(CoreType::Bool));
        let bytes = stable_cbor_bytes(&ty).expect("encode");
        let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(decoded, ty, "Task<Bool> must survive CBOR round-trip");
    }

    // S-B1j: Channel(Text) round-trips.
    // Triangulation: Task<Bool> and Channel<Text> must produce different CBOR.
    #[test]
    fn channel_with_inner_type_cbor_round_trip() {
        let ty = CoreType::Channel(Box::new(CoreType::Text));
        let bytes = stable_cbor_bytes(&ty).expect("encode");
        let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(decoded, ty, "Channel<Text> must survive CBOR round-trip");
        // Triangulation: Channel<Text> ≠ Task<Bool>
        let task_ty = CoreType::Task(Box::new(CoreType::Bool));
        let task_bytes = stable_cbor_bytes(&task_ty).expect("encode task");
        assert_ne!(bytes, task_bytes, "Channel<Text> must differ from Task<Bool> in CBOR");
    }

    // ── Task A1 (RED): new flat CoreType variants ─────────────────────────

    // S-A1a: All new flat CoreType variants are constructible.
    #[test]
    fn new_flat_core_type_variants_are_constructible() {
        let _decimal = CoreType::Decimal;
        let _existential = CoreType::Existential;
        let _code_point = CoreType::CodePoint;
        let _grapheme = CoreType::Grapheme;
        let _normalized_text = CoreType::NormalizedText("NFC".to_string());
        let _int32 = CoreType::Int32;
        let _int64 = CoreType::Int64;
        let _uint32 = CoreType::UInt32;
        let _uint64 = CoreType::UInt64;
        let _task_group = CoreType::TaskGroup;
        // All constructed without panic — test passes.
    }

    // S-A1b: NormalizedText carries its form string and round-trips through CBOR.
    #[test]
    fn normalized_text_cbor_round_trip() {
        let ty = CoreType::NormalizedText("NFC".to_string());
        let bytes = stable_cbor_bytes(&ty).expect("encode");
        let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(decoded, ty, "NormalizedText<NFC> must survive CBOR round-trip");
    }

    // S-A1c: Decimal is distinct from Int and Float in CBOR encoding.
    // Triangulation: different flat numeric types must produce different CBOR.
    #[test]
    fn decimal_is_distinct_from_int_and_float_in_cbor() {
        let b_decimal = stable_cbor_bytes(&CoreType::Decimal).expect("encode Decimal");
        let b_int = stable_cbor_bytes(&CoreType::Int).expect("encode Int");
        let b_float = stable_cbor_bytes(&CoreType::Float).expect("encode Float");
        assert_ne!(b_decimal, b_int, "Decimal must differ from Int in CBOR");
        assert_ne!(b_decimal, b_float, "Decimal must differ from Float in CBOR");
    }

    // S-A1d: Machine integer variants are all distinct from each other.
    // Triangulation: Int32/Int64/UInt32/UInt64 must encode differently.
    #[test]
    fn machine_integer_variants_are_distinct_in_cbor() {
        let b_i32 = stable_cbor_bytes(&CoreType::Int32).expect("encode Int32");
        let b_i64 = stable_cbor_bytes(&CoreType::Int64).expect("encode Int64");
        let b_u32 = stable_cbor_bytes(&CoreType::UInt32).expect("encode UInt32");
        let b_u64 = stable_cbor_bytes(&CoreType::UInt64).expect("encode UInt64");
        assert_ne!(b_i32, b_i64);
        assert_ne!(b_i32, b_u32);
        assert_ne!(b_i32, b_u64);
        assert_ne!(b_i64, b_u32);
        assert_ne!(b_i64, b_u64);
        assert_ne!(b_u32, b_u64);
    }

    // ── Task A3 (RED): new additive CoreExpr variants ─────────────────────

    // S-A3a: All new CoreExpr variants are constructible.
    #[test]
    fn new_core_expr_variants_are_constructible() {
        let _for_each = CoreExpr::ForEach {
            binding: "item".to_string(),
            collection: Box::new(CoreExpr::Var("cart.items".to_string())),
            body: Box::new(CoreExpr::Placeholder),
        };
        let _fold = CoreExpr::Fold {
            init: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
            list: Box::new(CoreExpr::Var("items".to_string())),
            func: Box::new(CoreExpr::Var("add".to_string())),
        };
        let _return_expr = CoreExpr::Return {
            value: Box::new(CoreExpr::Literal(LiteralValue::Int(42))),
        };
        let _map_new = CoreExpr::MapNew {
            entries: vec![(
                CoreExpr::Literal(LiteralValue::Text("key".to_string())),
                CoreExpr::Literal(LiteralValue::Int(1)),
            )],
        };
        let _set_new = CoreExpr::SetNew {
            elements: vec![
                CoreExpr::Literal(LiteralValue::Int(1)),
                CoreExpr::Literal(LiteralValue::Int(2)),
            ],
        };
        let _index_get = CoreExpr::IndexGet {
            collection: Box::new(CoreExpr::Var("arr".to_string())),
            index: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
        };
        let _boundary_call = CoreExpr::BoundaryCall {
            boundary: "payments.stripe".to_string(),
            func: "charge".to_string(),
            args: vec![CoreExpr::Var("order".to_string())],
        };
        let _assume = CoreExpr::Assume {
            predicate: "x > 0".to_string(),
            reason: "validated at entry point".to_string(),
        };
        let _abort = CoreExpr::Abort {
            message: "unreachable: invalid state".to_string(),
        };
        // All constructed without panic — test passes.
    }

    // S-A3b: ForEach fields are accessible and correct after construction.
    // Verifies the binding, collection, and body fields are correctly stored.
    #[test]
    fn for_each_fields_are_accessible() {
        let expr = CoreExpr::ForEach {
            binding: "item".to_string(),
            collection: Box::new(CoreExpr::Var("cart.items".to_string())),
            body: Box::new(CoreExpr::EffectCall {
                capability: "db".to_string(),
                func: "save".to_string(),
                args: vec![CoreExpr::Var("item".to_string())],
            }),
        };
        if let CoreExpr::ForEach {
            binding,
            collection,
            body,
        } = &expr
        {
            assert_eq!(binding, "item");
            assert_eq!(
                **collection,
                CoreExpr::Var("cart.items".to_string())
            );
            assert!(matches!(**body, CoreExpr::EffectCall { .. }));
        } else {
            panic!("expected ForEach variant");
        }
    }

    // S-A3c: BoundaryCall carries the boundary trust identifier.
    #[test]
    fn boundary_call_carries_trust_identifier() {
        let expr = CoreExpr::BoundaryCall {
            boundary: "payments.stripe".to_string(),
            func: "charge".to_string(),
            args: vec![],
        };
        if let CoreExpr::BoundaryCall { boundary, func, args } = &expr {
            assert_eq!(boundary, "payments.stripe");
            assert_eq!(func, "charge");
            assert!(args.is_empty());
        } else {
            panic!("expected BoundaryCall variant");
        }
    }

    // S-A3d: Assume and Abort CBOR round-trips.
    // Triangulation: two different structural variants with string fields.
    #[test]
    fn assume_and_abort_cbor_round_trip() {
        let assume_expr = CoreExpr::Assume {
            predicate: "balance >= 0".to_string(),
            reason: "invariant at domain boundary".to_string(),
        };
        let abort_expr = CoreExpr::Abort {
            message: "impossible branch reached".to_string(),
        };
        for expr in &[assume_expr, abort_expr] {
            let bytes = stable_cbor_bytes(expr).expect("encode");
            let decoded: CoreExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
            assert_eq!(&decoded, expr, "must survive CBOR round-trip");
        }
    }

    // S-A3e: ForEach, Fold, Return, MapNew, SetNew, IndexGet round-trip through CBOR.
    #[test]
    fn new_core_expr_variants_cbor_round_trip() {
        let variants: Vec<CoreExpr> = vec![
            CoreExpr::ForEach {
                binding: "x".to_string(),
                collection: Box::new(CoreExpr::Var("xs".to_string())),
                body: Box::new(CoreExpr::Placeholder),
            },
            CoreExpr::Fold {
                init: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
                list: Box::new(CoreExpr::Var("items".to_string())),
                func: Box::new(CoreExpr::Var("sum".to_string())),
            },
            CoreExpr::Return {
                value: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
            },
            CoreExpr::MapNew {
                entries: vec![(
                    CoreExpr::Literal(LiteralValue::Text("k".to_string())),
                    CoreExpr::Literal(LiteralValue::Int(0)),
                )],
            },
            CoreExpr::SetNew {
                elements: vec![CoreExpr::Literal(LiteralValue::Int(42))],
            },
            CoreExpr::IndexGet {
                collection: Box::new(CoreExpr::Var("list".to_string())),
                index: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
            },
            CoreExpr::BoundaryCall {
                boundary: "payments".to_string(),
                func: "charge".to_string(),
                args: vec![CoreExpr::Var("id".to_string())],
            },
        ];
        for expr in &variants {
            let bytes = stable_cbor_bytes(expr).expect("encode");
            let decoded: CoreExpr =
                ciborium::from_reader(bytes.as_slice()).expect("decode");
            assert_eq!(
                &decoded, expr,
                "CoreExpr::{:?} must survive CBOR round-trip",
                expr
            );
        }
    }
}
