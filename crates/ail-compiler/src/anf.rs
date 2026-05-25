// ── ail-compiler::anf ─────────────────────────────────────────────────────
//
// ANF (Administrative Normal Form) IR value types — the second lowering
// stage output.
//
// # Design constraints
//
// - `Vec` only (no `HashMap`) — workspace determinism contract.
// - All types `#[derive(Serialize)]` for CBOR hash sealing.
// - Every `AnfBinding` carries a `source_ref: NodeRef` that traces back to
//   the original `SemanticGraph` node; this provenance must survive lowering.
//
// # G3 scope (anf-real)
//
// Promotes `AnfBinding` from a flat placeholder to a real ANF IR node.
//
// `AnfExpr` mirrors `CoreExpr` but enforces A-Normal Form: every intermediate
// result is named.  Specifically:
//   - `Call.args` and `FieldGet.record` and `If.cond` are atomic names
//     (`String`), never nested expressions.
//   - Nested sub-expressions are let-bound before use (handled in lowering).
//
// `AnfBinding.expr` holds the normalised expression for this binding.
// Nodes without a `CoreExpr` body default to `AnfExpr::Literal(Unit)`.

use ail_core::semantic_graph::NodeRef;
use serde::{Deserialize, Serialize};

use crate::core_ir::{LiteralValue, StageHashes};

#[path = "anf_source_map.rs"]
mod anf_source_map;
pub use anf_source_map::{SourceMap, SourceMapEntry};

// ── Schema version ────────────────────────────────────────────────────────

/// Schema version for `AnfIr` serialization.
///
/// Incremented when the ANF IR schema changes in a backward-incompatible way.
/// Consumers MUST reject `AnfIr` artifacts whose `schema_version` is higher
/// than the version they understand.
pub const ANF_SCHEMA_VERSION: u32 = 1;

// ── AnfSelectClause ───────────────────────────────────────────────────────

/// One arm of an `AnfExpr::Select` expression (ANF form).
///
/// `channel` is an atomic variable name — guaranteed by the lowering pass.
/// `binding` names the variable that receives the value from the winning channel.
/// `body` is the ANF expression evaluated when this arm wins.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnfSelectClause {
    /// Channel variable name — atomic, guaranteed by lowering.
    pub channel: String,
    /// Name to bind the received value to within `body`.
    pub binding: String,
    /// Body expression evaluated when this arm wins.
    pub body: AnfExpr,
}

// ── AnfMatchArm ───────────────────────────────────────────────────────────

/// One arm of an `AnfExpr::Match` expression.
///
/// `pattern` is a string pattern (e.g. `"Ok(x)"`, `"None"`, `"_"`), matching
/// the same convention as `MatchArm` in `CoreExpr`.
/// `body` is the ANF expression evaluated when the pattern matches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnfMatchArm {
    /// Pattern string (e.g. `"Ok(x)"`, `"None"`, `"_"`). Backend execution
    /// supports:
    /// - integer literal patterns (e.g. `"42"`, `"-1"`) on I64 scrutinees.
    /// - boolean literal patterns (`"true"`, `"false"`) on I64/I32 scrutinees.
    /// - wildcard `"_"` — unconditionally matches.
    /// - tag-only constructor patterns (e.g. `"None"`) on I32 (variant pointer) scrutinees.
    /// - single-binding constructor patterns (e.g. `"Ok(val)"`, `"Some(x)"`) — loads
    ///   the payload from offset 8 in linear memory and binds it before evaluating the body.
    ///
    /// Multi-binding patterns (e.g. `"Ok(a, b)"`) are not yet supported and emit Unreachable.
    pub pattern: String,
    /// Body expression evaluated when the pattern matches.
    pub body: AnfExpr,
}

// ── AnfExpr ───────────────────────────────────────────────────────────────

/// A-Normal Form expression — all intermediate values are let-bound.
///
/// Corresponds to the ANF IR layer described in `docs/core-ir.md`:
/// > ANF IR: compiler IR principal; orden explícito de efectos.
///
/// Key ANF invariant: call arguments, field-access records, and if-conditions
/// are ALWAYS atomic (variable names), never nested expressions.  The lowering
/// stage ensures this by introducing fresh let-bindings for any non-atomic
/// sub-expression.
///
/// All variants must be serializable for CBOR determinism.  Recursive
/// sub-expressions use `Box<AnfExpr>`.  Collections use `Vec` (never
/// `HashMap`) per the workspace determinism contract.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AnfExpr {
    /// A typed constant — already atomic.
    Literal(LiteralValue),
    /// A reference to a local variable by name — atomic.
    Var(String),
    /// An immutable let-binding: `let <name> = <value> in <body>`.
    ///
    /// Used both for user-written lets and for synthetic temporaries
    /// introduced during ANF flattening of nested expressions.
    Let {
        name: String,
        value: Box<AnfExpr>,
        body: Box<AnfExpr>,
    },
    /// A boolean branch.
    ///
    /// `cond` is an atomic variable name — guaranteed by the lowering pass.
    If {
        cond: String,
        then_branch: Box<AnfExpr>,
        else_branch: Box<AnfExpr>,
    },
    /// A function call.
    ///
    /// `args` are atomic variable names — guaranteed by the lowering pass.
    Call { func: String, args: Vec<String> },
    /// Read one field from a named record.
    ///
    /// `record` is an atomic variable name — guaranteed by the lowering pass.
    FieldGet { record: String, field: String },
    /// Explicit return — wraps the return expression.
    Return(Box<AnfExpr>),
    /// Effect-ordered sequence of expressions.
    ///
    /// Used for sequential effect calls where the individual results are
    /// discarded (or each step produces a unit).
    Seq(Vec<AnfExpr>),

    // ── G20: expression body variants ────────────────────────────────────
    /// Pattern matching over a variant (or any scrutinee).
    ///
    /// `scrutinee` is an atomic variable name — guaranteed by the lowering
    /// pass.  Each arm carries a pattern string and a body expression.
    Match {
        scrutinee: String,
        arms: Vec<AnfMatchArm>,
    },

    /// An anonymous pure or effectful function.
    ///
    /// `params` are parameter names.  `body` is the ANF body expression,
    /// which may itself be a `Let`-chain.
    ///
    /// `captures` holds the names of variables from the enclosing scope that
    /// the lambda's body references — i.e., the explicit closure environment.
    /// Populated by the ANF lowering pass via `collect_free_vars` with `params`
    /// as the bound set.  Empty for lambdas that close over nothing.
    ///
    /// By-value captures only for now; resource-handle capture is deferred.
    Lambda {
        params: Vec<String>,
        /// Free variables of `body` relative to `params`, collected during
        /// ANF lowering.  These names must be in scope at the call site.
        ///
        /// `#[serde(default)]` ensures backward compatibility with CBOR artifacts
        /// produced before this field existed: missing key → empty Vec.
        #[serde(default)]
        captures: Vec<String>,
        body: Box<AnfExpr>,
    },

    /// Construct a record value from named field expressions.
    ///
    /// Fields are in declaration order.  Field expressions may be any
    /// `AnfExpr` (they are lowered recursively, not atomized).
    RecordNew { fields: Vec<(String, AnfExpr)> },

    /// Immutable field update — returns a new record with one field replaced.
    ///
    /// `record` is an atomic variable name — guaranteed by the lowering pass.
    /// `value` is the replacement expression, lowered recursively.
    FieldUpdate {
        record: String,
        field: String,
        value: Box<AnfExpr>,
    },

    /// Construct a tuple from positional expressions.
    ///
    /// Elements may be any `AnfExpr` (lowered recursively).
    TupleNew(Vec<AnfExpr>),

    /// Construct a variant case, optionally carrying a payload.
    ///
    /// `payload` is lowered recursively if present.
    VariantNew {
        tag: String,
        payload: Option<Box<AnfExpr>>,
    },

    /// Construct a list from element expressions.
    ///
    /// Elements may be any `AnfExpr` (lowered recursively).
    ListNew(Vec<AnfExpr>),

    /// Infinite loop expression. Exits through `Break`.
    Loop { body: Box<AnfExpr> },
    /// Exit the nearest enclosing `Loop` with a value.
    ///
    /// **Inside a `WhileLoop`**: the break value is discarded by the enclosing
    /// WASM block's arity 0 (`BlockType::Empty`).  `WhileLoop` always produces
    /// `I32 0` (unit) after the loop regardless of any `Break` value in the body.
    Break { value: Box<AnfExpr> },
    /// Continue at the nearest enclosing loop header.
    Continue,
    /// Structured while loop with an immutable condition name.
    ///
    /// `cond` is an ANF name — an immutable let-binding that must hold a `Bool`
    /// (`I32`) at runtime.  The condition is re-read via `LocalGet` on every
    /// iteration.  Because ANF names are immutable, a `cond` that is `true` at
    /// entry never terminates on its own: the body must contain a `Break`.
    ///
    /// **Result**: always `I32 0` (unit).  After the loop exits — whether the
    /// condition became false or a `Break` fired — `I32Const 0` is pushed onto
    /// the WASM stack so that `WhileLoop` can appear as a `Let`-binding value
    /// or in a `Seq` without a stack-underflow validation error.
    ///
    /// **`Break` inside `WhileLoop`**: the break value is discarded.  The
    /// outer WASM block has arity 0 (`BlockType::Empty`), so no break value is
    /// threaded out; the caller always sees the unit `I32 0` pushed after the loop.
    WhileLoop { cond: String, body: Box<AnfExpr> },

    // ── G20 R2: semantic effect / concurrency / runtime-check variants ────
    /// Short-circuit AND lowered to conditional branching.
    ///
    /// Evaluates `left`; if false, result is false without evaluating `right`.
    /// Both `left` and `right` are atomic variable names (guaranteed by lowering).
    ShortCircuitAnd { left: String, right: Box<AnfExpr> },

    /// Short-circuit OR lowered to conditional branching.
    ///
    /// Evaluates `left`; if true, result is true without evaluating `right`.
    /// Both `left` and `right` are atomic variable names (guaranteed by lowering).
    ShortCircuitOr { left: String, right: Box<AnfExpr> },

    /// Effect-ordered capability call.
    ///
    /// `capability` and `func` identify the effect operation.
    /// `args` are atomic variable names — guaranteed by lowering.
    EffectCall {
        capability: String,
        func: String,
        args: Vec<String>,
    },

    /// Dynamic dispatch through a handler/capability dispatch table.
    ///
    /// `handler` and `method` identify the dispatch target.
    /// `args` are atomic variable names — guaranteed by lowering.
    Dispatch {
        handler: String,
        method: String,
        args: Vec<String>,
    },

    /// Spawn a concurrent task (explicit ordering in ANF).
    ///
    /// `func` is the task entry-point name.
    /// `args` are atomic variable names — guaranteed by lowering.
    TaskSpawn { func: String, args: Vec<String> },

    /// Send a value on a channel (explicit ordering in ANF).
    ///
    /// `channel` is an atomic variable name.
    /// `value` is an atomic variable name.
    ChannelSend { channel: String, value: String },

    /// Receive a value from a channel (explicit ordering in ANF).
    ///
    /// `channel` is an atomic variable name.
    ChannelReceive { channel: String },

    /// Runtime assertion — contract/boundary check preserved through lowering.
    ///
    /// `check_ref` identifies the proof obligation.
    /// `cond` is an atomic variable name.
    /// `msg` is the failure message.
    RuntimeCheck {
        check_ref: String,
        cond: String,
        msg: String,
    },

    /// Acquire a named resource — ordering is explicit in ANF.
    ///
    /// `resource` identifies the resource type.
    /// `args` are atomic variable names — guaranteed by lowering.
    ResourceAcquire { resource: String, args: Vec<String> },

    /// Release a previously acquired resource handle.
    ///
    /// `handle` is an atomic variable name — guaranteed by lowering.
    ResourceRelease { handle: String },

    // ── G23: missing concurrency and cell primitives ──────────────────────
    /// Await a previously spawned task.
    ///
    /// `task` is an atomic variable name — guaranteed by lowering.
    TaskAwait { task: String },

    /// Cancel a previously spawned task.
    ///
    /// `task` is an atomic variable name — guaranteed by lowering.
    TaskCancel { task: String },

    /// A scoped task group — body may spawn tasks; all are awaited before exit.
    ///
    /// `body` is the ANF body expression for the group scope.
    TaskGroup { body: Box<AnfExpr> },

    /// Create a new channel (explicit ordering in ANF).
    ///
    /// `capacity` is `None` for unbounded, `Some(n)` for bounded.
    ChannelNew { capacity: Option<u64> },

    /// Select over multiple channel-receive cases; the first ready wins.
    ///
    /// `branches` must be non-empty; each clause has an atomic channel name,
    /// a binding name, and a body expression.
    Select { branches: Vec<AnfSelectClause> },

    /// Time-bound execution (explicit ordering in ANF).
    ///
    /// `duration` is an atomic variable name — guaranteed by lowering.
    /// `body` is the timed ANF expression.
    Timeout {
        duration: String,
        body: Box<AnfExpr>,
    },

    /// Create a new mutable cell initialised to a value.
    ///
    /// `init` is an atomic variable name — guaranteed by lowering.
    CellNew { init: String },

    /// Read the current value of a cell.
    ///
    /// `cell` is an atomic variable name — guaranteed by lowering.
    CellGet { cell: String },

    /// Write a new value to a cell.
    ///
    /// `cell` and `value` are atomic variable names — guaranteed by lowering.
    CellSet { cell: String, value: String },

    // ── ola5-compiler-core: Gap 2 — new ANF primitives ───────────────────
    /// An explicit proof assumption — no runtime effect; lowers to unit.
    ///
    /// `predicate` is the logical predicate being assumed.
    /// `reason` documents why the assumption is justified.
    Assume { predicate: String, reason: String },

    /// Explicit abort/panic — always traps at runtime.
    ///
    /// Represents an impossible-branch terminal with a diagnostic message.
    Abort { message: String },

    /// Indexed element access from a collection.
    ///
    /// `collection` and `index` are atomic variable names — guaranteed by lowering.
    /// Layout contract: collection pointer points to `[len: i64, elem0: i64, ...]`.
    IndexGet { collection: String, index: String },

    /// Construct a map from atomic key-value name pairs.
    ///
    /// Both keys and values are atomic variable names — guaranteed by lowering.
    /// Layout contract: `[count: i64, k0: i64, v0: i64, k1: i64, v1: i64, ...]`.
    MapNew { entries: Vec<(String, String)> },

    /// Construct a set from atomic element names.
    ///
    /// Elements are atomic variable names — guaranteed by lowering.
    /// Layout contract: `[count: i64, elem0: i64, elem1: i64, ...]`.
    SetNew { elements: Vec<String> },

    /// Structured loop over a list collection (ForEach).
    ///
    /// `collection` is an atomic variable name — guaranteed by lowering.
    /// `binding` names the loop variable; `body` is executed for each element.
    ForEach {
        binding: String,
        collection: String,
        body: Box<AnfExpr>,
    },

    /// Left fold (reduce) over a list collection.
    ///
    /// `init`, `list`, and `func` are atomic variable names — guaranteed by lowering.
    /// `func` must hold an I64 function pointer with signature `(acc: I64, elem: I64) -> I64`.
    Fold {
        init: String,
        list: String,
        func: String,
    },

    /// Placeholder for nodes that have no expression body yet, or for
    /// `CoreExpr::Placeholder` nodes.
    ///
    /// Backends treat this as a `trap`/`unreachable` stub.
    Placeholder,
}

// Manual Eq impl: required because `LiteralValue::Float` contains `f64`,
// which does not implement `Eq`.  We compare floats by bit pattern (NaN ==
// NaN for IR identity purposes — same bit pattern = same literal).
impl Eq for AnfExpr {}

// ── AnfBinding ────────────────────────────────────────────────────────────

/// One binding in the ANF IR — lowered from a `CoreNode`.
///
/// `source_ref` is the provenance chain back to the originating
/// `SemanticGraph` node.  It MUST equal the `CoreNode::source_ref` that
/// this binding was produced from.
///
/// `expr` holds the ANF expression for this binding.  Nodes without a
/// `CoreExpr` body default to `AnfExpr::Literal(LiteralValue::Unit)`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnfBinding {
    /// Original `NodeRef` from the `SemanticGraph` — preserved through
    /// Core IR and into ANF for full end-to-end provenance.
    pub source_ref: NodeRef,
    /// Binding name, copied from the `CoreNode`.
    pub name: String,
    /// ANF expression body for this binding.
    ///
    /// For top-level definitions without an expression body (modules, types,
    /// capabilities, etc.), this defaults to `AnfExpr::Literal(LiteralValue::Unit)`.
    pub expr: AnfExpr,
}

// ── AnfIr ─────────────────────────────────────────────────────────────────

/// Output of the second pipeline stage: a flat list of ANF bindings with
/// full provenance and an extended hash chain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnfIr {
    /// Schema version for forward compatibility.
    ///
    /// Consumers MUST reject artifacts whose `schema_version` exceeds the
    /// version they support.  Always set to `ANF_SCHEMA_VERSION` by
    /// `lower_to_anf`.
    pub schema_version: u32,

    /// ANF bindings in source traversal order.
    ///
    /// May contain more bindings than the originating `CoreIr.nodes` because
    /// the ANF flattening pass introduces synthetic let-bindings for nested
    /// sub-expressions.
    pub bindings: Vec<AnfBinding>,

    /// Semantic source map — maps ANF bindings back to semantic graph nodes.
    ///
    /// Generated by `lower_to_anf`; backend stages fill in `wasm_offset` /
    /// `native_offset` as they emit code.
    pub source_map: SourceMap,

    /// Hash chain extended through the ANF stage.
    /// `stage_hashes.anf_ir_hash` is `Some(...)` after this stage completes.
    pub stage_hashes: StageHashes,
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "anf_tests.rs"]
mod tests;
