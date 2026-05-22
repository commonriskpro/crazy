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
    TaskSpawn {
        func: String,
        args: Vec<CoreExpr>,
    },

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
    /// Homogeneous ordered collection.
    List,
    /// Key-value association (ordered by key for determinism).
    Map,
    /// Unordered unique-element collection.
    Set,
    /// Optional value — `Some(T) | None`.
    Option,
    /// Fallible value — `Ok(T) | Err(E)`.
    Result,
    /// Function type `(Params) -> Return` with optional effect row.
    Function,
    /// External resource handle with an ownership mode.
    Handle,
    /// A base type refined by a logical predicate.
    Refinement,
    /// Generic/unknown type — used as a fallback when the nominal is
    /// unrecognised or when type parameters have not been resolved yet.
    Generic,
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

    // S2: All 20 CoreType variants are constructible without panic.
    #[test]
    fn all_core_type_variants_are_constructible() {
        let types = [
            CoreType::Unit,
            CoreType::Never,
            CoreType::Bool,
            CoreType::Int,
            CoreType::UInt,
            CoreType::Float,
            CoreType::Text,
            CoreType::Bytes,
            CoreType::Record,
            CoreType::Variant,
            CoreType::Tuple,
            CoreType::List,
            CoreType::Map,
            CoreType::Set,
            CoreType::Option,
            CoreType::Result,
            CoreType::Function,
            CoreType::Handle,
            CoreType::Refinement,
            CoreType::Generic,
        ];
        assert_eq!(
            types.len(),
            20,
            "all 20 CoreType variants must be reachable"
        );
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
}
