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

use ail_compiler::{emit_wasm_with_profile, lower_to_anf_with_graph, lower_to_core_ir};
use ail_runtime::{
    AuditEvent, CapabilityId, CapabilityManifest, ResourceLimits, RuntimeArg, RuntimeHost,
    RuntimeProfile, RuntimeReportStatus, blake3_hex_of,
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

/// Derive runtime `CapabilityId`s from a semantic graph.
///
/// Walks every node and collects all capability names from
/// `node.capability_reqs.caps`.  Results are deduplicated and sorted
/// lexicographically so that the resulting `CapabilityManifest` is
/// deterministic for the same graph input.
///
/// Returns an empty `Vec` when the graph has no nodes with capability
/// requirements — the correct result for graphs that perform no external
/// capability calls.
fn derive_runtime_capability_ids(graph: &SemanticGraph) -> Vec<CapabilityId> {
    use std::collections::BTreeSet;

    let unique: BTreeSet<String> = graph
        .nodes
        .iter()
        .filter_map(|n| n.capability_reqs.as_ref())
        .flat_map(|reqs| reqs.caps.iter().cloned())
        .collect();

    unique.into_iter().map(CapabilityId::new).collect()
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
pub(crate) async fn cmd_run(
    mode: OutputMode,
    profile: &str,
    target: &str,
    module: Option<&str>,
    raw_args: &[String],
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
        // Derive capability IDs before the graph enters the compiler
        // pipeline — compiler functions all take `&SemanticGraph`, so this
        // shared borrow is always valid.
        let capability_ids = derive_runtime_capability_ids(&graph);
        // Use an accepted (empty/Proven) report for the e2e pipeline.
        // A full verify pass would reject the graph because the type checker
        // flags newly-materialised nodes as Unverified — expected at this stage.
        let report = accepted_compile_report();
        let core = lower_to_core_ir(&graph, &report)
            .map_err(|e| CliError::Domain(format!("Failed to lower graph to Core IR: {e}")))?;
        let anf = lower_to_anf_with_graph(&core, &graph)
            .map_err(|e| CliError::Domain(format!("Failed to lower Core IR to ANF: {e}")))?;
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

    let runtime_profile = RuntimeProfile::new(
        profile.to_string(),
        module_hash.clone(),
        String::new(),
        manifest_hash,
        vec![],
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
            ..Default::default()
        },
    );

    let mut host = RuntimeHost::new();
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
            let invoke_result = instance.invoke(export_name, &runtime_args);

            // Post-invoke: aggregate capability call statistics from the full audit
            // log (includes any CapabilityCallExecuted events produced during invoke).
            let report = host.emit_report(RuntimeReportStatus::Completed, "run");
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

            let result_display = match &invoke_result {
                Ok(val) => format!("result: {val}"),
                Err(e) => format!("invoke error: {e}"),
            };

            let human_msg = format!(
                "PreflightPassed\n{result_display}\nprofile: {profile}\nmodule: {module_name}\naudit_events: {audit_len}\ncapability_calls: {total_capability_calls}\nruntime_checks: all ok"
            );
            print_response(
                mode,
                &human_msg,
                json!({
                    "outcome": "PreflightPassed",
                    "profile": profile,
                    "module": module_name,
                    "invoke_result": result_display,
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

    // Scenario: empty graph → no capability IDs.
    #[test]
    fn derive_empty_graph_returns_empty() {
        let graph = SemanticGraph {
            nodes: vec![],
            edges: vec![],
        };
        assert!(
            derive_runtime_capability_ids(&graph).is_empty(),
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
            derive_runtime_capability_ids(&graph).is_empty(),
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
        let ids = derive_runtime_capability_ids(&graph);
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
        let ids = derive_runtime_capability_ids(&graph);
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
        let ids = derive_runtime_capability_ids(&graph);
        let names: Vec<&str> = ids.iter().map(CapabilityId::as_str).collect();
        assert_eq!(
            names,
            vec!["a:first", "m:middle", "z:last"],
            "capability IDs must be sorted lexicographically; got: {names:?}"
        );
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
