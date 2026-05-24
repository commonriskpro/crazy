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

use ail_core::semantic_graph::{
    BlockRef, ContractRef, EffectRef, NodeRef, ProofObligationRef, RuntimeCheckRef,
};
use serde::{Deserialize, Serialize};

use crate::core_ir::{LiteralValue, StageHashes};
use crate::error::CompileError;

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
    /// Exit the nearest enclosing loop with a value.
    Break { value: Box<AnfExpr> },
    /// Continue at the nearest enclosing loop header.
    Continue,
    /// Structured while loop with an atomic condition name.
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
mod tests {
    use super::*;
    use crate::core_ir::{LiteralValue, StageHashes};
    use crate::hash::stable_cbor_bytes;
    #[allow(unused_imports)]
    use ciborium;

    // ── AnfExpr construction ──────────────────────────────────────────────

    // All AnfExpr variants are constructible without panic.
    #[test]
    fn all_anf_expr_variants_are_constructible() {
        let _lit = AnfExpr::Literal(LiteralValue::Int(42));
        let _var = AnfExpr::Var("x".to_string());
        let _let = AnfExpr::Let {
            name: "y".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
            body: Box::new(AnfExpr::Var("y".to_string())),
        };
        let _if = AnfExpr::If {
            cond: "flag".to_string(),
            then_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            else_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
        };
        let _call = AnfExpr::Call {
            func: "fn.add".to_string(),
            args: vec!["a".to_string(), "b".to_string()],
        };
        let _fg = AnfExpr::FieldGet {
            record: "order".to_string(),
            field: "total".to_string(),
        };
        let _ret = AnfExpr::Return(Box::new(AnfExpr::Var("result".to_string())));
        let _seq = AnfExpr::Seq(vec![
            AnfExpr::Call {
                func: "db.write".to_string(),
                args: vec!["order".to_string()],
            },
            AnfExpr::Literal(LiteralValue::Unit),
        ]);
        let _placeholder = AnfExpr::Placeholder;
        // G20 variants
        let _match = AnfExpr::Match {
            scrutinee: "v".to_string(),
            arms: vec![AnfMatchArm {
                pattern: "Some(x)".to_string(),
                body: AnfExpr::Var("x".to_string()),
            }],
        };
        let _lambda = AnfExpr::Lambda {
            params: vec!["x".to_string()],
            captures: vec![],
            body: Box::new(AnfExpr::Var("x".to_string())),
        };
        let _record = AnfExpr::RecordNew {
            fields: vec![(
                "amount".to_string(),
                AnfExpr::Literal(LiteralValue::Int(10)),
            )],
        };
        let _field_update = AnfExpr::FieldUpdate {
            record: "order".to_string(),
            field: "status".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Text("Paid".to_string()))),
        };
        let _tuple = AnfExpr::TupleNew(vec![
            AnfExpr::Var("a".to_string()),
            AnfExpr::Var("b".to_string()),
        ]);
        let _variant = AnfExpr::VariantNew {
            tag: "Ok".to_string(),
            payload: Some(Box::new(AnfExpr::Var("x".to_string()))),
        };
        let _list = AnfExpr::ListNew(vec![AnfExpr::Literal(LiteralValue::Int(1))]);
        let _loop = AnfExpr::Loop {
            body: Box::new(AnfExpr::Break {
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(10))),
            }),
        };
        let _break = AnfExpr::Break {
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
        };
        let _continue = AnfExpr::Continue;
        let _while_loop = AnfExpr::WhileLoop {
            cond: "flag".to_string(),
            body: Box::new(AnfExpr::Continue),
        };
    }

    // G23: AnfSelectClause is constructible with correct fields.
    #[test]
    fn anf_select_clause_is_constructible() {
        let clause = AnfSelectClause {
            channel: "inbox".to_string(),
            binding: "msg".to_string(),
            body: AnfExpr::Var("msg".to_string()),
        };
        assert_eq!(clause.channel, "inbox");
        assert_eq!(clause.binding, "msg");
        assert_eq!(clause.body, AnfExpr::Var("msg".to_string()));
    }

    // G23: all new concurrency + cell AnfExpr variants are constructible.
    #[test]
    fn all_new_concurrency_cell_anf_expr_variants_are_constructible() {
        let _task_await = AnfExpr::TaskAwait {
            task: "t".to_string(),
        };
        let _task_cancel = AnfExpr::TaskCancel {
            task: "t".to_string(),
        };
        let _task_group = AnfExpr::TaskGroup {
            body: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
        };
        let _channel_new_unbounded = AnfExpr::ChannelNew { capacity: None };
        let _channel_new_bounded = AnfExpr::ChannelNew { capacity: Some(32) };
        let _select = AnfExpr::Select {
            branches: vec![AnfSelectClause {
                channel: "ch".to_string(),
                binding: "v".to_string(),
                body: AnfExpr::Var("v".to_string()),
            }],
        };
        let _timeout = AnfExpr::Timeout {
            duration: "dur".to_string(),
            body: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
        };
        let _cell_new = AnfExpr::CellNew {
            init: "zero".to_string(),
        };
        let _cell_get = AnfExpr::CellGet {
            cell: "c".to_string(),
        };
        let _cell_set = AnfExpr::CellSet {
            cell: "c".to_string(),
            value: "v".to_string(),
        };
        // All constructed without panic — test passes.
    }

    // TRIANGULATE: channel operands are atomic strings (not nested exprs).
    #[test]
    fn anf_task_await_task_is_atomic_string() {
        let expr = AnfExpr::TaskAwait {
            task: "task_0".to_string(),
        };
        if let AnfExpr::TaskAwait { task } = expr {
            assert_eq!(task, "task_0");
        } else {
            panic!("expected TaskAwait");
        }
    }

    // G23: new concurrency + cell variants CBOR round-trip.
    #[test]
    fn new_concurrency_cell_anf_variants_cbor_round_trip() {
        use crate::hash::stable_cbor_bytes;
        let variants: Vec<AnfExpr> = vec![
            AnfExpr::TaskAwait {
                task: "t".to_string(),
            },
            AnfExpr::TaskCancel {
                task: "t".to_string(),
            },
            AnfExpr::TaskGroup {
                body: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
            },
            AnfExpr::ChannelNew { capacity: None },
            AnfExpr::ChannelNew { capacity: Some(4) },
            AnfExpr::Select {
                branches: vec![AnfSelectClause {
                    channel: "ch".to_string(),
                    binding: "v".to_string(),
                    body: AnfExpr::Var("v".to_string()),
                }],
            },
            AnfExpr::Timeout {
                duration: "d".to_string(),
                body: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
            },
            AnfExpr::CellNew {
                init: "zero".to_string(),
            },
            AnfExpr::CellGet {
                cell: "c".to_string(),
            },
            AnfExpr::CellSet {
                cell: "c".to_string(),
                value: "v".to_string(),
            },
        ];
        for expr in &variants {
            let bytes = stable_cbor_bytes(expr).expect("encode must succeed");
            let decoded: AnfExpr =
                ciborium::from_reader(bytes.as_slice()).expect("decode must succeed");
            assert_eq!(&decoded, expr, "AnfExpr must survive CBOR round-trip");
        }
    }

    // G20: AnfMatchArm is constructible and has correct fields.
    #[test]
    fn anf_match_arm_is_constructible() {
        let arm = AnfMatchArm {
            pattern: "None".to_string(),
            body: AnfExpr::Literal(LiteralValue::Unit),
        };
        assert_eq!(arm.pattern, "None");
        assert_eq!(arm.body, AnfExpr::Literal(LiteralValue::Unit));
    }

    // G20: AnfExpr::Match — scrutinee is a String (atomic name).
    #[test]
    fn anf_match_scrutinee_is_atomic_string() {
        let expr = AnfExpr::Match {
            scrutinee: "payment".to_string(),
            arms: vec![AnfMatchArm {
                pattern: "Ok(r)".to_string(),
                body: AnfExpr::Var("r".to_string()),
            }],
        };
        if let AnfExpr::Match { scrutinee, arms } = &expr {
            assert_eq!(scrutinee, "payment");
            assert_eq!(arms.len(), 1);
            assert_eq!(arms[0].pattern, "Ok(r)");
        } else {
            panic!("expected Match variant");
        }
    }

    // G20: AnfExpr::Match CBOR round-trip.
    #[test]
    fn anf_match_cbor_round_trip() {
        let expr = AnfExpr::Match {
            scrutinee: "result".to_string(),
            arms: vec![
                AnfMatchArm {
                    pattern: "Ok(v)".to_string(),
                    body: AnfExpr::Var("v".to_string()),
                },
                AnfMatchArm {
                    pattern: "Err(e)".to_string(),
                    body: AnfExpr::Var("e".to_string()),
                },
            ],
        };
        let bytes = stable_cbor_bytes(&expr).expect("encode");
        let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(decoded, expr, "AnfExpr::Match must survive CBOR round-trip");
    }

    // G20: AnfExpr::Lambda — params and body are correct.
    #[test]
    fn anf_lambda_fields_are_correct() {
        let expr = AnfExpr::Lambda {
            params: vec!["x".to_string(), "y".to_string()],
            captures: vec![],
            body: Box::new(AnfExpr::Var("x".to_string())),
        };
        if let AnfExpr::Lambda {
            params,
            body,
            captures,
        } = &expr
        {
            assert_eq!(params, &["x", "y"]);
            assert!(captures.is_empty());
            assert_eq!(**body, AnfExpr::Var("x".to_string()));
        } else {
            panic!("expected Lambda variant");
        }
    }

    // G20: AnfExpr::Lambda CBOR round-trip.
    #[test]
    fn anf_lambda_cbor_round_trip() {
        let expr = AnfExpr::Lambda {
            params: vec!["a".to_string()],
            captures: vec![],
            body: Box::new(AnfExpr::Literal(LiteralValue::Int(42))),
        };
        let bytes = stable_cbor_bytes(&expr).expect("encode");
        let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(
            decoded, expr,
            "AnfExpr::Lambda must survive CBOR round-trip"
        );
    }

    // G20: AnfExpr::RecordNew CBOR round-trip.
    #[test]
    fn anf_record_new_cbor_round_trip() {
        let expr = AnfExpr::RecordNew {
            fields: vec![
                (
                    "name".to_string(),
                    AnfExpr::Literal(LiteralValue::Text("Alice".to_string())),
                ),
                ("age".to_string(), AnfExpr::Literal(LiteralValue::Int(30))),
            ],
        };
        let bytes = stable_cbor_bytes(&expr).expect("encode");
        let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(
            decoded, expr,
            "AnfExpr::RecordNew must survive CBOR round-trip"
        );
    }

    // G20: AnfExpr::FieldUpdate — record is an atomic String.
    #[test]
    fn anf_field_update_record_is_atomic_string() {
        let expr = AnfExpr::FieldUpdate {
            record: "order".to_string(),
            field: "status".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Text("Paid".to_string()))),
        };
        if let AnfExpr::FieldUpdate { record, field, .. } = &expr {
            assert_eq!(record, "order");
            assert_eq!(field, "status");
        } else {
            panic!("expected FieldUpdate variant");
        }
    }

    // G20: AnfExpr::FieldUpdate CBOR round-trip.
    #[test]
    fn anf_field_update_cbor_round_trip() {
        let expr = AnfExpr::FieldUpdate {
            record: "rec".to_string(),
            field: "x".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(99))),
        };
        let bytes = stable_cbor_bytes(&expr).expect("encode");
        let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(
            decoded, expr,
            "AnfExpr::FieldUpdate must survive CBOR round-trip"
        );
    }

    // G20: AnfExpr::TupleNew CBOR round-trip.
    #[test]
    fn anf_tuple_new_cbor_round_trip() {
        let expr = AnfExpr::TupleNew(vec![
            AnfExpr::Literal(LiteralValue::Int(1)),
            AnfExpr::Literal(LiteralValue::Bool(false)),
        ]);
        let bytes = stable_cbor_bytes(&expr).expect("encode");
        let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(
            decoded, expr,
            "AnfExpr::TupleNew must survive CBOR round-trip"
        );
    }

    // G20: AnfExpr::VariantNew with payload CBOR round-trip.
    #[test]
    fn anf_variant_new_with_payload_cbor_round_trip() {
        let expr = AnfExpr::VariantNew {
            tag: "Some".to_string(),
            payload: Some(Box::new(AnfExpr::Literal(LiteralValue::Int(42)))),
        };
        let bytes = stable_cbor_bytes(&expr).expect("encode");
        let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(
            decoded, expr,
            "AnfExpr::VariantNew with payload must survive CBOR round-trip"
        );
    }

    // G20: AnfExpr::VariantNew without payload CBOR round-trip.
    #[test]
    fn anf_variant_new_no_payload_cbor_round_trip() {
        let expr = AnfExpr::VariantNew {
            tag: "None".to_string(),
            payload: None,
        };
        let bytes = stable_cbor_bytes(&expr).expect("encode");
        let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(
            decoded, expr,
            "AnfExpr::VariantNew without payload must survive CBOR round-trip"
        );
    }

    // G20: AnfExpr::ListNew CBOR round-trip.
    #[test]
    fn anf_list_new_cbor_round_trip() {
        let expr = AnfExpr::ListNew(vec![
            AnfExpr::Literal(LiteralValue::Int(1)),
            AnfExpr::Literal(LiteralValue::Int(2)),
            AnfExpr::Literal(LiteralValue::Int(3)),
        ]);
        let bytes = stable_cbor_bytes(&expr).expect("encode");
        let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(
            decoded, expr,
            "AnfExpr::ListNew must survive CBOR round-trip"
        );
    }

    // If.cond is a String (atomic), not a nested AnfExpr.
    #[test]
    fn anf_if_cond_is_atomic_string() {
        let expr = AnfExpr::If {
            cond: "my_flag".to_string(),
            then_branch: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
            else_branch: Box::new(AnfExpr::Literal(LiteralValue::Bool(false))),
        };
        if let AnfExpr::If { cond, .. } = expr {
            assert_eq!(cond, "my_flag");
        } else {
            panic!("expected If variant");
        }
    }

    // Call.args are Vec<String> (atomic names), not nested expressions.
    #[test]
    fn anf_call_args_are_atomic_strings() {
        let expr = AnfExpr::Call {
            func: "fn.checkout".to_string(),
            args: vec!["cart_id".to_string(), "user_id".to_string()],
        };
        if let AnfExpr::Call { func, args } = expr {
            assert_eq!(func, "fn.checkout");
            assert_eq!(args, vec!["cart_id", "user_id"]);
        } else {
            panic!("expected Call variant");
        }
    }

    // ── AnfBinding ────────────────────────────────────────────────────────

    // Scenario: AnfBinding preserves its source_ref provenance.
    // Spec: "every AnfBinding.source_ref matches origin NodeRef"
    #[test]
    fn anf_binding_preserves_source_ref() {
        let binding = AnfBinding {
            source_ref: NodeRef(7),
            name: "fn_x".to_string(),
            expr: AnfExpr::Literal(LiteralValue::Unit),
        };
        assert_eq!(
            binding.source_ref,
            NodeRef(7),
            "source_ref must be preserved verbatim"
        );
    }

    // Scenario: AnfBinding with Let expr is constructible.
    #[test]
    fn anf_binding_with_let_expr() {
        let binding = AnfBinding {
            source_ref: NodeRef(5),
            name: "fn_checkout".to_string(),
            expr: AnfExpr::Let {
                name: "cart".to_string(),
                value: Box::new(AnfExpr::Call {
                    func: "db.read".to_string(),
                    args: vec!["cart_id".to_string()],
                }),
                body: Box::new(AnfExpr::Var("cart".to_string())),
            },
        };
        assert_eq!(binding.source_ref, NodeRef(5));
        assert_eq!(binding.name, "fn_checkout");
    }

    // Scenario: AnfIr is constructible with bindings and stage hashes.
    #[test]
    fn anf_ir_is_constructible() {
        let bindings = vec![
            AnfBinding {
                source_ref: NodeRef(0),
                name: "mod_root".to_string(),
                expr: AnfExpr::Literal(LiteralValue::Unit),
            },
            AnfBinding {
                source_ref: NodeRef(1),
                name: "fn_main".to_string(),
                expr: AnfExpr::Placeholder,
            },
        ];
        let source_map = crate::anf::SourceMap::from_bindings(&bindings);
        let ir = AnfIr {
            schema_version: crate::anf::ANF_SCHEMA_VERSION,
            bindings,
            source_map,
            stage_hashes: StageHashes {
                graph_snapshot_hash: [0u8; 32],
                verification_report_hash: [0u8; 32],
                core_ir_hash: [1u8; 32],
                anf_ir_hash: Some([2u8; 32]),
                wasm_hash: None,
                native_hash: None,
                source_map_hash: None,
                artifact_manifest_hash: None,
            },
        };
        assert_eq!(ir.bindings.len(), 2);
        assert!(ir.stage_hashes.anf_ir_hash.is_some());
    }

    // TRIANGULATE: stable_cbor_bytes on Vec<AnfBinding> is deterministic.
    #[test]
    fn anf_binding_list_cbor_is_deterministic() {
        let bindings = vec![
            AnfBinding {
                source_ref: NodeRef(0),
                name: "a".to_string(),
                expr: AnfExpr::Literal(LiteralValue::Int(1)),
            },
            AnfBinding {
                source_ref: NodeRef(1),
                name: "b".to_string(),
                expr: AnfExpr::Var("a".to_string()),
            },
            AnfBinding {
                source_ref: NodeRef(2),
                name: "c".to_string(),
                expr: AnfExpr::Placeholder,
            },
        ];
        let b1 = stable_cbor_bytes(&bindings).expect("first encode");
        let b2 = stable_cbor_bytes(&bindings).expect("second encode");
        assert_eq!(b1, b2, "Vec<AnfBinding> must produce identical CBOR bytes");
    }

    // TRIANGULATE: different binding lists produce different CBOR bytes.
    #[test]
    fn different_anf_binding_lists_produce_different_cbor() {
        let list_a = vec![AnfBinding {
            source_ref: NodeRef(0),
            name: "x".to_string(),
            expr: AnfExpr::Literal(LiteralValue::Int(1)),
        }];
        let list_b = vec![AnfBinding {
            source_ref: NodeRef(0),
            name: "x".to_string(),
            expr: AnfExpr::Literal(LiteralValue::Int(2)),
        }];
        let b_a = stable_cbor_bytes(&list_a).expect("encode a");
        let b_b = stable_cbor_bytes(&list_b).expect("encode b");
        assert_ne!(
            b_a, b_b,
            "different AnfBinding lists must produce different CBOR"
        );
    }

    // ── G32: SourceMapEntry typed ref fields ──────────────────────────────

    // Spec: SourceMapEntry uses typed ref newtypes for provenance fields.
    // RED: written after types exist; GREEN from the start of this change.
    #[test]
    fn source_map_entry_with_typed_refs_is_constructible() {
        use ail_core::semantic_graph::{
            BlockRef, ContractRef, EffectRef, ProofObligationRef, RuntimeCheckRef,
        };

        let entry = SourceMapEntry {
            binding_name: "fn_checkout".to_string(),
            node_id: NodeRef(0),
            block_ref: Some(BlockRef("block_checkout".to_string())),
            change_set: Some("change.add_checkout".to_string()),
            contract_ref: Some(ContractRef("contract.payment".to_string())),
            effect_ref: Some(EffectRef("effect.db.read".to_string())),
            proof_obligation_ref: Some(ProofObligationRef("proof.no_negative_balance".to_string())),
            runtime_check_ref: Some(RuntimeCheckRef("rtcheck.null_guard".to_string())),
            wasm_offset: None,
            native_offset: None,
        };
        assert_eq!(entry.block_ref.as_ref().unwrap().0, "block_checkout");
        assert_eq!(entry.contract_ref.as_ref().unwrap().0, "contract.payment");
        assert_eq!(entry.effect_ref.as_ref().unwrap().0, "effect.db.read");
        assert_eq!(
            entry.proof_obligation_ref.as_ref().unwrap().0,
            "proof.no_negative_balance"
        );
        assert_eq!(
            entry.runtime_check_ref.as_ref().unwrap().0,
            "rtcheck.null_guard"
        );
    }

    // TRIANGULATE: SourceMapEntry with typed refs survives CBOR round-trip.
    #[test]
    fn source_map_entry_typed_refs_cbor_round_trip() {
        use ail_core::semantic_graph::{BlockRef, ContractRef};

        let entry = SourceMapEntry {
            binding_name: "fn_pay".to_string(),
            node_id: NodeRef(3),
            block_ref: Some(BlockRef("block_pay".to_string())),
            change_set: Some("change.add_payment".to_string()),
            contract_ref: Some(ContractRef("contract.payment.verify".to_string())),
            effect_ref: None,
            proof_obligation_ref: None,
            runtime_check_ref: None,
            wasm_offset: None,
            native_offset: None,
        };
        let bytes = stable_cbor_bytes(&entry).expect("encode");
        let decoded: SourceMapEntry = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(
            decoded, entry,
            "SourceMapEntry with typed refs must survive CBOR round-trip"
        );
    }

    // Spec: SourceMap from_bindings builds entries with None for all optional fields.
    #[test]
    fn source_map_from_bindings_sets_all_optional_fields_to_none() {
        let bindings = vec![
            AnfBinding {
                source_ref: NodeRef(0),
                name: "fn_a".to_string(),
                expr: AnfExpr::Placeholder,
            },
            AnfBinding {
                source_ref: NodeRef(1),
                name: "fn_b".to_string(),
                expr: AnfExpr::Placeholder,
            },
        ];
        let sm = SourceMap::from_bindings(&bindings);
        assert_eq!(sm.entries.len(), 2);
        for entry in &sm.entries {
            assert!(
                entry.block_ref.is_none(),
                "block_ref must be None from from_bindings"
            );
            assert!(
                entry.change_set.is_none(),
                "change_set must be None from from_bindings"
            );
            assert!(
                entry.contract_ref.is_none(),
                "contract_ref must be None from from_bindings"
            );
            assert!(
                entry.effect_ref.is_none(),
                "effect_ref must be None from from_bindings"
            );
            assert!(
                entry.proof_obligation_ref.is_none(),
                "proof_obligation_ref must be None from from_bindings"
            );
            assert!(
                entry.runtime_check_ref.is_none(),
                "runtime_check_ref must be None from from_bindings"
            );
            assert!(
                entry.wasm_offset.is_none(),
                "wasm_offset must be None from from_bindings"
            );
            assert!(
                entry.native_offset.is_none(),
                "native_offset must be None from from_bindings"
            );
        }
    }

    // Spec: source_map has one entry per binding (including synthetic ones).
    #[test]
    fn source_map_preserves_duplicate_node_refs_for_synthetic_bindings() {
        // Two bindings with the same source_ref simulate G20 synthetic expansion.
        let bindings = vec![
            AnfBinding {
                source_ref: NodeRef(5),
                name: "fn_x".to_string(),
                expr: AnfExpr::Placeholder,
            },
            AnfBinding {
                source_ref: NodeRef(5), // duplicate NodeRef (synthetic)
                name: "anf_0".to_string(),
                expr: AnfExpr::Placeholder,
            },
        ];
        let sm = SourceMap::from_bindings(&bindings);
        assert_eq!(
            sm.entries.len(),
            2,
            "duplicate NodeRefs must NOT be collapsed"
        );
        assert_eq!(sm.entries[0].node_id, NodeRef(5));
        assert_eq!(sm.entries[1].node_id, NodeRef(5));
        assert_eq!(sm.entries[0].binding_name, "fn_x");
        assert_eq!(sm.entries[1].binding_name, "anf_0");
    }

    // Spec: empty input yields empty source map.
    #[test]
    fn source_map_from_empty_bindings_is_empty() {
        let sm = SourceMap::from_bindings(&[]);
        assert!(
            sm.entries.is_empty(),
            "empty bindings must produce empty source map"
        );
    }

    // Scenario: source_ref is not dropped when name is the same.
    #[test]
    fn anf_binding_distinct_refs_are_not_equal() {
        let b1 = AnfBinding {
            source_ref: NodeRef(3),
            name: "shared_name".to_string(),
            expr: AnfExpr::Placeholder,
        };
        let b2 = AnfBinding {
            source_ref: NodeRef(4),
            name: "shared_name".to_string(),
            expr: AnfExpr::Placeholder,
        };
        assert_ne!(b1, b2, "bindings with different NodeRefs must not be equal");
    }

    // S11: CBOR round-trip for AnfBinding with Let expr is lossless.
    #[test]
    fn anf_binding_cbor_round_trip_with_let_expr() {
        let binding = AnfBinding {
            source_ref: NodeRef(9),
            name: "fn_round_trip".to_string(),
            expr: AnfExpr::Let {
                name: "tmp".to_string(),
                value: Box::new(AnfExpr::Call {
                    func: "fn.add".to_string(),
                    args: vec!["x".to_string(), "y".to_string()],
                }),
                body: Box::new(AnfExpr::Var("tmp".to_string())),
            },
        };
        let bytes = stable_cbor_bytes(&binding).expect("encode must succeed");
        let decoded: AnfBinding =
            ciborium::from_reader(bytes.as_slice()).expect("decode must succeed");
        assert_eq!(
            decoded, binding,
            "AnfBinding with Let expr must survive CBOR round-trip"
        );
    }
}
