// ── ail-compiler::lower ───────────────────────────────────────────────────
//
// Pipeline lowering functions: SemanticGraph → CoreIr → AnfIr.
//
// # Pre-condition (both functions)
//
// `lower_to_core_ir` MUST be called with a `VerificationReport` whose
// `summary()` is `Proven` or `RuntimeChecked`.  Any other summary causes
// `Err(CompileError::RejectedReport)` to be returned immediately.
//
// # Hash chain contract
//
// - `graph_snapshot_hash    = blake3(graph_cbor_bytes)`
// - `verification_report_hash = blake3(report_cbor_bytes)`
// - `core_ir_hash           = blake3(graph_snapshot_hash || core_ir_bytes)`
// - `anf_ir_hash            = blake3(core_ir_hash || anf_ir_bytes)`
//
// # Determinism contract
//
// All collections are `Vec` / `BTreeMap` — never `HashMap`.
// `stable_cbor_bytes` + BLAKE3 gives byte-identical output across runs.

use std::collections::BTreeMap;

use ail_core::semantic_graph::{
    BlockRef, ContractRef, EffectRef, GraphValidationError, NodeKind, NodeRef, ProofObligationRef,
    RuntimeCheckRef, SemanticGraph,
};
use ail_verify::report::{VerificationReport, VerificationState};

use crate::anf::{ANF_SCHEMA_VERSION, AnfBinding, AnfExpr, AnfIr, SourceMap, SourceMapEntry};
use crate::core_ir::{
    CoreExpr, CoreIr, CoreNode, CoreNodeKind, CoreType, LiteralValue, ResourceMode, StageHashes,
};
use crate::error::CompileError;
use crate::expr_parser::parse_expr;
use crate::hash::{hash_with_parent, stable_cbor_bytes};
use crate::optimize::optimize_bindings;

// ── is_report_accepted ────────────────────────────────────────────────────

/// Return `true` if `report.summary()` is `Proven` or `RuntimeChecked`.
///
/// All other summary states (`Assumed`, `Unverified`, `Unsafe`, `Failed`)
/// are treated as rejected.  This is a local policy helper for Phase 7;
/// a future top-level `VerificationReport.status` field should replace it.
pub fn is_report_accepted(report: &VerificationReport) -> bool {
    matches!(
        report.summary(),
        VerificationState::Proven | VerificationState::RuntimeChecked
    )
}

// ── map_node_kind ─────────────────────────────────────────────────────────

/// Map a `NodeKind` (source graph) to its `CoreNodeKind` counterpart.
///
/// In Phase 7 the two enums are structurally identical; the mapping is kept
/// explicit so the compiler IR can diverge from the source model in future
/// phases without a breaking change here.
pub(crate) fn map_node_kind(kind: NodeKind) -> CoreNodeKind {
    match kind {
        NodeKind::Module => CoreNodeKind::Module,
        NodeKind::Function => CoreNodeKind::Function,
        NodeKind::Type => CoreNodeKind::Type,
        NodeKind::Effect => CoreNodeKind::Effect,
        NodeKind::Capability => CoreNodeKind::Capability,
        NodeKind::Contract => CoreNodeKind::Contract,
        NodeKind::Invariant => CoreNodeKind::Invariant,
        NodeKind::Test => CoreNodeKind::Test,
        NodeKind::Boundary => CoreNodeKind::Boundary,
        NodeKind::Package => CoreNodeKind::Package,
        NodeKind::Interface => CoreNodeKind::Interface,
        NodeKind::Impl => CoreNodeKind::Impl,
        NodeKind::EffectAlias => CoreNodeKind::EffectAlias,
        NodeKind::Import => CoreNodeKind::Import,
        NodeKind::Export => CoreNodeKind::Export,
        NodeKind::VersionConstraint => CoreNodeKind::VersionConstraint,
        NodeKind::CapabilityExport => CoreNodeKind::CapabilityExport,
        NodeKind::ContractExport => CoreNodeKind::ContractExport,
    }
}

// ── atomize ───────────────────────────────────────────────────────────────

/// Ensure `expr` is atomic (a variable name).
///
/// If `expr` is already `CoreExpr::Var(n)`, returns `n` without emitting any
/// binding.  Otherwise lowers `expr` to an `AnfExpr`, pushes a synthetic
/// `AnfBinding` with a fresh name, and returns that fresh name.
///
/// The pushed binding carries the same `source_ref` as the enclosing node
/// (provenance is preserved for synthetic temporaries).
fn atomize(
    expr: &CoreExpr,
    fresh: &mut u32,
    source_ref: ail_core::semantic_graph::NodeRef,
    out: &mut Vec<AnfBinding>,
) -> String {
    if let CoreExpr::Var(name) = expr {
        return name.clone();
    }
    let anf_expr = lower_core_expr_to_anf(expr, fresh, source_ref, out);
    let name = format!("anf_{}", *fresh);
    *fresh += 1;
    out.push(AnfBinding {
        source_ref,
        name: name.clone(),
        expr: anf_expr,
    });
    name
}

fn atomize_local(
    expr: &CoreExpr,
    fresh: &mut u32,
    source_ref: ail_core::semantic_graph::NodeRef,
) -> (String, Option<(String, AnfExpr)>) {
    if let CoreExpr::Var(name) = expr {
        return (name.clone(), None);
    }
    let value = lower_core_expr_to_anf_local(expr, fresh, source_ref);
    let name = format!("anf_{}", *fresh);
    *fresh += 1;
    (name.clone(), Some((name, value)))
}

fn wrap_local_bindings(mut bindings: Vec<(String, AnfExpr)>, body: AnfExpr) -> AnfExpr {
    bindings.reverse();
    bindings
        .into_iter()
        .fold(body, |body, (name, value)| AnfExpr::Let {
            name,
            value: Box::new(value),
            body: Box::new(body),
        })
}

fn lower_core_call_to_anf(
    func: &str,
    args: &[CoreExpr],
    fresh: &mut u32,
    source_ref: ail_core::semantic_graph::NodeRef,
) -> AnfExpr {
    let mut bindings = Vec::new();
    let mut arg_names = Vec::with_capacity(args.len());
    for arg in args {
        let (name, binding) = atomize_local(arg, fresh, source_ref);
        if let Some(binding) = binding {
            bindings.push(binding);
        }
        arg_names.push(name);
    }
    wrap_local_bindings(
        bindings,
        AnfExpr::Call {
            func: func.to_string(),
            args: arg_names,
        },
    )
}

fn lower_core_binary_to_anf(
    func: &str,
    left: &CoreExpr,
    right: &CoreExpr,
    fresh: &mut u32,
    source_ref: ail_core::semantic_graph::NodeRef,
) -> AnfExpr {
    lower_core_call_to_anf(func, &[left.clone(), right.clone()], fresh, source_ref)
}

fn lower_core_expr_to_anf_local(
    expr: &CoreExpr,
    fresh: &mut u32,
    source_ref: ail_core::semantic_graph::NodeRef,
) -> AnfExpr {
    match expr {
        CoreExpr::Let { name, value, body } => AnfExpr::Let {
            name: name.clone(),
            value: Box::new(lower_core_expr_to_anf_local(value, fresh, source_ref)),
            body: Box::new(lower_core_expr_to_anf_local(body, fresh, source_ref)),
        },
        CoreExpr::If { cond, then_, else_ } => {
            let (cond_name, cond_binding) = atomize_local(cond, fresh, source_ref);
            let if_expr = AnfExpr::If {
                cond: cond_name,
                then_branch: Box::new(lower_core_expr_to_anf_local(then_, fresh, source_ref)),
                else_branch: Box::new(lower_core_expr_to_anf_local(else_, fresh, source_ref)),
            };
            if let Some(binding) = cond_binding {
                wrap_local_bindings(vec![binding], if_expr)
            } else {
                if_expr
            }
        }
        CoreExpr::Call { func, args } => lower_core_call_to_anf(func, args, fresh, source_ref),
        CoreExpr::Add(left, right) => {
            lower_core_binary_to_anf("add", left, right, fresh, source_ref)
        }
        CoreExpr::Sub(left, right) => {
            lower_core_binary_to_anf("sub", left, right, fresh, source_ref)
        }
        CoreExpr::Mul(left, right) => {
            lower_core_binary_to_anf("mul", left, right, fresh, source_ref)
        }
        CoreExpr::Div(left, right) => {
            lower_core_binary_to_anf("div", left, right, fresh, source_ref)
        }
        CoreExpr::Mod(left, right) => {
            lower_core_binary_to_anf("mod", left, right, fresh, source_ref)
        }
        CoreExpr::Eq(left, right) => lower_core_binary_to_anf("eq", left, right, fresh, source_ref),
        CoreExpr::Lt(left, right) => lower_core_binary_to_anf("lt", left, right, fresh, source_ref),
        CoreExpr::Gt(left, right) => lower_core_binary_to_anf("gt", left, right, fresh, source_ref),
        CoreExpr::And { left, right } => {
            let (left_name, left_binding) = atomize_local(left, fresh, source_ref);
            let and_expr = AnfExpr::ShortCircuitAnd {
                left: left_name,
                right: Box::new(lower_core_expr_to_anf_local(right, fresh, source_ref)),
            };
            if let Some(binding) = left_binding {
                wrap_local_bindings(vec![binding], and_expr)
            } else {
                and_expr
            }
        }
        CoreExpr::Or { left, right } => {
            let (left_name, left_binding) = atomize_local(left, fresh, source_ref);
            let or_expr = AnfExpr::ShortCircuitOr {
                left: left_name,
                right: Box::new(lower_core_expr_to_anf_local(right, fresh, source_ref)),
            };
            if let Some(binding) = left_binding {
                wrap_local_bindings(vec![binding], or_expr)
            } else {
                or_expr
            }
        }
        _ => lower_core_expr_to_anf(expr, fresh, source_ref, &mut Vec::new()),
    }
}

// ── lower_core_expr_to_anf ────────────────────────────────────────────────

/// Recursively lower a `CoreExpr` to an `AnfExpr`.
///
/// Non-atomic sub-expressions (nested calls, non-trivial conditions, etc.)
/// are atomized: a synthetic `AnfBinding` is pushed to `out` and the
/// sub-expression is replaced by a `Var` reference to that binding.
///
/// All synthetic bindings carry `source_ref` for end-to-end provenance.
pub fn lower_core_expr_to_anf(
    expr: &CoreExpr,
    fresh: &mut u32,
    source_ref: ail_core::semantic_graph::NodeRef,
    out: &mut Vec<AnfBinding>,
) -> AnfExpr {
    match expr {
        // Atomic values — no sub-expressions to flatten.
        CoreExpr::Literal(v) => AnfExpr::Literal(v.clone()),
        CoreExpr::Var(n) => AnfExpr::Var(n.clone()),

        // Let: lower value and body recursively; no atomization needed.
        CoreExpr::Let { name, value, body } => {
            let anf_value = lower_core_expr_to_anf(value, fresh, source_ref, out);
            let anf_body = lower_core_expr_to_anf(body, fresh, source_ref, out);
            AnfExpr::Let {
                name: name.clone(),
                value: Box::new(anf_value),
                body: Box::new(anf_body),
            }
        }

        // If: condition must be atomic (atomize if needed).
        CoreExpr::If { cond, then_, else_ } => {
            let cond_name = atomize(cond, fresh, source_ref, out);
            let anf_then = lower_core_expr_to_anf(then_, fresh, source_ref, out);
            let anf_else = lower_core_expr_to_anf(else_, fresh, source_ref, out);
            AnfExpr::If {
                cond: cond_name,
                then_branch: Box::new(anf_then),
                else_branch: Box::new(anf_else),
            }
        }

        // Call: all args must be atomic (atomize each non-Var arg).
        CoreExpr::Call { func, args } => {
            let atomic_args: Vec<String> = args
                .iter()
                .map(|a| atomize(a, fresh, source_ref, out))
                .collect();
            AnfExpr::Call {
                func: func.clone(),
                args: atomic_args,
            }
        }
        CoreExpr::Add(left, right) => {
            lower_core_binary_to_anf("add", left, right, fresh, source_ref)
        }
        CoreExpr::Sub(left, right) => {
            lower_core_binary_to_anf("sub", left, right, fresh, source_ref)
        }
        CoreExpr::Mul(left, right) => {
            lower_core_binary_to_anf("mul", left, right, fresh, source_ref)
        }
        CoreExpr::Div(left, right) => {
            lower_core_binary_to_anf("div", left, right, fresh, source_ref)
        }
        CoreExpr::Mod(left, right) => {
            lower_core_binary_to_anf("mod", left, right, fresh, source_ref)
        }
        CoreExpr::Eq(left, right) => lower_core_binary_to_anf("eq", left, right, fresh, source_ref),
        CoreExpr::Lt(left, right) => lower_core_binary_to_anf("lt", left, right, fresh, source_ref),
        CoreExpr::Gt(left, right) => lower_core_binary_to_anf("gt", left, right, fresh, source_ref),

        // FieldGet: record expression must be atomic.
        CoreExpr::FieldGet { record, field } => {
            let record_name = atomize(record, fresh, source_ref, out);
            AnfExpr::FieldGet {
                record: record_name,
                field: field.clone(),
            }
        }

        // ── G20: Expression body lowering ────────────────────────────────

        // Match: scrutinee must be atomic (atomize if non-Var).
        // Each arm body is lowered recursively.
        CoreExpr::Match { scrutinee, arms } => {
            let scrutinee_name = atomize(scrutinee, fresh, source_ref, out);
            let anf_arms = arms
                .iter()
                .map(|arm| crate::anf::AnfMatchArm {
                    pattern: arm.pattern.clone(),
                    body: lower_core_expr_to_anf(&arm.body, fresh, source_ref, out),
                })
                .collect();
            AnfExpr::Match {
                scrutinee: scrutinee_name,
                arms: anf_arms,
            }
        }

        // Lambda: params are already names; lower body recursively.
        CoreExpr::Lambda { params, body } => {
            let anf_body = lower_core_expr_to_anf(body, fresh, source_ref, out);
            AnfExpr::Lambda {
                params: params.clone(),
                body: Box::new(anf_body),
            }
        }

        // RecordNew: full ANF normalization — each field value is let-bound
        // so field construction arguments are always atomic.
        CoreExpr::RecordNew { fields } => {
            let anf_fields: Vec<(String, AnfExpr)> = fields
                .iter()
                .map(|(name, val)| {
                    let atom = atomize(val, fresh, source_ref, out);
                    (name.clone(), AnfExpr::Var(atom))
                })
                .collect();
            AnfExpr::RecordNew { fields: anf_fields }
        }

        // FieldUpdate: record expression must be atomic; value is also atomized
        // for full ANF normalization.
        CoreExpr::FieldUpdate {
            record,
            field,
            value,
        } => {
            let record_name = atomize(record, fresh, source_ref, out);
            let value_name = atomize(value, fresh, source_ref, out);
            AnfExpr::FieldUpdate {
                record: record_name,
                field: field.clone(),
                value: Box::new(AnfExpr::Var(value_name)),
            }
        }

        // TupleNew: full ANF normalization — each element is let-bound.
        CoreExpr::TupleNew(elems) => {
            let anf_elems: Vec<AnfExpr> = elems
                .iter()
                .map(|e| {
                    let name = atomize(e, fresh, source_ref, out);
                    AnfExpr::Var(name)
                })
                .collect();
            AnfExpr::TupleNew(anf_elems)
        }

        // VariantNew: payload is atomized for full ANF normalization.
        CoreExpr::VariantNew { tag, payload } => {
            let anf_payload = payload.as_ref().map(|p| {
                let name = atomize(p, fresh, source_ref, out);
                Box::new(AnfExpr::Var(name))
            });
            AnfExpr::VariantNew {
                tag: tag.clone(),
                payload: anf_payload,
            }
        }

        // ListNew: lower each element recursively; let-bind non-atomic elements
        // to enforce full ANF normalization.
        CoreExpr::ListNew(elems) => {
            let anf_elems: Vec<AnfExpr> = elems
                .iter()
                .map(|e| {
                    let name = atomize(e, fresh, source_ref, out);
                    AnfExpr::Var(name)
                })
                .collect();
            AnfExpr::ListNew(anf_elems)
        }

        // Loop: body is lowered recursively; exits through Break.
        // The termination field is not used during ANF lowering.
        CoreExpr::Loop { body, .. } => {
            let anf_body = lower_core_expr_to_anf(body, fresh, source_ref, out);
            AnfExpr::Loop {
                body: Box::new(anf_body),
            }
        }

        // Break: value is lowered recursively so it can be emitted before br.
        CoreExpr::Break { value } => {
            let anf_value = lower_core_expr_to_anf(value, fresh, source_ref, out);
            AnfExpr::Break {
                value: Box::new(anf_value),
            }
        }

        CoreExpr::Continue => AnfExpr::Continue,

        // WhileLoop: condition must be atomic; body is lowered recursively.
        // The termination field is not used during ANF lowering.
        CoreExpr::WhileLoop { cond, body, .. } => {
            let cond_name = atomize(cond, fresh, source_ref, out);
            let anf_body = lower_core_expr_to_anf(body, fresh, source_ref, out);
            AnfExpr::WhileLoop {
                cond: cond_name,
                body: Box::new(anf_body),
            }
        }

        // ── G20 R2: new semantic variants ────────────────────────────────

        // And: short-circuit — lower left; result is Var(left_name); right
        // is wrapped in the ANF ShortCircuitAnd so it is only evaluated when
        // the condition demands it.
        CoreExpr::And { left, right } => {
            let left_name = atomize(left, fresh, source_ref, out);
            let anf_right = lower_core_expr_to_anf(right, fresh, source_ref, out);
            AnfExpr::ShortCircuitAnd {
                left: left_name,
                right: Box::new(anf_right),
            }
        }

        // Or: symmetric short-circuit — left is atomized; right is wrapped.
        CoreExpr::Or { left, right } => {
            let left_name = atomize(left, fresh, source_ref, out);
            let anf_right = lower_core_expr_to_anf(right, fresh, source_ref, out);
            AnfExpr::ShortCircuitOr {
                left: left_name,
                right: Box::new(anf_right),
            }
        }

        // EffectCall: atomize all args; effect ordering is structural.
        CoreExpr::EffectCall {
            capability,
            func,
            args,
        } => {
            let atomic_args: Vec<String> = args
                .iter()
                .map(|a| atomize(a, fresh, source_ref, out))
                .collect();
            AnfExpr::EffectCall {
                capability: capability.clone(),
                func: func.clone(),
                args: atomic_args,
            }
        }

        // Dispatch: dynamic handler dispatch — atomize all args.
        CoreExpr::Dispatch {
            handler,
            method,
            args,
        } => {
            let atomic_args: Vec<String> = args
                .iter()
                .map(|a| atomize(a, fresh, source_ref, out))
                .collect();
            AnfExpr::Dispatch {
                handler: handler.clone(),
                method: method.clone(),
                args: atomic_args,
            }
        }

        // TaskSpawn: atomize all args; explicit ordering via ANF let-chain.
        CoreExpr::TaskSpawn { func, args } => {
            let atomic_args: Vec<String> = args
                .iter()
                .map(|a| atomize(a, fresh, source_ref, out))
                .collect();
            AnfExpr::TaskSpawn {
                func: func.clone(),
                args: atomic_args,
            }
        }

        // ChannelSend: both channel and value must be atomic.
        CoreExpr::ChannelSend { channel, value } => {
            let channel_name = atomize(channel, fresh, source_ref, out);
            let value_name = atomize(value, fresh, source_ref, out);
            AnfExpr::ChannelSend {
                channel: channel_name,
                value: value_name,
            }
        }

        // ChannelReceive: channel must be atomic.
        CoreExpr::ChannelReceive { channel } => {
            let channel_name = atomize(channel, fresh, source_ref, out);
            AnfExpr::ChannelReceive {
                channel: channel_name,
            }
        }

        // RuntimeCheck: condition must be atomic; check_ref and msg are preserved.
        CoreExpr::RuntimeCheck {
            check_ref,
            cond,
            msg,
        } => {
            let cond_name = atomize(cond, fresh, source_ref, out);
            AnfExpr::RuntimeCheck {
                check_ref: check_ref.clone(),
                cond: cond_name,
                msg: msg.clone(),
            }
        }

        // ResourceAcquire: atomize all args; acquisition ordering is structural.
        CoreExpr::ResourceAcquire { resource, args } => {
            let atomic_args: Vec<String> = args
                .iter()
                .map(|a| atomize(a, fresh, source_ref, out))
                .collect();
            AnfExpr::ResourceAcquire {
                resource: resource.clone(),
                args: atomic_args,
            }
        }

        // ResourceRelease: handle must be atomic.
        CoreExpr::ResourceRelease { handle } => {
            let handle_name = atomize(handle, fresh, source_ref, out);
            AnfExpr::ResourceRelease {
                handle: handle_name,
            }
        }

        // ── G23: new concurrency and cell primitives ─────────────────────

        // TaskAwait: task handle must be atomic.
        CoreExpr::TaskAwait { task } => {
            let task_name = atomize(task, fresh, source_ref, out);
            AnfExpr::TaskAwait { task: task_name }
        }

        // TaskCancel: task handle must be atomic.
        CoreExpr::TaskCancel { task } => {
            let task_name = atomize(task, fresh, source_ref, out);
            AnfExpr::TaskCancel { task: task_name }
        }

        // TaskGroup: body is lowered recursively (may contain TaskSpawn calls).
        CoreExpr::TaskGroup { body } => {
            let anf_body = lower_core_expr_to_anf(body, fresh, source_ref, out);
            AnfExpr::TaskGroup {
                body: Box::new(anf_body),
            }
        }

        // ChannelNew: capacity is a primitive scalar — no sub-expression to lower.
        CoreExpr::ChannelNew { capacity } => AnfExpr::ChannelNew {
            capacity: *capacity,
        },

        // Select: each branch channel must be atomic; body is lowered recursively.
        CoreExpr::Select { branches } => {
            let anf_branches = branches
                .iter()
                .map(|clause| {
                    let channel_name = atomize(&clause.channel, fresh, source_ref, out);
                    let anf_body = lower_core_expr_to_anf(&clause.body, fresh, source_ref, out);
                    crate::anf::AnfSelectClause {
                        channel: channel_name,
                        binding: clause.binding.clone(),
                        body: anf_body,
                    }
                })
                .collect();
            AnfExpr::Select {
                branches: anf_branches,
            }
        }

        // Timeout: duration must be atomic; body is lowered recursively.
        CoreExpr::Timeout { duration, body } => {
            let duration_name = atomize(duration, fresh, source_ref, out);
            let anf_body = lower_core_expr_to_anf(body, fresh, source_ref, out);
            AnfExpr::Timeout {
                duration: duration_name,
                body: Box::new(anf_body),
            }
        }

        // CellNew: init is atomized (must be atomic in ANF).
        CoreExpr::CellNew { init } => {
            let init_name = atomize(init, fresh, source_ref, out);
            AnfExpr::CellNew { init: init_name }
        }

        // CellGet: cell must be atomic.
        CoreExpr::CellGet { cell } => {
            let cell_name = atomize(cell, fresh, source_ref, out);
            AnfExpr::CellGet { cell: cell_name }
        }

        // CellSet: both cell and value must be atomic.
        CoreExpr::CellSet { cell, value } => {
            let cell_name = atomize(cell, fresh, source_ref, out);
            let value_name = atomize(value, fresh, source_ref, out);
            AnfExpr::CellSet {
                cell: cell_name,
                value: value_name,
            }
        }

        // CoreExpr::Placeholder → AnfExpr::Placeholder (no expression body).
        CoreExpr::Placeholder => AnfExpr::Placeholder,

        // ── ola5-compiler-core: Gap 2 — real ANF lowering ────────────────

        // Return: atomize the value, wrap in AnfExpr::Return.
        CoreExpr::Return { value } => {
            let val_name = atomize(value, fresh, source_ref, out);
            AnfExpr::Return(Box::new(AnfExpr::Var(val_name)))
        }

        // Assume: no runtime effect — produces unit.  Predicate and reason
        // are preserved for static analysis / documentation purposes.
        CoreExpr::Assume { predicate, reason } => AnfExpr::Assume {
            predicate: predicate.clone(),
            reason: reason.clone(),
        },

        // Abort: always traps — diagnostic message preserved.
        CoreExpr::Abort { message } => AnfExpr::Abort {
            message: message.clone(),
        },

        // BoundaryCall: atomize all args; encode boundary + func as a
        // namespaced call so backends can route to the trust boundary.
        CoreExpr::BoundaryCall { boundary, func, args } => {
            let atomic_args: Vec<String> = args
                .iter()
                .map(|a| atomize(a, fresh, source_ref, out))
                .collect();
            AnfExpr::Call {
                func: format!("{boundary}::{func}"),
                args: atomic_args,
            }
        }

        // DynCall: atomize all args; encode interface + method as a
        // namespaced call for dynamic dispatch through Dyn<Interface>.
        CoreExpr::DynCall { interface, method, args } => {
            let atomic_args: Vec<String> = args
                .iter()
                .map(|a| atomize(a, fresh, source_ref, out))
                .collect();
            AnfExpr::Call {
                func: format!("dyn::{interface}::{method}"),
                args: atomic_args,
            }
        }

        // ── doc-alignment: new CoreExpr variant lowering ─────────────────────

        // CapabilityUse: lower as an EffectCall-like call.
        CoreExpr::CapabilityUse { capability, args } => {
            let atomic_args: Vec<String> = args
                .iter()
                .map(|a| atomize(a, fresh, source_ref, out))
                .collect();
            AnfExpr::EffectCall {
                capability: capability.clone(),
                func: capability.clone(),
                args: atomic_args,
            }
        }

        // ResourceUse: atomize handle, lower body.
        CoreExpr::ResourceUse { handle, body } => {
            let _h = atomize(handle, fresh, source_ref, out);
            lower_core_expr_to_anf(body, fresh, source_ref, out)
        }

        // ResourceUsing: acquire, bind, use, release — lowered as let + body.
        CoreExpr::ResourceUsing { resource, binding, body } => {
            let res_name = atomize(resource, fresh, source_ref, out);
            // Emit acquire binding.
            out.push(AnfBinding {
                source_ref,
                name: binding.clone(),
                expr: AnfExpr::ResourceAcquire {
                    resource: res_name,
                    args: vec![],
                },
            });
            let result = lower_core_expr_to_anf(body, fresh, source_ref, out);
            // Emit implicit release after body.
            let release_tmp = format!("anf_{}", *fresh);
            *fresh += 1;
            out.push(AnfBinding {
                source_ref,
                name: release_tmp,
                expr: AnfExpr::ResourceRelease {
                    handle: binding.clone(),
                },
            });
            result
        }

        // ResourceTransfer: atomize both operands.
        CoreExpr::ResourceTransfer { handle, target } => {
            let h = atomize(handle, fresh, source_ref, out);
            let t = atomize(target, fresh, source_ref, out);
            AnfExpr::Call {
                func: "__resource_transfer".to_string(),
                args: vec![h, t],
            }
        }

        // ForeignFunctionCall: lower as a boundary-style call.
        CoreExpr::ForeignFunctionCall { func, args } => {
            let atomic_args: Vec<String> = args
                .iter()
                .map(|a| atomize(a, fresh, source_ref, out))
                .collect();
            AnfExpr::Call {
                func: format!("__foreign::{func}"),
                args: atomic_args,
            }
        }

        // PatchFieldConstruct: lower as a variant construction.
        CoreExpr::PatchFieldConstruct { state, value } => {
            let payload = value
                .as_ref()
                .map(|v| {
                    let name = atomize(v, fresh, source_ref, out);
                    Box::new(AnfExpr::Var(name))
                });
            AnfExpr::VariantNew {
                tag: state.clone(),
                payload,
            }
        }

        // PatchFieldMatch: lower as a standard match.
        CoreExpr::PatchFieldMatch { scrutinee, arms } => {
            let scrutinee_name = atomize(scrutinee, fresh, source_ref, out);
            let anf_arms: Vec<crate::anf::AnfMatchArm> = arms
                .iter()
                .map(|arm| crate::anf::AnfMatchArm {
                    pattern: arm.pattern.clone(),
                    body: lower_core_expr_to_anf(&arm.body, fresh, source_ref, out),
                })
                .collect();
            AnfExpr::Match {
                scrutinee: scrutinee_name,
                arms: anf_arms,
            }
        }

        // IndexGet: atomize both collection and index.
        CoreExpr::IndexGet { collection, index } => {
            let col_name = atomize(collection, fresh, source_ref, out);
            let idx_name = atomize(index, fresh, source_ref, out);
            AnfExpr::IndexGet {
                collection: col_name,
                index: idx_name,
            }
        }

        // MapNew: atomize all keys and values; preserve declaration order.
        CoreExpr::MapNew { entries } => {
            let atomic_entries: Vec<(String, String)> = entries
                .iter()
                .map(|(k, v)| {
                    let k_name = atomize(k, fresh, source_ref, out);
                    let v_name = atomize(v, fresh, source_ref, out);
                    (k_name, v_name)
                })
                .collect();
            AnfExpr::MapNew { entries: atomic_entries }
        }

        // SetNew: atomize all elements; preserve declaration order.
        CoreExpr::SetNew { elements } => {
            let atomic_elems: Vec<String> = elements
                .iter()
                .map(|e| atomize(e, fresh, source_ref, out))
                .collect();
            AnfExpr::SetNew { elements: atomic_elems }
        }

        // ForEach: atomize the collection; body is lowered recursively.
        CoreExpr::ForEach { binding, collection, body } => {
            let col_name = atomize(collection, fresh, source_ref, out);
            let anf_body = lower_core_expr_to_anf(body, fresh, source_ref, out);
            AnfExpr::ForEach {
                binding: binding.clone(),
                collection: col_name,
                body: Box::new(anf_body),
            }
        }

        // Fold: atomize init, list, and func; all three become atomic names.
        CoreExpr::Fold { init, list, func } => {
            let init_name = atomize(init, fresh, source_ref, out);
            let list_name = atomize(list, fresh, source_ref, out);
            let func_name = atomize(func, fresh, source_ref, out);
            AnfExpr::Fold {
                init: init_name,
                list: list_name,
                func: func_name,
            }
        }

        // CoreExpr::Placeholder → AnfExpr::Placeholder (no expression body).
        // (kept last for clarity — already handled above)
    }
}

// ── map_core_node_to_anf ──────────────────────────────────────────────────

/// Lower one `CoreNode` into one or more `AnfBinding`s.
///
/// If the node has a `CoreExpr` body, inline temporaries are kept inside the
/// node binding as local `let` expressions so executable function parameters do
/// not accidentally become global synthetic bindings.
///
/// Nodes without `expr` (modules, types, capabilities, etc.) get a default
/// `AnfExpr::Literal(LiteralValue::Unit)`.
///
/// Provenance (`source_ref`) is preserved verbatim on every emitted binding.
fn map_core_node_to_anf(node: &CoreNode, fresh: &mut u32, out: &mut Vec<AnfBinding>) {
    let anf_expr = match &node.expr {
        Some(core_expr) => lower_core_expr_to_anf_local(core_expr, fresh, node.source_ref),
        None => AnfExpr::Literal(LiteralValue::Unit),
    };
    out.push(AnfBinding {
        source_ref: node.source_ref,
        name: node.name.clone(),
        expr: anf_expr,
    });
}

// ── nominal_to_core_type ─────────────────────────────────────────────────

/// Map a `TypeFacts.nominal` string to a `CoreType` variant.
///
/// Recognised nominals correspond to the 20 type primitives listed in
/// `docs/core-ir.md §3`.  Any unrecognised nominal falls back to
/// `CoreType::Generic(None)`.
pub fn nominal_to_core_type(nominal: &str) -> CoreType {
    match nominal {
        "Unit" => CoreType::Unit,
        "Never" => CoreType::Never,
        "Bool" => CoreType::Bool,
        "Int" => CoreType::Int,
        "UInt" => CoreType::UInt,
        "Float" => CoreType::Float,
        "Text" => CoreType::Text,
        "Bytes" => CoreType::Bytes,
        "Record" => CoreType::Record,
        "Variant" => CoreType::Variant,
        "Tuple" => CoreType::Tuple,
        // Parameterized variants: inner type defaults to Generic when only the
        // nominal name is available (full resolution requires type-param phase).
        "List" => CoreType::List(Box::new(CoreType::Generic(None))),
        "Map" => CoreType::Map(Box::new(CoreType::Generic(None)), Box::new(CoreType::Generic(None))),
        "Set" => CoreType::Set(Box::new(CoreType::Generic(None))),
        "Option" => CoreType::Option(Box::new(CoreType::Generic(None))),
        "Result" => CoreType::Result(Box::new(CoreType::Generic(None)), Box::new(CoreType::Generic(None))),
        "Function" => CoreType::Function {
            params: vec![],
            ret: Box::new(CoreType::Generic(None)),
            effects: vec![],
        },
        "Handle" => CoreType::Handle {
            resource: Box::new(CoreType::Generic(None)),
            mode: ResourceMode::Copy,
        },
        "Refinement" => CoreType::Refinement {
            base: Box::new(CoreType::Generic(None)),
            predicate: String::new(),
        },
        "Generic" => CoreType::Generic(None),
        _ => CoreType::Generic(None),
    }
}

fn literal_expr_from_runtime_checks(
    checks: Option<&Vec<ail_core::semantic_graph::RuntimeCheckMeta>>,
) -> Option<CoreExpr> {
    let predicate = checks?.first()?.predicate.strip_prefix("literal:i64=")?;
    let value = predicate.parse::<i64>().ok()?;
    Some(CoreExpr::Literal(LiteralValue::Int(value)))
}

fn expr_from_graph_node(
    node: &ail_core::semantic_graph::GraphNode,
) -> Result<Option<CoreExpr>, CompileError> {
    if let Some(body) = &node.body_expr {
        if body.trim_start().starts_with('@') {
            return Ok(None);
        }
        return parse_expr(body)
            .map(Some)
            .map_err(|err| CompileError::InvalidGraph(format!("{} body: {err}", node.name)));
    }
    Ok(literal_expr_from_runtime_checks(
        node.runtime_checks.as_ref(),
    ))
}

// ── lower_to_core_ir ──────────────────────────────────────────────────────

/// Lower a verified `SemanticGraph` into a `CoreIr`.
///
/// # Pre-conditions
///
/// - `report.summary()` must be `Proven` or `RuntimeChecked`; any other
///   summary immediately returns `Err(CompileError::RejectedReport)`.
///
/// # Hash chain
///
/// Seals `core_ir_hash = blake3(graph_snapshot_hash || core_ir_bytes)`.
/// `graph_snapshot_hash` and `verification_report_hash` are recorded in
/// `StageHashes` for downstream stages.
///
/// # Errors
///
/// - `CompileError::RejectedReport` — report summary is not accepted.
/// - `CompileError::EncodingError` — CBOR serialization failed.
pub fn lower_to_core_ir(
    graph: &SemanticGraph,
    report: &VerificationReport,
) -> Result<CoreIr, CompileError> {
    // Gate: reject unacceptable reports.
    if !is_report_accepted(report) {
        return Err(CompileError::RejectedReport);
    }

    // Gate: validate graph structural invariants (unique refs, no dangling edges).
    graph.validate().map_err(|e| match e {
        GraphValidationError::DuplicateRef(r) => {
            CompileError::InvalidGraph(format!("duplicate NodeRef({})", r.0))
        }
        GraphValidationError::DanglingEdge { r#ref, .. } => CompileError::MissingNode(r#ref),
        GraphValidationError::EffectRowNoEmitsEdge(r) => {
            CompileError::InvalidGraph(format!("effect_row declared but no Emits edge on NodeRef({})", r.0))
        }
        GraphValidationError::CapabilityReqsMissingNode { owner_ref, cap_name } => {
            CompileError::InvalidGraph(format!(
                "capability '{}' required by NodeRef({}) has no matching Capability node",
                cap_name, owner_ref.0
            ))
        }
    })?;

    // Hash the pipeline inputs (empty parent → blake3(content)).
    let graph_cbor = stable_cbor_bytes(graph)?;
    let graph_snapshot_hash = hash_with_parent(&[], &graph_cbor);

    let report_cbor = stable_cbor_bytes(report)?;
    let verification_report_hash = hash_with_parent(&[], &report_cbor);

    // Lower each source GraphNode to a CoreNode (1-to-1, in traversal order).
    // G2: populate `ty` from `GraphNode.type_facts.nominal` when present.
    // Function `body_expr` strings are parsed into executable CoreExpr bodies;
    // legacy literal runtime-check bodies are preserved as a fallback.
    let nodes: Vec<CoreNode> = graph
        .nodes
        .iter()
        .map(|gn| {
            Ok(CoreNode {
                source_ref: gn.id,
                kind: map_node_kind(gn.kind),
                name: gn.name.clone(),
                ty: gn
                    .type_facts
                    .as_ref()
                    .map(|tf| nominal_to_core_type(&tf.nominal)),
                expr: if gn.kind == NodeKind::Function {
                    expr_from_graph_node(gn)?
                } else {
                    None
                },
            })
        })
        .collect::<Result<Vec<_>, CompileError>>()?;

    // Seal: core_ir_hash = blake3(graph_snapshot_hash || core_ir_bytes).
    let core_ir_bytes = stable_cbor_bytes(&nodes)?;
    let core_ir_hash = hash_with_parent(&graph_snapshot_hash, &core_ir_bytes);

    Ok(CoreIr {
        nodes,
        stage_hashes: StageHashes {
            graph_snapshot_hash,
            verification_report_hash,
            core_ir_hash,
            anf_ir_hash: None,
            wasm_hash: None,
            native_hash: None,
            source_map_hash: None,
            artifact_manifest_hash: None,
        },
    })
}

// ── Semantic provenance extraction ───────────────────────────────────────

/// Node-level provenance data extracted from a `GraphNode` for source-map
/// enrichment.  All fields are `Option` because graph nodes are not required
/// to carry this metadata.
struct NodeProvenance {
    /// From `GraphNode.provenance.change_id` — the `ChangeSet` that last
    /// created or modified this node.
    change_set: Option<String>,
    /// Derived block identity: `Some(format!("block.{name}"))` for
    /// `Module` / `Boundary` nodes; `None` for all other kinds.
    block_ref: Option<String>,
    /// Derived contract ref: `Some(format!("contract.{name}"))` when the
    /// node has `contract_clauses`; `None` otherwise.
    contract_ref: Option<String>,
    /// First declared effect from `GraphNode.effect_row.effects`, if any.
    effect_ref: Option<String>,
    /// Content hash of the first `RuntimeCheckMeta` in
    /// `GraphNode.runtime_checks`, if any.
    runtime_check_ref: Option<String>,
}

/// Build a `NodeRef → NodeProvenance` lookup from a `SemanticGraph`.
///
/// Used by `lower_to_anf_with_graph` to enrich `SourceMapEntry` fields
/// without changing the `lower_to_anf` public API.
fn extract_provenance_lookup(graph: &SemanticGraph) -> BTreeMap<NodeRef, NodeProvenance> {
    graph
        .nodes
        .iter()
        .map(|gn| {
            let prov = NodeProvenance {
                change_set: gn.provenance.as_ref().map(|p| p.change_id.clone()),
                block_ref: match gn.kind {
                    NodeKind::Module | NodeKind::Boundary => Some(format!("block.{}", gn.name)),
                    _ => None,
                },
                contract_ref: gn
                    .contract_clauses
                    .as_ref()
                    .map(|_| format!("contract.{}", gn.name)),
                effect_ref: gn
                    .effect_row
                    .as_ref()
                    .and_then(|er| er.effects.first().cloned()),
                runtime_check_ref: gn
                    .runtime_checks
                    .as_ref()
                    .and_then(|rcs| rcs.first())
                    .map(|rc| rc.hash.clone()),
            };
            (gn.id, prov)
        })
        .collect()
}

/// Build an enriched `SourceMap` from ANF bindings and a provenance lookup.
///
/// Each `SourceMapEntry` is populated with the provenance fields available
/// in the lookup for the binding's `source_ref`.  Fields for which no
/// upstream data exists remain `None`.
///
/// `proof_obligation_ref` is always `None` — the upstream pipeline does not
/// yet produce proof obligation metadata.  The field is plumbed correctly so
/// that when upstream starts producing it, it flows through automatically.
fn build_enriched_source_map(
    bindings: &[AnfBinding],
    lookup: &BTreeMap<NodeRef, NodeProvenance>,
) -> SourceMap {
    let entries = bindings
        .iter()
        .map(|b| {
            let prov = lookup.get(&b.source_ref);
            SourceMapEntry {
                binding_name: b.name.clone(),
                node_id: b.source_ref,
                block_ref: prov
                    .and_then(|p| p.block_ref.as_ref())
                    .map(|s| BlockRef(s.clone())),
                change_set: prov.and_then(|p| p.change_set.clone()),
                contract_ref: prov
                    .and_then(|p| p.contract_ref.as_ref())
                    .map(|s| ContractRef(s.clone())),
                effect_ref: prov
                    .and_then(|p| p.effect_ref.as_ref())
                    .map(|s| EffectRef(s.clone())),
                // No upstream proof-obligation data yet — field plumbed for
                // future use.
                proof_obligation_ref: None::<ProofObligationRef>,
                runtime_check_ref: prov
                    .and_then(|p| p.runtime_check_ref.as_ref())
                    .map(|s| RuntimeCheckRef(s.clone())),
                wasm_offset: None,
                native_offset: None,
            }
        })
        .collect();
    SourceMap { entries }
}

// ── lower_to_anf_impl ────────────────────────────────────────────────────

/// Shared ANF lowering implementation.
///
/// When `provenance_lookup` is `Some`, each `SourceMapEntry` is enriched with
/// semantic provenance extracted from the original `SemanticGraph`.  When it
/// is `None`, all optional provenance fields remain `None` (backward-compat
/// path used by `lower_to_anf`).
fn lower_to_anf_impl(
    core: &CoreIr,
    provenance_lookup: Option<&BTreeMap<NodeRef, NodeProvenance>>,
) -> Result<AnfIr, CompileError> {
    // Lower each CoreNode — collecting synthetic temporaries and node bindings.
    let mut bindings: Vec<AnfBinding> = Vec::with_capacity(core.nodes.len());
    let mut fresh: u32 = 0;
    for node in &core.nodes {
        map_core_node_to_anf(node, &mut fresh, &mut bindings);
    }
    let bindings = optimize_bindings(bindings);

    // Build semantic source map, optionally enriched with provenance.
    let source_map = match provenance_lookup {
        Some(lookup) => build_enriched_source_map(&bindings, lookup),
        None => SourceMap::from_bindings(&bindings),
    };

    // Seal: anf_ir_hash = blake3(core_ir_hash || anf_ir_bytes).
    // Note: anf_ir_hash covers *bindings* only, not the source map, so it is
    // identical whether or not provenance enrichment is applied.
    let anf_ir_bytes = stable_cbor_bytes(&bindings)?;
    let anf_ir_hash = hash_with_parent(&core.stage_hashes.core_ir_hash, &anf_ir_bytes);

    // Extend the stage hashes from Core IR.
    let mut stage_hashes = core.stage_hashes.clone();
    stage_hashes.anf_ir_hash = Some(anf_ir_hash);

    Ok(AnfIr {
        schema_version: ANF_SCHEMA_VERSION,
        bindings,
        source_map,
        stage_hashes,
    })
}

// ── lower_to_anf ─────────────────────────────────────────────────────────

/// Normalize a `CoreIr` into Administrative Normal Form (`AnfIr`).
///
/// # Pure transformation
///
/// No side effects, no I/O.  The same `CoreIr` always produces an identical
/// `AnfIr` (same bindings, same `anf_ir_hash`).
///
/// # Provenance
///
/// Every `AnfBinding.source_ref` is copied verbatim from its `CoreNode`
/// counterpart.  The mapping is 1-to-1 in Phase 7; structural normalisation
/// (e.g. let-bindings for sub-expressions) is deferred to Phase 8.
///
/// All `SourceMapEntry` optional provenance fields (`block_ref`, `change_set`,
/// etc.) remain `None` in this path.  Use `lower_to_anf_with_graph` when the
/// original `SemanticGraph` is available to get enriched provenance.
///
/// # Hash chain
///
/// Extends the chain: `anf_ir_hash = blake3(core_ir_hash || anf_ir_bytes)`.
///
/// # Errors
///
/// - `CompileError::EncodingError` — CBOR serialization failed.
pub fn lower_to_anf(core: &CoreIr) -> Result<AnfIr, CompileError> {
    lower_to_anf_impl(core, None)
}

// ── lower_to_anf_with_graph ───────────────────────────────────────────────

/// Normalize a `CoreIr` into ANF with full semantic provenance enrichment.
///
/// Identical to `lower_to_anf` in every respect except that each
/// `SourceMapEntry` is enriched with provenance data extracted from the
/// original `SemanticGraph`:
///
/// | `SourceMapEntry` field  | Source in `GraphNode`                        |
/// |-------------------------|----------------------------------------------|
/// | `change_set`            | `provenance.change_id`                       |
/// | `block_ref`             | Derived: `"block.<name>"` for Module/Boundary|
/// | `contract_ref`          | Derived: `"contract.<name>"` when clauses set|
/// | `effect_ref`            | First effect in `effect_row.effects`         |
/// | `runtime_check_ref`     | First `RuntimeCheckMeta.hash`                |
/// | `proof_obligation_ref`  | Always `None` — upstream not producing yet   |
///
/// # Hash chain
///
/// `anf_ir_hash` is identical to the one produced by `lower_to_anf` because
/// it covers only the bindings, not the source-map provenance fields.
///
/// # Errors
///
/// - `CompileError::EncodingError` — CBOR serialization failed.
pub fn lower_to_anf_with_graph(
    core: &CoreIr,
    graph: &SemanticGraph,
) -> Result<AnfIr, CompileError> {
    let lookup = extract_provenance_lookup(graph);
    lower_to_anf_impl(core, Some(&lookup))
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
    use ail_verify::report::{VerificationEntry, VerificationReport, VerificationState};

    use super::*;

    // ── Helpers ───────────────────────────────────────────────────────────

    fn report_with_state(state: VerificationState) -> VerificationReport {
        VerificationReport {
            entries: vec![VerificationEntry {
                claim: "claim".to_string(),
                state,
                scope: "s".to_string(),
                evidence: None,
                blocking: matches!(state, VerificationState::Failed | VerificationState::Unsafe),
                repair_options: vec![],
            }],
            ..Default::default()
        }
    }

    fn proven_report() -> VerificationReport {
        VerificationReport {
            entries: vec![],
            ..Default::default()
        }
    }

    fn one_node_graph() -> SemanticGraph {
        SemanticGraph {
            nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Module, "m")],
            edges: vec![],
        }
    }

    // ── is_report_accepted ────────────────────────────────────────────────

    // Proven and RuntimeChecked are accepted; all others are rejected.
    #[test]
    fn proven_is_accepted() {
        assert!(is_report_accepted(&proven_report()));
    }

    #[test]
    fn runtime_checked_is_accepted() {
        let report = report_with_state(VerificationState::RuntimeChecked);
        assert!(is_report_accepted(&report));
    }

    #[test]
    fn failed_is_rejected() {
        let report = report_with_state(VerificationState::Failed);
        assert!(!is_report_accepted(&report));
    }

    #[test]
    fn assumed_is_rejected() {
        let report = report_with_state(VerificationState::Assumed);
        assert!(!is_report_accepted(&report));
    }

    #[test]
    fn unverified_is_rejected() {
        let report = report_with_state(VerificationState::Unverified);
        assert!(!is_report_accepted(&report));
    }

    #[test]
    fn unsafe_is_rejected() {
        let report = report_with_state(VerificationState::Unsafe);
        assert!(!is_report_accepted(&report));
    }

    // ── map_node_kind ─────────────────────────────────────────────────────

    // All 10 source kinds map to their CoreNodeKind counterpart.
    #[test]
    fn all_node_kinds_map_correctly() {
        let cases = [
            (NodeKind::Module, CoreNodeKind::Module),
            (NodeKind::Function, CoreNodeKind::Function),
            (NodeKind::Type, CoreNodeKind::Type),
            (NodeKind::Effect, CoreNodeKind::Effect),
            (NodeKind::Capability, CoreNodeKind::Capability),
            (NodeKind::Contract, CoreNodeKind::Contract),
            (NodeKind::Invariant, CoreNodeKind::Invariant),
            (NodeKind::Test, CoreNodeKind::Test),
            (NodeKind::Boundary, CoreNodeKind::Boundary),
            (NodeKind::Package, CoreNodeKind::Package),
        ];
        for (src, expected) in cases {
            assert_eq!(
                map_node_kind(src),
                expected,
                "NodeKind::{src:?} must map to CoreNodeKind::{expected:?}"
            );
        }
    }

    // ── map_core_node_to_anf ──────────────────────────────────────────────

    // Provenance and name are preserved verbatim.
    #[test]
    fn map_core_node_to_anf_preserves_source_ref_and_name() {
        let node = CoreNode {
            source_ref: NodeRef(7),
            kind: CoreNodeKind::Function,
            name: "fn_x".to_string(),
            ty: None,
            expr: None,
        };
        let mut fresh = 0u32;
        let mut out = Vec::new();
        map_core_node_to_anf(&node, &mut fresh, &mut out);
        let binding = out.into_iter().next().expect("must produce one binding");
        assert_eq!(binding.source_ref, NodeRef(7));
        assert_eq!(binding.name, "fn_x");
    }

    // ── lower_to_core_ir ──────────────────────────────────────────────────

    #[test]
    fn rejected_report_returns_rejected_error() {
        let graph = one_node_graph();
        let report = report_with_state(VerificationState::Failed);
        assert_eq!(
            lower_to_core_ir(&graph, &report),
            Err(CompileError::RejectedReport)
        );
    }

    #[test]
    fn accepted_report_returns_core_ir() {
        let graph = one_node_graph();
        let report = proven_report();
        let result = lower_to_core_ir(&graph, &report);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(result.unwrap().nodes.len(), 1);
    }

    // ── lower_to_anf ─────────────────────────────────────────────────────

    #[test]
    fn lower_to_anf_produces_one_binding_per_core_node() {
        let graph = one_node_graph();
        let report = proven_report();
        let core = lower_to_core_ir(&graph, &report).unwrap();
        let anf = lower_to_anf(&core).unwrap();
        assert_eq!(anf.bindings.len(), 1);
        assert_eq!(anf.bindings[0].source_ref, NodeRef(0));
    }

    #[test]
    fn anf_ir_hash_is_set_after_lowering() {
        let graph = one_node_graph();
        let report = proven_report();
        let core = lower_to_core_ir(&graph, &report).unwrap();
        let anf = lower_to_anf(&core).unwrap();
        assert!(anf.stage_hashes.anf_ir_hash.is_some());
    }

    // ── nominal_to_core_type ─────────────────────────────────────────────

    // S6 (partial): all 20 known nominals map to their CoreType variant.
    #[test]
    fn all_known_nominals_map_to_correct_core_type() {
        let cases: &[(&str, CoreType)] = &[
            ("Unit", CoreType::Unit),
            ("Never", CoreType::Never),
            ("Bool", CoreType::Bool),
            ("Int", CoreType::Int),
            ("UInt", CoreType::UInt),
            ("Float", CoreType::Float),
            ("Text", CoreType::Text),
            ("Bytes", CoreType::Bytes),
            ("Record", CoreType::Record),
            ("Variant", CoreType::Variant),
            ("Tuple", CoreType::Tuple),
            ("List", CoreType::List(Box::new(CoreType::Generic(None)))),
            ("Map", CoreType::Map(Box::new(CoreType::Generic(None)), Box::new(CoreType::Generic(None)))),
            ("Set", CoreType::Set(Box::new(CoreType::Generic(None)))),
            ("Option", CoreType::Option(Box::new(CoreType::Generic(None)))),
            ("Result", CoreType::Result(Box::new(CoreType::Generic(None)), Box::new(CoreType::Generic(None)))),
            ("Function", CoreType::Function { params: vec![], ret: Box::new(CoreType::Generic(None)), effects: vec![] }),
            ("Handle", CoreType::Handle { resource: Box::new(CoreType::Generic(None)), mode: ResourceMode::Copy }),
            ("Refinement", CoreType::Refinement { base: Box::new(CoreType::Generic(None)), predicate: String::new() }),
            ("Generic", CoreType::Generic(None)),
        ];
        for (nominal, expected) in cases {
            assert_eq!(
                nominal_to_core_type(nominal),
                *expected,
                "nominal {nominal:?} must map to {expected:?}"
            );
        }
    }

    // S7: unknown nominal falls back to CoreType::Generic(None).
    #[test]
    fn unknown_nominal_maps_to_generic() {
        assert_eq!(nominal_to_core_type("Exotic"), CoreType::Generic(None));
        assert_eq!(nominal_to_core_type(""), CoreType::Generic(None));
        assert_eq!(nominal_to_core_type("int"), CoreType::Generic(None)); // case-sensitive
    }

    // ── G2 lower_to_core_ir with type_facts ──────────────────────────────

    // S6: lower_to_core_ir populates CoreType::Int for a node with
    // type_facts.nominal = "Int".
    #[test]
    fn lower_to_core_ir_populates_core_type_from_type_facts() {
        use ail_core::semantic_graph::TypeFacts;

        let mut node = GraphNode::new(NodeRef(0), NodeKind::Type, "amount");
        node.type_facts = Some(TypeFacts {
            nominal: "Int".to_string(),
            generics: vec![],
        });
        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };
        let report = proven_report();
        let core = lower_to_core_ir(&graph, &report).unwrap();
        assert_eq!(
            core.nodes[0].ty,
            Some(CoreType::Int),
            "node with TypeFacts.nominal=Int must get ty=Some(CoreType::Int)"
        );
    }

    #[test]
    fn lower_to_core_ir_maps_literal_function_value_to_expr() {
        use ail_core::semantic_graph::RuntimeCheckMeta;

        let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.answer");
        node.return_type = Some("Int".to_string());
        node.runtime_checks = Some(vec![RuntimeCheckMeta {
            predicate: "literal:i64=42".to_string(),
            hash: "literal-hash".to_string(),
        }]);
        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };

        let core = lower_to_core_ir(&graph, &proven_report()).unwrap();

        assert_eq!(
            core.nodes[0].expr,
            Some(CoreExpr::Literal(LiteralValue::Int(42)))
        );
    }

    #[test]
    fn lower_to_core_ir_parses_function_body_expr() {
        let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.add");
        node.body_expr = Some("add(x, y)".to_string());
        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };

        let core = lower_to_core_ir(&graph, &proven_report()).unwrap();

        assert_eq!(
            core.nodes[0].expr,
            Some(CoreExpr::Add(
                Box::new(CoreExpr::Var("x".to_string())),
                Box::new(CoreExpr::Var("y".to_string()))
            ))
        );
    }

    // S8: lower_to_core_ir leaves ty = None for nodes without type_facts.
    #[test]
    fn lower_to_core_ir_leaves_ty_none_without_type_facts() {
        let graph = one_node_graph(); // GraphNode::new — type_facts is None
        let report = proven_report();
        let core = lower_to_core_ir(&graph, &report).unwrap();
        assert_eq!(
            core.nodes[0].ty, None,
            "node without TypeFacts must have ty=None"
        );
    }

    // S7 (lowering): lower_to_core_ir uses Generic for unknown nominals.
    #[test]
    fn lower_to_core_ir_uses_generic_for_unknown_nominal() {
        use ail_core::semantic_graph::TypeFacts;

        let mut node = GraphNode::new(NodeRef(0), NodeKind::Type, "exotic");
        node.type_facts = Some(TypeFacts {
            nominal: "Exotic".to_string(),
            generics: vec![],
        });
        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };
        let report = proven_report();
        let core = lower_to_core_ir(&graph, &report).unwrap();
        assert_eq!(
            core.nodes[0].ty,
            Some(CoreType::Generic(None)),
            "unknown nominal must produce CoreType::Generic(None)"
        );
    }

    // expr is always None after lower_to_core_ir (deferred phase).
    #[test]
    fn lower_to_core_ir_expr_is_always_none() {
        let graph = one_node_graph();
        let report = proven_report();
        let core = lower_to_core_ir(&graph, &report).unwrap();
        assert!(
            core.nodes[0].expr.is_none(),
            "expr must be None after lower_to_core_ir (deferred to expression lowering)"
        );
    }

    // ── G20: Expression body lowering tests ──────────────────────────────

    // Helper: lower a single CoreExpr to AnfExpr (no prior bindings).
    fn lower_single(expr: &CoreExpr) -> (AnfExpr, Vec<AnfBinding>) {
        let mut fresh = 0u32;
        let mut out: Vec<AnfBinding> = Vec::new();
        let result = lower_core_expr_to_anf(expr, &mut fresh, NodeRef(0), &mut out);
        (result, out)
    }

    // S1: Match — scrutinee Var is preserved as atomic name.
    #[test]
    fn lower_match_var_scrutinee_is_preserved() {
        use crate::core_ir::MatchArm;
        let expr = CoreExpr::Match {
            scrutinee: Box::new(CoreExpr::Var("payment".to_string())),
            arms: vec![MatchArm {
                pattern: "Ok(r)".to_string(),
                body: CoreExpr::Var("r".to_string()),
            }],
        };
        let (result, out) = lower_single(&expr);
        // Scrutinee is already Var, so no extra bindings emitted.
        assert!(
            out.is_empty(),
            "Var scrutinee must not produce extra bindings"
        );
        match result {
            crate::anf::AnfExpr::Match { scrutinee, arms } => {
                assert_eq!(scrutinee, "payment");
                assert_eq!(arms.len(), 1);
                assert_eq!(arms[0].pattern, "Ok(r)");
            }
            other => panic!("expected AnfExpr::Match, got {other:?}"),
        }
    }

    // S1b: Match — non-Var scrutinee is atomized (produces synthetic binding).
    #[test]
    fn lower_match_complex_scrutinee_is_atomized() {
        use crate::core_ir::MatchArm;
        let expr = CoreExpr::Match {
            scrutinee: Box::new(CoreExpr::Literal(LiteralValue::Int(42))),
            arms: vec![MatchArm {
                pattern: "_".to_string(),
                body: CoreExpr::Literal(LiteralValue::Unit),
            }],
        };
        let (result, out) = lower_single(&expr);
        // Literal scrutinee must be atomized → one synthetic binding.
        assert!(
            !out.is_empty(),
            "Literal scrutinee must produce a synthetic binding"
        );
        match result {
            crate::anf::AnfExpr::Match { scrutinee, .. } => {
                // scrutinee must be the synthetic name, not "42"
                assert!(
                    scrutinee.starts_with("anf_"),
                    "scrutinee must be synthetic name, got {scrutinee}"
                );
            }
            other => panic!("expected AnfExpr::Match, got {other:?}"),
        }
    }

    // S2: Lambda — params and body lowered correctly.
    #[test]
    fn lower_lambda_params_and_body() {
        let expr = CoreExpr::Lambda {
            params: vec!["x".to_string(), "y".to_string()],
            body: Box::new(CoreExpr::Var("x".to_string())),
        };
        let (result, out) = lower_single(&expr);
        assert!(
            out.is_empty(),
            "Lambda body Var must not produce extra bindings"
        );
        match result {
            crate::anf::AnfExpr::Lambda { params, body } => {
                assert_eq!(params, vec!["x", "y"]);
                assert_eq!(*body, crate::anf::AnfExpr::Var("x".to_string()));
            }
            other => panic!("expected AnfExpr::Lambda, got {other:?}"),
        }
    }

    // S3: RecordNew — field values are fully ANF-normalized (let-bound atomics).
    //
    // Full ANF normalization: non-Var field values must be let-bound before use.
    // A Var field still passes through atomize but returns the same name without
    // producing an extra binding.  A Literal field WILL produce a synthetic
    // binding (anf_0) and the field value will be Var("anf_0").
    #[test]
    fn lower_record_new_field_values() {
        let expr = CoreExpr::RecordNew {
            fields: vec![
                (
                    "amount".to_string(),
                    CoreExpr::Literal(LiteralValue::Int(10)),
                ),
                ("label".to_string(), CoreExpr::Var("lbl".to_string())),
            ],
        };
        let (result, out) = lower_single(&expr);
        // Literal field must produce one synthetic binding.
        assert_eq!(
            out.len(),
            1,
            "Literal field must produce one synthetic binding, got {out:?}"
        );
        assert_eq!(
            out[0].expr,
            crate::anf::AnfExpr::Literal(LiteralValue::Int(10))
        );
        let synthetic_name = out[0].name.clone();
        match result {
            crate::anf::AnfExpr::RecordNew { fields } => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].0, "amount");
                // Literal field → Var(synthetic_name)
                assert_eq!(fields[0].1, crate::anf::AnfExpr::Var(synthetic_name));
                assert_eq!(fields[1].0, "label");
                // Var field → Var("lbl") (same name, no extra binding)
                assert_eq!(fields[1].1, crate::anf::AnfExpr::Var("lbl".to_string()));
            }
            other => panic!("expected AnfExpr::RecordNew, got {other:?}"),
        }
    }

    // S4: FieldUpdate — record Var is preserved as atomic name;
    //     value is also atomized (full ANF normalization).
    #[test]
    fn lower_field_update_var_record_is_preserved() {
        let expr = CoreExpr::FieldUpdate {
            record: Box::new(CoreExpr::Var("order".to_string())),
            field: "status".to_string(),
            value: Box::new(CoreExpr::Literal(LiteralValue::Text("Paid".to_string()))),
        };
        let (result, out) = lower_single(&expr);
        // Literal value must produce one synthetic binding.
        assert_eq!(
            out.len(),
            1,
            "Literal value must produce one synthetic binding"
        );
        let value_name = out[0].name.clone();
        match result {
            crate::anf::AnfExpr::FieldUpdate {
                record,
                field,
                value,
            } => {
                assert_eq!(record, "order");
                assert_eq!(field, "status");
                // Value is now a Var referring to the synthetic binding.
                assert_eq!(*value, crate::anf::AnfExpr::Var(value_name));
            }
            other => panic!("expected AnfExpr::FieldUpdate, got {other:?}"),
        }
    }

    // S5: TupleNew — elements are lowered recursively.
    #[test]
    fn lower_tuple_new_elements() {
        let expr = CoreExpr::TupleNew(vec![
            CoreExpr::Var("a".to_string()),
            CoreExpr::Var("b".to_string()),
        ]);
        let (result, _out) = lower_single(&expr);
        match result {
            crate::anf::AnfExpr::TupleNew(elems) => {
                assert_eq!(elems.len(), 2);
                assert_eq!(elems[0], crate::anf::AnfExpr::Var("a".to_string()));
                assert_eq!(elems[1], crate::anf::AnfExpr::Var("b".to_string()));
            }
            other => panic!("expected AnfExpr::TupleNew, got {other:?}"),
        }
    }

    // S6: VariantNew with payload.
    #[test]
    fn lower_variant_new_with_payload() {
        let expr = CoreExpr::VariantNew {
            tag: "Ok".to_string(),
            payload: Some(Box::new(CoreExpr::Var("x".to_string()))),
        };
        let (result, out) = lower_single(&expr);
        assert!(
            out.is_empty(),
            "Var payload must not produce extra bindings"
        );
        match result {
            crate::anf::AnfExpr::VariantNew { tag, payload } => {
                assert_eq!(tag, "Ok");
                assert_eq!(*payload.unwrap(), crate::anf::AnfExpr::Var("x".to_string()));
            }
            other => panic!("expected AnfExpr::VariantNew, got {other:?}"),
        }
    }

    // S6b: VariantNew without payload.
    #[test]
    fn lower_variant_new_no_payload() {
        let expr = CoreExpr::VariantNew {
            tag: "None".to_string(),
            payload: None,
        };
        let (result, _out) = lower_single(&expr);
        match result {
            crate::anf::AnfExpr::VariantNew { tag, payload } => {
                assert_eq!(tag, "None");
                assert!(payload.is_none());
            }
            other => panic!("expected AnfExpr::VariantNew, got {other:?}"),
        }
    }

    // S7: ListNew — elements are fully ANF-normalized (let-bound atomics).
    //
    // Full ANF normalization: non-Var elements are let-bound.
    // Literal(1) → synthetic binding anf_0 → Var("anf_0")
    // Var("x")   → passes through atomize as "x" → Var("x")
    #[test]
    fn lower_list_new_elements() {
        let expr = CoreExpr::ListNew(vec![
            CoreExpr::Literal(LiteralValue::Int(1)),
            CoreExpr::Var("x".to_string()),
        ]);
        let (result, out) = lower_single(&expr);
        // Literal element must produce one synthetic binding.
        assert_eq!(
            out.len(),
            1,
            "Literal element must produce one synthetic binding"
        );
        let lit_name = out[0].name.clone();
        match result {
            crate::anf::AnfExpr::ListNew(elems) => {
                assert_eq!(elems.len(), 2);
                assert_eq!(elems[0], crate::anf::AnfExpr::Var(lit_name));
                assert_eq!(elems[1], crate::anf::AnfExpr::Var("x".to_string()));
            }
            other => panic!("expected AnfExpr::ListNew, got {other:?}"),
        }
    }

    // S8: No Placeholder produced for real CoreExpr variants.
    #[test]
    fn real_core_exprs_do_not_produce_placeholder() {
        use crate::core_ir::MatchArm;
        let real_exprs = vec![
            CoreExpr::Match {
                scrutinee: Box::new(CoreExpr::Var("x".to_string())),
                arms: vec![MatchArm {
                    pattern: "_".to_string(),
                    body: CoreExpr::Literal(LiteralValue::Unit),
                }],
            },
            CoreExpr::Lambda {
                params: vec![],
                body: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
            },
            CoreExpr::RecordNew { fields: vec![] },
            CoreExpr::FieldUpdate {
                record: Box::new(CoreExpr::Var("r".to_string())),
                field: "f".to_string(),
                value: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
            },
            CoreExpr::TupleNew(vec![]),
            CoreExpr::VariantNew {
                tag: "A".to_string(),
                payload: None,
            },
            CoreExpr::ListNew(vec![]),
        ];
        for expr in &real_exprs {
            let (result, _out) = lower_single(expr);
            assert_ne!(
                result,
                crate::anf::AnfExpr::Placeholder,
                "CoreExpr::{expr:?} must NOT produce Placeholder"
            );
        }
    }

    // S9: CoreExpr::Placeholder still produces AnfExpr::Placeholder.
    #[test]
    fn placeholder_still_maps_to_placeholder() {
        let (result, _out) = lower_single(&CoreExpr::Placeholder);
        assert_eq!(result, crate::anf::AnfExpr::Placeholder);
    }

    // ── G23: Lowering tests for new concurrency + cell primitives ─────────

    // TaskAwait: Var task → no synthetic bindings, atomic name preserved.
    #[test]
    fn lower_task_await_var_is_preserved() {
        let expr = CoreExpr::TaskAwait {
            task: Box::new(CoreExpr::Var("task_0".to_string())),
        };
        let (result, out) = lower_single(&expr);
        assert!(
            out.is_empty(),
            "Var task must not produce synthetic bindings"
        );
        match result {
            crate::anf::AnfExpr::TaskAwait { task } => {
                assert_eq!(task, "task_0");
            }
            other => panic!("expected TaskAwait, got {other:?}"),
        }
    }

    // TRIANGULATE: TaskAwait with non-Var task atomizes it.
    #[test]
    fn lower_task_await_complex_task_is_atomized() {
        let expr = CoreExpr::TaskAwait {
            task: Box::new(CoreExpr::Call {
                func: "fn.spawn_work".to_string(),
                args: vec![],
            }),
        };
        let (result, out) = lower_single(&expr);
        assert!(
            !out.is_empty(),
            "non-Var task must produce a synthetic binding"
        );
        match result {
            crate::anf::AnfExpr::TaskAwait { task } => {
                assert!(task.starts_with("anf_"), "task must be synthetic: {task}");
            }
            other => panic!("expected TaskAwait, got {other:?}"),
        }
    }

    // TaskCancel: Var task → no synthetic bindings.
    #[test]
    fn lower_task_cancel_var_is_preserved() {
        let expr = CoreExpr::TaskCancel {
            task: Box::new(CoreExpr::Var("t".to_string())),
        };
        let (result, out) = lower_single(&expr);
        assert!(out.is_empty());
        match result {
            crate::anf::AnfExpr::TaskCancel { task } => {
                assert_eq!(task, "t");
            }
            other => panic!("expected TaskCancel, got {other:?}"),
        }
    }

    // TaskGroup: body is lowered recursively.
    #[test]
    fn lower_task_group_body_is_lowered() {
        let expr = CoreExpr::TaskGroup {
            body: Box::new(CoreExpr::Var("spawner".to_string())),
        };
        let (result, out) = lower_single(&expr);
        assert!(
            out.is_empty(),
            "Var body must not produce synthetic bindings"
        );
        match result {
            crate::anf::AnfExpr::TaskGroup { body } => {
                assert_eq!(*body, crate::anf::AnfExpr::Var("spawner".to_string()));
            }
            other => panic!("expected TaskGroup, got {other:?}"),
        }
    }

    // ChannelNew unbounded: no sub-expressions to lower.
    #[test]
    fn lower_channel_new_unbounded() {
        let expr = CoreExpr::ChannelNew { capacity: None };
        let (result, out) = lower_single(&expr);
        assert!(
            out.is_empty(),
            "ChannelNew must produce no synthetic bindings"
        );
        match result {
            crate::anf::AnfExpr::ChannelNew { capacity } => {
                assert!(capacity.is_none());
            }
            other => panic!("expected ChannelNew, got {other:?}"),
        }
    }

    // TRIANGULATE: ChannelNew bounded preserves capacity.
    #[test]
    fn lower_channel_new_bounded_preserves_capacity() {
        let expr = CoreExpr::ChannelNew { capacity: Some(64) };
        let (result, out) = lower_single(&expr);
        assert!(out.is_empty());
        match result {
            crate::anf::AnfExpr::ChannelNew { capacity } => {
                assert_eq!(capacity, Some(64));
            }
            other => panic!("expected ChannelNew, got {other:?}"),
        }
    }

    // Select: Var channel → no synthetic bindings; clause fields preserved.
    #[test]
    fn lower_select_var_channel_is_preserved() {
        use crate::core_ir::SelectClause;
        let expr = CoreExpr::Select {
            branches: vec![SelectClause {
                channel: Box::new(CoreExpr::Var("inbox".to_string())),
                binding: "item".to_string(),
                body: CoreExpr::Var("item".to_string()),
            }],
        };
        let (result, out) = lower_single(&expr);
        assert!(
            out.is_empty(),
            "Var channel must not produce synthetic bindings"
        );
        match result {
            crate::anf::AnfExpr::Select { branches } => {
                assert_eq!(branches.len(), 1);
                assert_eq!(branches[0].channel, "inbox");
                assert_eq!(branches[0].binding, "item");
                assert_eq!(
                    branches[0].body,
                    crate::anf::AnfExpr::Var("item".to_string())
                );
            }
            other => panic!("expected Select, got {other:?}"),
        }
    }

    // TRIANGULATE: Select with non-Var channel atomizes it.
    #[test]
    fn lower_select_complex_channel_is_atomized() {
        use crate::core_ir::SelectClause;
        let expr = CoreExpr::Select {
            branches: vec![SelectClause {
                channel: Box::new(CoreExpr::Call {
                    func: "fn.get_channel".to_string(),
                    args: vec![],
                }),
                binding: "v".to_string(),
                body: CoreExpr::Literal(LiteralValue::Unit),
            }],
        };
        let (result, out) = lower_single(&expr);
        assert!(
            !out.is_empty(),
            "non-Var channel must produce a synthetic binding"
        );
        match result {
            crate::anf::AnfExpr::Select { branches } => {
                assert!(branches[0].channel.starts_with("anf_"));
            }
            other => panic!("expected Select, got {other:?}"),
        }
    }

    // Timeout: Var duration → no synthetic bindings; body lowered recursively.
    #[test]
    fn lower_timeout_var_duration_is_preserved() {
        let expr = CoreExpr::Timeout {
            duration: Box::new(CoreExpr::Var("ms".to_string())),
            body: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
        };
        let (result, out) = lower_single(&expr);
        assert!(
            out.is_empty(),
            "Var duration must not produce synthetic bindings"
        );
        match result {
            crate::anf::AnfExpr::Timeout { duration, body } => {
                assert_eq!(duration, "ms");
                assert_eq!(*body, crate::anf::AnfExpr::Literal(LiteralValue::Unit));
            }
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    // TRIANGULATE: Timeout with non-Var duration atomizes it.
    #[test]
    fn lower_timeout_complex_duration_is_atomized() {
        let expr = CoreExpr::Timeout {
            duration: Box::new(CoreExpr::Literal(LiteralValue::Int(5000))),
            body: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
        };
        let (result, out) = lower_single(&expr);
        assert!(
            !out.is_empty(),
            "Literal duration must produce a synthetic binding"
        );
        match result {
            crate::anf::AnfExpr::Timeout { duration, .. } => {
                assert!(
                    duration.starts_with("anf_"),
                    "duration must be synthetic: {duration}"
                );
            }
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    // CellNew: Var init → no synthetic bindings.
    #[test]
    fn lower_cell_new_var_init_is_preserved() {
        let expr = CoreExpr::CellNew {
            init: Box::new(CoreExpr::Var("zero".to_string())),
        };
        let (result, out) = lower_single(&expr);
        assert!(
            out.is_empty(),
            "Var init must not produce synthetic bindings"
        );
        match result {
            crate::anf::AnfExpr::CellNew { init } => {
                assert_eq!(init, "zero");
            }
            other => panic!("expected CellNew, got {other:?}"),
        }
    }

    // TRIANGULATE: CellNew with Literal init atomizes it.
    #[test]
    fn lower_cell_new_literal_init_is_atomized() {
        let expr = CoreExpr::CellNew {
            init: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
        };
        let (result, out) = lower_single(&expr);
        assert!(
            !out.is_empty(),
            "Literal init must produce a synthetic binding"
        );
        match result {
            crate::anf::AnfExpr::CellNew { init } => {
                assert!(init.starts_with("anf_"), "init must be synthetic: {init}");
            }
            other => panic!("expected CellNew, got {other:?}"),
        }
    }

    // CellGet: Var cell → no synthetic bindings.
    #[test]
    fn lower_cell_get_var_cell_is_preserved() {
        let expr = CoreExpr::CellGet {
            cell: Box::new(CoreExpr::Var("counter".to_string())),
        };
        let (result, out) = lower_single(&expr);
        assert!(out.is_empty());
        match result {
            crate::anf::AnfExpr::CellGet { cell } => {
                assert_eq!(cell, "counter");
            }
            other => panic!("expected CellGet, got {other:?}"),
        }
    }

    // CellSet: both Var cell and Var value → no synthetic bindings.
    #[test]
    fn lower_cell_set_var_operands_are_preserved() {
        let expr = CoreExpr::CellSet {
            cell: Box::new(CoreExpr::Var("c".to_string())),
            value: Box::new(CoreExpr::Var("v".to_string())),
        };
        let (result, out) = lower_single(&expr);
        assert!(
            out.is_empty(),
            "Var operands must not produce synthetic bindings"
        );
        match result {
            crate::anf::AnfExpr::CellSet { cell, value } => {
                assert_eq!(cell, "c");
                assert_eq!(value, "v");
            }
            other => panic!("expected CellSet, got {other:?}"),
        }
    }

    // TRIANGULATE: CellSet with non-Var cell and non-Var value atomizes both.
    #[test]
    fn lower_cell_set_literal_operands_are_atomized() {
        let expr = CoreExpr::CellSet {
            cell: Box::new(CoreExpr::Call {
                func: "fn.get_cell".to_string(),
                args: vec![],
            }),
            value: Box::new(CoreExpr::Literal(LiteralValue::Int(42))),
        };
        let (result, out) = lower_single(&expr);
        assert_eq!(
            out.len(),
            2,
            "two non-Var operands must produce two synthetic bindings"
        );
        match result {
            crate::anf::AnfExpr::CellSet { cell, value } => {
                assert!(cell.starts_with("anf_"), "cell must be synthetic: {cell}");
                assert!(
                    value.starts_with("anf_"),
                    "value must be synthetic: {value}"
                );
            }
            other => panic!("expected CellSet, got {other:?}"),
        }
    }
}
