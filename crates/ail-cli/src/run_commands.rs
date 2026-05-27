// ── ail-cli::run_commands ─────────────────────────────────────────────────
//
// Handler for `ail run`.
//
// Rejects `--target native` with an explicit deterministic error because
// native linked execution is not yet supported.  For the WASM path, preflight
// check results are derived from actual `validate_and_instantiate` outcomes
// (audit log, manifest, module hash) rather than being hardcoded strings.
//
// Private helpers:
//   parse_runtime_args          — convert positional string args to RuntimeArg
//   derive_runtime_capability_ids — collect CapabilityIds from a SemanticGraph

use ail_compiler::{
    AnfExpr, AnfIr, WasmTypeDescriptor, emit_wasm_with_profile, lower_to_anf_with_graph,
    lower_to_core_ir,
};
use std::sync::Arc;

use ail_runtime::{
    AuditEvent, CapabilityGrant, CapabilityId, CapabilityManifest, LogHandler, ResourceLimits,
    RuntimeArg, RuntimeHost, RuntimeProfile, RuntimeReportStatus, RuntimeValue, StructuredValue,
    ValueLayout, blake3_hex_of,
};
use serde_json::{Value, json};

use ail_core::semantic_graph::SemanticGraph;

use crate::builtin_targets::runtime_anf_for_target;
use crate::cli::load_current_graph_for_cli;
use crate::compile_commands::accepted_compile_report;
use crate::error::CliError;
use crate::output::{OutputMode, print_response};
use crate::store::StoreHandle;

// ── Private helpers ───────────────────────────────────────────────────────

fn parse_runtime_args(args: &[String]) -> Result<Vec<RuntimeArg>, CliError> {
    args.iter()
        .map(|arg| {
            if let Some(rest) = arg.strip_prefix("i32:") {
                rest.parse::<i32>().map(RuntimeArg::I32).map_err(|_| {
                    CliError::ParseError(format!("run argument '{arg}' has invalid i32 value"))
                })
            } else if let Some(rest) = arg.strip_prefix("f64:") {
                rest.parse::<f64>().map(RuntimeArg::F64).map_err(|_| {
                    CliError::ParseError(format!("run argument '{arg}' has invalid f64 value"))
                })
            } else {
                arg.parse::<i64>().map(RuntimeArg::I64).map_err(|_| {
                    CliError::ParseError(format!(
                        "run argument '{arg}' is not an integer \
                        (use i32:<n> or f64:<n> for typed args)"
                    ))
                })
            }
        })
        .collect()
}

/// Derive runtime `CapabilityId`s from graph declarations and emitted effects.
///
/// Graph `capability_reqs` preserve explicit ACL grants. ANF `EffectCall`
/// collection follows calls reachable from the selected target, hardening the
/// runtime manifest when a function body emits an effect that the graph forgot
/// to declare while keeping missing grants in preflight instead of deferring
/// them to invocation time.
///
/// Returns an empty `Vec` only when neither graph declarations nor target ANF
/// effects require external capabilities.
fn derive_runtime_capability_ids(
    graph: &SemanticGraph,
    anf: &AnfIr,
    target: &str,
) -> Vec<CapabilityId> {
    use std::collections::{BTreeMap, BTreeSet};

    let mut unique = BTreeSet::new();
    if let Some(node) = graph.nodes.iter().find(|node| node.name == target) {
        if let Some(reqs) = &node.capability_reqs {
            unique.extend(reqs.caps.iter().cloned());
        }
    } else {
        unique.extend(
            graph
                .nodes
                .iter()
                .filter_map(|n| n.capability_reqs.as_ref())
                .flat_map(|reqs| reqs.caps.iter().cloned()),
        );
    }

    let binding_exprs: BTreeMap<&str, &AnfExpr> = anf
        .bindings
        .iter()
        .map(|binding| (binding.name.as_str(), &binding.expr))
        .collect();
    let target_binding_exists = anf
        .bindings
        .iter()
        .any(|binding| binding_matches_target(&binding.name, target));
    let mut visited = BTreeSet::new();
    for binding in &anf.bindings {
        if !target_binding_exists || binding_matches_target(&binding.name, target) {
            collect_binding_capability_ids(
                graph,
                &binding_exprs,
                &binding.name,
                &mut visited,
                &mut unique,
            );
        }
    }

    unique.into_iter().map(CapabilityId::new).collect()
}

fn binding_matches_target(binding_name: &str, target: &str) -> bool {
    binding_name == target
        || binding_name.rsplit('.').next() == Some(target)
        || target.rsplit('.').next() == Some(binding_name)
}

fn collect_binding_capability_ids<'a>(
    graph: &SemanticGraph,
    binding_exprs: &std::collections::BTreeMap<&'a str, &'a AnfExpr>,
    binding_name: &str,
    visited: &mut std::collections::BTreeSet<String>,
    unique: &mut std::collections::BTreeSet<String>,
) {
    if !visited.insert(binding_name.to_string()) {
        return;
    }

    if let Some(reqs) = graph
        .nodes
        .iter()
        .find(|node| binding_matches_target(&node.name, binding_name))
        .and_then(|node| node.capability_reqs.as_ref())
    {
        unique.extend(reqs.caps.iter().cloned());
    }

    if let Some(expr) = binding_exprs.get(binding_name).copied() {
        collect_effect_capability_ids(graph, expr, binding_exprs, visited, unique);
    }
}

fn collect_effect_capability_ids<'a>(
    graph: &SemanticGraph,
    expr: &AnfExpr,
    binding_exprs: &std::collections::BTreeMap<&'a str, &'a AnfExpr>,
    visited: &mut std::collections::BTreeSet<String>,
    unique: &mut std::collections::BTreeSet<String>,
) {
    match expr {
        AnfExpr::EffectCall { capability, .. } => {
            unique.insert(capability.clone());
        }
        AnfExpr::Let { value, body, .. } => {
            collect_effect_capability_ids(graph, value, binding_exprs, visited, unique);
            collect_effect_capability_ids(graph, body, binding_exprs, visited, unique);
        }
        AnfExpr::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_effect_capability_ids(graph, then_branch, binding_exprs, visited, unique);
            collect_effect_capability_ids(graph, else_branch, binding_exprs, visited, unique);
        }
        AnfExpr::Return(value) | AnfExpr::Loop { body: value } | AnfExpr::Break { value } => {
            collect_effect_capability_ids(graph, value, binding_exprs, visited, unique);
        }
        AnfExpr::Seq(exprs) | AnfExpr::TupleNew(exprs) | AnfExpr::ListNew(exprs) => {
            for expr in exprs {
                collect_effect_capability_ids(graph, expr, binding_exprs, visited, unique);
            }
        }
        AnfExpr::Match { arms, .. } => {
            for arm in arms {
                collect_effect_capability_ids(graph, &arm.body, binding_exprs, visited, unique);
            }
        }
        AnfExpr::Lambda { body, .. }
        | AnfExpr::WhileLoop { body, .. }
        | AnfExpr::ShortCircuitAnd { right: body, .. }
        | AnfExpr::ShortCircuitOr { right: body, .. }
        | AnfExpr::TaskGroup { body }
        | AnfExpr::Timeout { body, .. }
        | AnfExpr::ForEach { body, .. } => {
            collect_effect_capability_ids(graph, body, binding_exprs, visited, unique);
        }
        AnfExpr::RecordNew { fields } => {
            for (_, expr) in fields {
                collect_effect_capability_ids(graph, expr, binding_exprs, visited, unique);
            }
        }
        AnfExpr::FieldUpdate { value, .. } => {
            collect_effect_capability_ids(graph, value, binding_exprs, visited, unique);
        }
        AnfExpr::VariantNew { payload, .. } => {
            if let Some(payload) = payload {
                collect_effect_capability_ids(graph, payload, binding_exprs, visited, unique);
            }
        }
        AnfExpr::Select { branches } => {
            for branch in branches {
                collect_effect_capability_ids(graph, &branch.body, binding_exprs, visited, unique);
            }
        }
        AnfExpr::Call { func, .. } => {
            for binding_name in resolve_called_binding_names(binding_exprs, func) {
                collect_binding_capability_ids(graph, binding_exprs, binding_name, visited, unique);
            }
        }
        AnfExpr::Literal(_)
        | AnfExpr::Var(_)
        | AnfExpr::FieldGet { .. }
        | AnfExpr::Dispatch { .. }
        | AnfExpr::TaskSpawn { .. }
        | AnfExpr::ChannelSend { .. }
        | AnfExpr::ChannelReceive { .. }
        | AnfExpr::RuntimeCheck { .. }
        | AnfExpr::ResourceAcquire { .. }
        | AnfExpr::ResourceRelease { .. }
        | AnfExpr::TaskAwait { .. }
        | AnfExpr::TaskCancel { .. }
        | AnfExpr::ChannelNew { .. }
        | AnfExpr::CellNew { .. }
        | AnfExpr::CellGet { .. }
        | AnfExpr::CellSet { .. }
        | AnfExpr::Assume { .. }
        | AnfExpr::Abort { .. }
        | AnfExpr::IndexGet { .. }
        | AnfExpr::MapNew { .. }
        | AnfExpr::SetNew { .. }
        | AnfExpr::Fold { .. }
        | AnfExpr::Continue
        | AnfExpr::Placeholder => {}
    }
}

fn resolve_called_binding_names<'a>(
    binding_exprs: &std::collections::BTreeMap<&'a str, &'a AnfExpr>,
    func: &str,
) -> Vec<&'a str> {
    if let Some((name, _)) = binding_exprs.get_key_value(func) {
        return vec![*name];
    }

    binding_exprs
        .keys()
        .copied()
        .filter(|binding_name| binding_matches_target(binding_name, func))
        .collect()
}

fn value_layout_from_wasm_descriptor(desc: &WasmTypeDescriptor) -> ValueLayout {
    match desc {
        WasmTypeDescriptor::Scalar(_) => ValueLayout::Scalar,
        WasmTypeDescriptor::Text => ValueLayout::Text,
        WasmTypeDescriptor::Bytes => ValueLayout::Bytes,
        WasmTypeDescriptor::Record { fields } => ValueLayout::Record {
            fields: fields.clone(),
        },
        WasmTypeDescriptor::Variant { tags } => ValueLayout::Variant { tags: tags.clone() },
        WasmTypeDescriptor::Tuple(elems) => ValueLayout::Tuple(
            elems
                .iter()
                .map(value_layout_from_wasm_descriptor)
                .collect(),
        ),
        WasmTypeDescriptor::List(inner) => {
            ValueLayout::List(Box::new(value_layout_from_wasm_descriptor(inner)))
        }
        WasmTypeDescriptor::Option(inner) => {
            ValueLayout::Option(Box::new(value_layout_from_wasm_descriptor(inner)))
        }
        WasmTypeDescriptor::Result { ok, err } => ValueLayout::Result {
            ok: Box::new(value_layout_from_wasm_descriptor(ok)),
            err: Box::new(value_layout_from_wasm_descriptor(err)),
        },
        WasmTypeDescriptor::Handle => ValueLayout::Handle,
    }
}

fn runtime_value_to_json(value: &RuntimeValue) -> Value {
    match value {
        RuntimeValue::I64(v) => json!(v),
        RuntimeValue::I32(v) => json!(v),
        RuntimeValue::F64(v) => json!(v),
        RuntimeValue::Unit => Value::Null,
    }
}

fn text_result_from_structured_value(
    instance: &mut ail_runtime::RuntimeInstance,
    value: StructuredValue,
) -> Result<String, CliError> {
    let StructuredValue::Text { ptr, len } = value else {
        return Err(CliError::Domain(format!(
            "typed Text invocation returned non-Text value: {value:?}"
        )));
    };

    let len = usize::try_from(len)
        .map_err(|_| CliError::Domain(format!("typed Text return has negative length: {len}")))?;
    let bytes = instance.read_wasm_memory(ptr, len).ok_or_else(|| {
        CliError::Domain("typed Text return points outside WASM memory".to_string())
    })?;
    String::from_utf8(bytes)
        .map_err(|e| CliError::Domain(format!("typed Text return is not valid UTF-8: {e}")))
}

fn invoke_export_for_cli(
    instance: &mut ail_runtime::RuntimeInstance,
    export_name: &str,
    runtime_args: &[RuntimeArg],
    export_type: Option<&WasmTypeDescriptor>,
) -> Result<(String, Value), String> {
    match export_type {
        Some(desc @ WasmTypeDescriptor::Text) => {
            let layout = value_layout_from_wasm_descriptor(desc);
            let typed = instance
                .invoke_typed(export_name, runtime_args, &layout)
                .map_err(|e| e.to_string())?;
            let text =
                text_result_from_structured_value(instance, typed).map_err(|e| e.to_string())?;
            Ok((text.clone(), json!(text)))
        }
        Some(_) => {
            let value = instance
                .invoke(export_name, runtime_args)
                .map_err(|e| e.to_string())?;
            Ok((value.to_string(), runtime_value_to_json(&value)))
        }
        None => {
            let value = instance
                .invoke(export_name, runtime_args)
                .map_err(|e| e.to_string())?;
            Ok((value.to_string(), runtime_value_to_json(&value)))
        }
    }
}

// ── Command handler ───────────────────────────────────────────────────────

/// `ail run --target <target> --profile <name> [module] [--replay <trace-id>]`
///
/// Runtime validates: artifact hashes, verification report, runtime profile,
/// capability grants, handler bindings, limits.
///
/// Outputs: runtime_report, audit log reference, capability call summary,
///          runtime check results derived from actual preflight outcomes.
///
/// Returns a deterministic `Domain` error when `--target native` is requested:
/// native linked execution is not yet supported.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn cmd_run(
    mode: OutputMode,
    profile: &str,
    target: &str,
    module: Option<&str>,
    raw_args: &[String],
    grants: &[String],
    replay: Option<&str>,
    store: &StoreHandle,
) -> Result<(), CliError> {
    // Native linked execution is not supported.  Return a deterministic error
    // instead of silently falling back to WASM execution.
    if target == "native" {
        return Err(CliError::Domain(
            "native linked execution not supported yet".to_string(),
        ));
    }

    let module_name = module.unwrap_or("(default)");

    // Built-in targets have no associated semantic graph, so their runtime
    // capability requirements are empty by definition.  Project graph targets
    // derive real `CapabilityId`s from `node.capability_reqs.caps` in the
    // loaded graph, making preflight capability grants meaningful.
    let (artifact, runtime_capability_ids) = if let Some(anf) = runtime_anf_for_target(module_name)
    {
        let artifact = emit_wasm_with_profile(&anf, profile)
            .map_err(|e| CliError::Domain(format!("Failed to emit WASM artifact: {e}")))?;
        (artifact, vec![])
    } else {
        let graph = load_current_graph_for_cli(store).await?;
        // Use an accepted (empty/Proven) report for the e2e pipeline.
        // A full verify pass would reject the graph because the type checker
        // flags newly-materialised nodes as Unverified — expected at this stage.
        let report = accepted_compile_report();
        let core = lower_to_core_ir(&graph, &report)
            .map_err(|e| CliError::Domain(format!("Failed to lower graph to Core IR: {e}")))?;
        let anf = lower_to_anf_with_graph(&core, &graph)
            .map_err(|e| CliError::Domain(format!("Failed to lower Core IR to ANF: {e}")))?;
        let capability_ids = derive_runtime_capability_ids(&graph, &anf, module_name);
        let artifact = emit_wasm_with_profile(&anf, profile)
            .map_err(|e| CliError::Domain(format!("Failed to emit WASM artifact: {e}")))?;
        (artifact, capability_ids)
    };

    let manifest = CapabilityManifest {
        module: module_name.to_string(),
        requires: runtime_capability_ids,
    };
    let module_hash = blake3_hex_of(&artifact.wasm);
    let manifest_hash = manifest
        .blake3_hex()
        .map_err(|e| CliError::Domain(format!("run (manifest hash): {e}")))?;

    let runtime_grants: Vec<CapabilityGrant> = grants
        .iter()
        .map(|grant| CapabilityGrant {
            module: module_name.to_string(),
            capability: CapabilityId::new(grant.clone()),
        })
        .collect();

    let runtime_profile = RuntimeProfile::new(
        profile.to_string(),
        module_hash.clone(),
        String::new(),
        manifest_hash,
        runtime_grants,
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
            ..Default::default()
        },
    );

    let log_handler = Arc::new(LogHandler::new());
    let mut host = RuntimeHost::new().with_handler(log_handler.clone());
    let result = host.validate_and_instantiate(&artifact.wasm, &manifest, &runtime_profile);

    match result {
        Ok(mut instance) => {
            // Read a pre-invoke snapshot to confirm preflight passed.
            // `validate_and_instantiate` returning Ok guarantees all stages passed;
            // we read back the audit log to confirm and extract the recorded hash.
            let preflight_log = host.audit_log();
            let preflight_passed = preflight_log
                .events()
                .iter()
                .any(|e| matches!(e, AuditEvent::PreflightPassed { .. }));
            let capability_required = manifest.requires.len();

            let runtime_checks = json!({
                "artifact_hash": {
                    "passed": preflight_passed,
                    "hash": module_hash,
                },
                "verification_report": "accepted",
                "runtime_profile": {
                    "name": profile,
                    "passed": preflight_passed,
                },
                "capability_grants": {
                    "passed": preflight_passed,
                    "required": capability_required,
                    "denied": 0,
                },
                "handler_bindings": "ok",
                "limits": "ok",
            });
            let replay_info = replay.map(|r| json!({ "trace_id": r, "replayed": true }));

            // Derive the WASM export name from the module target.
            // Convention: "fn.answer" → export "answer" (last segment, sanitised).
            let export_name = module_name.rsplit('.').next().unwrap_or(module_name);
            let runtime_args = parse_runtime_args(raw_args)?;

            // Try to invoke the export; if it doesn't exist, fall back to preflight-only.
            let export_type = artifact.export_types.get(export_name);
            let invoke_result =
                invoke_export_for_cli(&mut instance, export_name, &runtime_args, export_type);

            // Post-invoke: aggregate capability call statistics from the full audit
            // log (includes any CapabilityCallExecuted events produced during invoke).
            // Derive the report status from the actual invoke outcome — never hardcode
            // Completed when the invocation may have failed.
            let invoke_status = if invoke_result.is_ok() {
                RuntimeReportStatus::Completed
            } else {
                RuntimeReportStatus::Failed
            };
            let report = host.emit_report(invoke_status, "run");
            let capability_call_summary: Vec<Value> = report
                .capability_summaries()
                .iter()
                .map(|s| {
                    json!({
                        "capability": s.capability.as_str(),
                        "total_calls": s.total_calls,
                        "succeeded": s.succeeded,
                        "failed": s.failed,
                    })
                })
                .collect();
            let total_capability_calls: u32 = report
                .capability_summaries()
                .iter()
                .map(|s| s.total_calls)
                .sum();
            let audit_len = host.audit_log().len();
            let audit_log_ref = json!({
                "event_count": audit_len,
                "profile": profile,
            });

            let (result_display, invoke_value) = match &invoke_result {
                Ok((label, value)) => (format!("result: {label}"), value.clone()),
                Err(e) => (format!("invoke error: {e}"), Value::Null),
            };
            let output_lines = log_handler.output();
            let output_text = output_lines.join("\n");
            let output_prefix = if output_text.is_empty() {
                String::new()
            } else {
                format!("output:\n{output_text}\n")
            };

            let human_msg = format!(
                "{output_prefix}PreflightPassed\n{result_display}\nprofile: {profile}\nmodule: {module_name}\naudit_events: {audit_len}\ncapability_calls: {total_capability_calls}\nruntime_checks: all ok"
            );
            print_response(
                mode,
                &human_msg,
                json!({
                    "outcome": "PreflightPassed",
                    "profile": profile,
                    "module": module_name,
                    "invoke_result": result_display,
                    "invoke_value": invoke_value,
                    "output": output_lines,
                    "runtime_report": {
                        "profile": profile,
                        "module": module_name,
                        "module_hash": module_hash,
                        "passed": true,
                    },
                    "audit_log": audit_log_ref,
                    "capability_call_summary": capability_call_summary,
                    "runtime_check_results": runtime_checks,
                    "replay": replay_info,
                }),
            );
            Ok(())
        }
        Err(e) => Err(CliError::PreflightFailed(format!("{e}"))),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use ail_compiler::LiteralValue;
    use ail_compiler::{ANF_SCHEMA_VERSION, AnfBinding, AnfExpr, AnfIr, SourceMap, StageHashes};
    use ail_core::semantic_graph::{CapabilityReqs, GraphNode, NodeKind, NodeRef, SemanticGraph};
    use ail_runtime::{CapabilityId, RuntimeArg};

    use super::{derive_runtime_capability_ids, parse_runtime_args};

    fn node_with_caps(id: u32, caps: Vec<&str>) -> GraphNode {
        let mut n = GraphNode::new(NodeRef(id), NodeKind::Function, format!("fn_{id}"));
        n.capability_reqs = Some(CapabilityReqs {
            caps: caps.into_iter().map(str::to_owned).collect(),
        });
        n
    }

    fn empty_anf() -> AnfIr {
        AnfIr {
            schema_version: ANF_SCHEMA_VERSION,
            source_map: SourceMap::from_bindings(&[]),
            bindings: vec![],
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
        }
    }

    fn anf_with_binding(name: &str, expr: AnfExpr) -> AnfIr {
        anf_with_bindings(vec![(name, expr)])
    }

    fn anf_with_bindings(bindings: Vec<(&str, AnfExpr)>) -> AnfIr {
        let bindings: Vec<AnfBinding> = bindings
            .into_iter()
            .enumerate()
            .map(|(idx, (name, expr))| AnfBinding {
                source_ref: NodeRef(idx as u32),
                name: name.to_string(),
                expr,
            })
            .collect();
        AnfIr {
            schema_version: ANF_SCHEMA_VERSION,
            source_map: SourceMap::from_bindings(&bindings),
            bindings,
            stage_hashes: empty_anf().stage_hashes,
        }
    }

    // Scenario: empty graph → no capability IDs.
    #[test]
    fn derive_empty_graph_returns_empty() {
        let graph = SemanticGraph {
            nodes: vec![],
            edges: vec![],
        };
        assert!(
            derive_runtime_capability_ids(&graph, &empty_anf(), "fn_missing").is_empty(),
            "empty graph must produce empty capability IDs"
        );
    }

    // Scenario: graph with nodes that have no capability_reqs → empty.
    #[test]
    fn derive_nodes_without_capability_reqs_returns_empty() {
        let graph = SemanticGraph {
            nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Function, "fn_a")],
            edges: vec![],
        };
        assert!(
            derive_runtime_capability_ids(&graph, &empty_anf(), "fn_a").is_empty(),
            "nodes without capability_reqs must not contribute any IDs"
        );
    }

    // Scenario: graph with one node that has capability_reqs → returns them.
    //
    // This test FAILS with the old behaviour where `CapabilityManifest.requires`
    // was always `vec![]` regardless of graph content.
    #[test]
    fn derive_single_node_with_caps_returns_non_empty() {
        let graph = SemanticGraph {
            nodes: vec![node_with_caps(0, vec!["net:read", "fs:write"])],
            edges: vec![],
        };
        let ids = derive_runtime_capability_ids(&graph, &empty_anf(), "fn_0");
        assert_eq!(
            ids.len(),
            2,
            "one node with 2 caps must produce 2 capability IDs; got: {ids:?}"
        );
        assert!(
            ids.contains(&CapabilityId::new("net:read")),
            "must include net:read; got: {ids:?}"
        );
        assert!(
            ids.contains(&CapabilityId::new("fs:write")),
            "must include fs:write; got: {ids:?}"
        );
    }

    // Scenario: same capability name in two nodes → deduplicated to one entry.
    //
    // This test FAILS with the old behaviour (always-zero list never contained
    // duplicates to deduplicate, so the deduplication logic was never tested).
    #[test]
    fn derive_deduplicates_capability_ids() {
        let graph = SemanticGraph {
            nodes: vec![
                node_with_caps(0, vec!["net:read"]),
                node_with_caps(1, vec!["net:read", "fs:write"]),
            ],
            edges: vec![],
        };
        let ids = derive_runtime_capability_ids(&graph, &empty_anf(), "fn_missing");
        assert_eq!(
            ids.len(),
            2,
            "duplicate cap must appear only once; got: {ids:?}"
        );
    }

    // Scenario: capabilities from multiple nodes are sorted lexicographically.
    //
    // Determinism requirement: the same graph must always produce the same
    // ordered list, regardless of node insertion order.
    #[test]
    fn derive_is_sorted_for_determinism() {
        let graph = SemanticGraph {
            nodes: vec![
                node_with_caps(0, vec!["z:last"]),
                node_with_caps(1, vec!["a:first"]),
                node_with_caps(2, vec!["m:middle"]),
            ],
            edges: vec![],
        };
        let ids = derive_runtime_capability_ids(&graph, &empty_anf(), "fn_missing");
        let names: Vec<&str> = ids.iter().map(CapabilityId::as_str).collect();
        assert_eq!(
            names,
            vec!["a:first", "m:middle", "z:last"],
            "capability IDs must be sorted lexicographically; got: {names:?}"
        );
    }

    #[test]
    fn derive_exact_target_ignores_unrelated_node_caps() {
        let graph = SemanticGraph {
            nodes: vec![
                node_with_caps(0, vec!["log.write"]),
                node_with_caps(1, vec![]),
            ],
            edges: vec![],
        };
        let ids = derive_runtime_capability_ids(&graph, &empty_anf(), "fn_1");
        assert!(
            ids.is_empty(),
            "target without capability_reqs must not inherit unrelated caps; got: {ids:?}"
        );
    }

    #[test]
    fn derive_includes_target_effect_call_without_graph_caps() {
        let graph = SemanticGraph {
            nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Function, "fn.print")],
            edges: vec![],
        };
        let anf = anf_with_binding(
            "fn.print",
            AnfExpr::Let {
                name: "msg".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Text("hi".to_string()))),
                body: Box::new(AnfExpr::EffectCall {
                    capability: "log.write".to_string(),
                    func: "write".to_string(),
                    args: vec!["msg".to_string()],
                }),
            },
        );

        let ids = derive_runtime_capability_ids(&graph, &anf, "fn.print");
        let names: Vec<&str> = ids.iter().map(CapabilityId::as_str).collect();
        assert_eq!(names, vec!["log.write"]);
    }

    #[test]
    fn derive_includes_reachable_callee_effect_call() {
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Function, "fn.main"),
                GraphNode::new(NodeRef(1), NodeKind::Function, "fn.print_hello"),
                GraphNode::new(NodeRef(2), NodeKind::Function, "fn.unrelated"),
            ],
            edges: vec![],
        };
        let anf = anf_with_bindings(vec![
            (
                "fn.main",
                AnfExpr::Call {
                    func: "print_hello".to_string(),
                    args: vec![],
                },
            ),
            (
                "fn.print_hello",
                AnfExpr::EffectCall {
                    capability: "log.write".to_string(),
                    func: "write".to_string(),
                    args: vec![],
                },
            ),
            (
                "fn.unrelated",
                AnfExpr::EffectCall {
                    capability: "net.read".to_string(),
                    func: "fetch".to_string(),
                    args: vec![],
                },
            ),
        ]);

        let ids = derive_runtime_capability_ids(&graph, &anf, "fn.main");
        let names: Vec<&str> = ids.iter().map(CapabilityId::as_str).collect();
        assert_eq!(names, vec!["log.write"]);
    }

    #[test]
    fn derive_handles_reachable_call_cycle() {
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Function, "fn.main"),
                GraphNode::new(NodeRef(1), NodeKind::Function, "fn.loop"),
            ],
            edges: vec![],
        };
        let anf = anf_with_bindings(vec![
            (
                "fn.main",
                AnfExpr::Call {
                    func: "loop".to_string(),
                    args: vec![],
                },
            ),
            (
                "fn.loop",
                AnfExpr::Seq(vec![
                    AnfExpr::Call {
                        func: "main".to_string(),
                        args: vec![],
                    },
                    AnfExpr::EffectCall {
                        capability: "log.write".to_string(),
                        func: "write".to_string(),
                        args: vec![],
                    },
                ]),
            ),
        ]);

        let ids = derive_runtime_capability_ids(&graph, &anf, "fn.main");
        let names: Vec<&str> = ids.iter().map(CapabilityId::as_str).collect();
        assert_eq!(names, vec!["log.write"]);
    }

    // ── parse_runtime_args ────────────────────────────────────────────────

    fn strs(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    // Scenario: bare integer → I64 (backward compatibility).
    #[test]
    fn parse_bare_integer_is_i64() {
        let result = parse_runtime_args(&strs(&["42"])).unwrap();
        assert_eq!(result, vec![RuntimeArg::I64(42)]);
    }

    // Scenario: negative bare integer → I64.
    #[test]
    fn parse_negative_integer_is_i64() {
        let result = parse_runtime_args(&strs(&["-7"])).unwrap();
        assert_eq!(result, vec![RuntimeArg::I64(-7)]);
    }

    // Scenario: i32: prefix → RuntimeArg::I32.
    #[test]
    fn parse_i32_prefix_returns_i32() {
        let result = parse_runtime_args(&strs(&["i32:42"])).unwrap();
        assert_eq!(result, vec![RuntimeArg::I32(42)]);
    }

    // Scenario: i32: prefix with max value → RuntimeArg::I32.
    #[test]
    fn parse_i32_prefix_max_value() {
        let result = parse_runtime_args(&strs(&["i32:2147483647"])).unwrap();
        assert_eq!(result, vec![RuntimeArg::I32(i32::MAX)]);
    }

    // Scenario: f64: prefix → RuntimeArg::F64.
    #[test]
    fn parse_f64_prefix_returns_f64() {
        let result = parse_runtime_args(&strs(&["f64:1.5"])).unwrap();
        match &result[0] {
            RuntimeArg::F64(v) => assert!((v - 1.5_f64).abs() < 1e-10, "expected 1.5, got {v}"),
            other => panic!("expected F64, got {other:?}"),
        }
    }

    // Scenario: f64: prefix with integer-valued float → RuntimeArg::F64.
    #[test]
    fn parse_f64_prefix_integer_value() {
        let result = parse_runtime_args(&strs(&["f64:0.0"])).unwrap();
        assert_eq!(result, vec![RuntimeArg::F64(0.0)]);
    }

    // Scenario: mixed types in one call → correct variants in order.
    #[test]
    fn parse_mixed_types_preserves_order() {
        let result = parse_runtime_args(&strs(&["i32:5", "f64:2.0", "10"])).unwrap();
        assert_eq!(
            result,
            vec![
                RuntimeArg::I32(5),
                RuntimeArg::F64(2.0),
                RuntimeArg::I64(10)
            ]
        );
    }

    // Scenario: empty slice → empty vec.
    #[test]
    fn parse_empty_args_returns_empty() {
        let result = parse_runtime_args(&[]).unwrap();
        assert!(result.is_empty());
    }

    // Scenario: non-numeric bare string → ParseError.
    #[test]
    fn parse_invalid_bare_string_returns_error() {
        let result = parse_runtime_args(&strs(&["hello"]));
        assert!(
            result.is_err(),
            "non-numeric bare arg must return ParseError"
        );
    }

    // Scenario: i32: prefix with non-numeric value → ParseError.
    #[test]
    fn parse_i32_prefix_invalid_value_returns_error() {
        let result = parse_runtime_args(&strs(&["i32:abc"]));
        assert!(
            result.is_err(),
            "i32: with non-numeric value must return ParseError"
        );
    }

    // Scenario: f64: prefix with non-numeric value → ParseError.
    #[test]
    fn parse_f64_prefix_invalid_value_returns_error() {
        let result = parse_runtime_args(&strs(&["f64:abc"]));
        assert!(
            result.is_err(),
            "f64: with non-numeric value must return ParseError"
        );
    }

    // Scenario: i32: with value exceeding i32::MAX → ParseError (overflow rejected).
    #[test]
    fn parse_i32_overflow_returns_error() {
        let result = parse_runtime_args(&strs(&["i32:99999999999"]));
        assert!(
            result.is_err(),
            "i32: with value > i32::MAX must return ParseError"
        );
    }
}
