// ── ail-compiler::core_ir::expr ──────────────────────────────────────────
//
// Pure expression primitives of the Semantic Core IR, together with the
// helper structs (`MatchArm`, `SelectClause`) that reference `CoreExpr`.

use serde::{Deserialize, Serialize};

use super::primitives::{LiteralValue, LoopTermination};

// ── MatchArm ──────────────────────────────────────────────────────────────

/// One arm of a `CoreExpr::Match` expression.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchArm {
    /// Pattern string (e.g. `"Ok(x)"`, `"None"`, `"_"`). Backend execution
    /// currently supports integer literals, boolean literals, and wildcard;
    /// constructor payload strings are syntax-only until payload bindings are
    /// represented in Core/ANF.
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

    // ── ola4-type-formalism: dynamic dispatch through Dyn<Interface> ──────
    /// Explicit dynamic dispatch through a `Dyn<Interface>` typed value.
    ///
    /// Distinct from `Dispatch` (which targets handler/capability tables).
    /// `DynCall` is interface-typed: the `interface` field names the interface
    /// (e.g., `"Repository<User>"`), `method` names the operation, and `args`
    /// are the call arguments.
    ///
    /// Backward compat: absent from the wire format before ola4-type-formalism;
    /// pre-existing `CoreNode`s decode with `expr = None` as before.
    DynCall {
        /// Interface name (e.g., `"Repository<User>"`).
        interface: String,
        /// Method name on the interface (e.g., `"get"`).
        method: String,
        /// Call arguments — may be non-atomic; atomized during ANF lowering.
        args: Vec<CoreExpr>,
    },

    // ── doc-alignment: missing CoreExpr variants from core-ir.md ──────────
    /// Use of a declared capability.
    ///
    /// Corresponds to `docs/core-ir.md §9 — CapabilityUse`.
    /// `capability` names the capability being used (e.g., `"database.read"`).
    CapabilityUse {
        /// Capability name.
        capability: String,
        /// Arguments to the capability operation.
        args: Vec<CoreExpr>,
    },

    /// Resource use expression — access a resource within a scope.
    ///
    /// Corresponds to `docs/core-ir.md §11 — Use`.
    ResourceUse {
        /// The resource handle expression.
        handle: Box<CoreExpr>,
        /// The body that uses the resource.
        body: Box<CoreExpr>,
    },

    /// Scoped resource usage — acquire, use, and release within a block.
    ///
    /// Corresponds to `docs/core-ir.md §11 — Using`.
    ResourceUsing {
        /// The resource acquisition expression.
        resource: Box<CoreExpr>,
        /// Name to bind the acquired handle to.
        binding: String,
        /// Body expression with the resource in scope.
        body: Box<CoreExpr>,
    },

    /// Transfer ownership of a resource handle to another scope.
    ///
    /// Corresponds to `docs/core-ir.md §11 — Transfer`.
    ResourceTransfer {
        /// The resource handle to transfer.
        handle: Box<CoreExpr>,
        /// The target scope or recipient expression.
        target: Box<CoreExpr>,
    },

    /// Foreign function call — invocation of an externally-defined function.
    ///
    /// Corresponds to `docs/core-ir.md §13 — ForeignFunction`.
    ForeignFunctionCall {
        /// Fully qualified foreign function name.
        func: String,
        /// Call arguments.
        args: Vec<CoreExpr>,
    },

    /// Construct a `PatchField<T>` value — one of `Unchanged`, `Set(T)`, or `Clear`.
    ///
    /// Corresponds to `docs/core-ir.md` PatchField construction.
    PatchFieldConstruct {
        /// The PatchField state: `"Unchanged"`, `"Set"`, or `"Clear"`.
        state: String,
        /// Optional payload (present only for the `Set` state).
        value: Option<Box<CoreExpr>>,
    },

    /// Pattern match on a `PatchField<T>` value.
    ///
    /// Corresponds to `docs/core-ir.md` PatchField matching.
    PatchFieldMatch {
        /// The PatchField expression to match on.
        scrutinee: Box<CoreExpr>,
        /// Arms: each MatchArm pattern is `"Unchanged"`, `"Set(x)"`, or `"Clear"`.
        arms: Vec<MatchArm>,
    },
}
