// ── ail-cli::test_commands ────────────────────────────────────────────────
//
// User-facing `ail test` runner.
//
// This is intentionally small, but it is a real vertical slice:
// persisted graph → verification gate → Core IR → ANF → WASM → runtime invoke.

use ail_compiler::{
    emit_wasm_with_profile, export_name, lower_to_anf_with_graph, lower_to_core_ir,
};
use ail_core::semantic_graph::{GraphNode, NodeKind};
use ail_runtime::{
    CapabilityGrant, CapabilityManifest, ResourceLimits, RuntimeHost, RuntimeProfile, blake3_hex_of,
};
use serde_json::{Value, json};

use crate::cli::load_current_graph_for_cli;
use crate::compile_commands::{accepted_compile_report, verify_graph_for_compile};
use crate::error::CliError;
use crate::output::{OutputMode, print_response};
use crate::run_commands::{
    derive_runtime_capability_ids, invoke_export_for_cli, parse_runtime_args,
};
use crate::source_commands::load_source_graph;
use crate::store::StoreHandle;
use std::path::Path;

// ── Command handler ───────────────────────────────────────────────────────

pub(crate) async fn cmd_test(
    mode: OutputMode,
    profile: &str,
    target: &str,
    source_file: Option<&Path>,
    filter: Option<&str>,
    raw_args: &[String],
    grants: &[String],
    store: &StoreHandle,
) -> Result<(), CliError> {
    if target == "native" {
        return Err(CliError::Domain(
            "native test execution not supported yet".to_string(),
        ));
    }

    let mut graph = if let Some(path) = source_file {
        load_source_graph(path)?
    } else {
        load_current_graph_for_cli(store).await?
    };
    verify_graph_for_compile(&graph)?;
    let tests = discover_tests(&graph, filter);

    // The Core IR lowerer only materializes bodies for `Function` nodes, so a `Test`
    // node would otherwise compile to an empty (zero-returning) export. Retag the
    // discovered test nodes as functions before lowering so their assertion bodies
    // are emitted and invoked. Discovery already captured the test set above.
    for node in &mut graph.nodes {
        if node.kind == NodeKind::Test {
            node.kind = NodeKind::Function;
        }
    }

    let report = accepted_compile_report();
    let core = lower_to_core_ir(&graph, &report)
        .map_err(|e| CliError::Domain(format!("Failed to lower graph to Core IR: {e}")))?;
    let anf = lower_to_anf_with_graph(&core, &graph)
        .map_err(|e| CliError::Domain(format!("Failed to lower Core IR to ANF: {e}")))?;
    let artifact = emit_wasm_with_profile(&anf, profile)
        .map_err(|e| CliError::Domain(format!("Failed to emit test WASM artifact: {e}")))?;

    let runtime_capabilities = tests
        .iter()
        .flat_map(|test| derive_runtime_capability_ids(&graph, &anf, &test.name))
        .collect();
    let manifest = CapabilityManifest {
        module: "tests".to_string(),
        requires: runtime_capabilities,
    };
    let module_hash = blake3_hex_of(&artifact.wasm);
    let manifest_hash = manifest
        .blake3_hex()
        .map_err(|e| CliError::Domain(format!("test (manifest hash): {e}")))?;
    let runtime_grants = grants
        .iter()
        .map(|grant| CapabilityGrant {
            module: "tests".to_string(),
            capability: ail_runtime::CapabilityId::new(grant.clone()),
        })
        .collect();
    let runtime_profile = RuntimeProfile::new(
        profile.to_string(),
        module_hash,
        String::new(),
        manifest_hash,
        runtime_grants,
        ResourceLimits::default(),
    );
    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&artifact.wasm, &manifest, &runtime_profile)
        .map_err(|e| CliError::PreflightFailed(format!("Failed to start test runtime: {e}")))?;
    let runtime_args = parse_runtime_args(raw_args)?;

    let mut results = Vec::with_capacity(tests.len());
    for test in &tests {
        let export_name = export_name(&test.name);
        let export_type = artifact.export_types.get(export_name.as_str());
        let result = match invoke_export_for_cli(
            &mut instance,
            export_name.as_str(),
            &runtime_args,
            export_type,
        ) {
            Ok((label, value)) => {
                let passed = test_value_passed(&value);
                TestResult {
                    name: test.name.clone(),
                    export_name: export_name.clone(),
                    passed,
                    detail: label,
                }
            }
            Err(err) => TestResult {
                name: test.name.clone(),
                export_name: export_name.clone(),
                passed: false,
                detail: err,
            },
        };
        results.push(result);
    }

    let passed = results.iter().filter(|result| result.passed).count();
    let failed = results.len().saturating_sub(passed);
    let human_lines = results
        .iter()
        .map(|result| {
            let status = if result.passed { "PASS" } else { "FAIL" };
            format!("{status} {} ({})", result.name, result.detail)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let human_msg = if human_lines.is_empty() {
        "test result: ok. 0 passed; 0 failed".to_string()
    } else {
        format!("{human_lines}\ntest result: {passed} passed; {failed} failed")
    };
    print_response(
        mode,
        &human_msg,
        json!({
            "profile": profile,
            "target": target,
            "source_file": source_file.map(|path| path.display().to_string()),
            "filter": filter,
            "passed": passed,
            "failed": failed,
            "total": results.len(),
            "tests": results.iter().map(TestResult::to_json).collect::<Vec<_>>(),
        }),
    );

    if failed > 0 {
        return Err(CliError::Domain(format!("{failed} test(s) failed")));
    }
    Ok(())
}

// ── Discovery ─────────────────────────────────────────────────────────────

struct TestTarget {
    name: String,
}

fn discover_tests(
    graph: &ail_core::semantic_graph::SemanticGraph,
    filter: Option<&str>,
) -> Vec<TestTarget> {
    graph
        .nodes
        .iter()
        .filter(|node| is_test_node(node))
        .filter(|node| match filter {
            Some(needle) => node.name.contains(needle),
            None => true,
        })
        .map(|node| TestTarget {
            name: node.name.clone(),
        })
        .collect()
}

fn is_test_node(node: &GraphNode) -> bool {
    node.kind == NodeKind::Test
        || node.name.starts_with("test.")
        || node.name.starts_with("fn.test_")
        || node.name.contains(".test.")
}

// ── Result helpers ────────────────────────────────────────────────────────

struct TestResult {
    name: String,
    export_name: String,
    passed: bool,
    detail: String,
}

impl TestResult {
    fn to_json(&self) -> Value {
        json!({
            "name": self.name.clone(),
            "export": self.export_name.clone(),
            "status": if self.passed { "passed" } else { "failed" },
            "detail": self.detail.clone(),
        })
    }
}

fn test_value_passed(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_f64().is_some_and(|value| value != 0.0),
        Value::String(value) => {
            let value = value.trim().to_ascii_lowercase();
            matches!(value.as_str(), "" | "ok" | "pass" | "passed" | "true")
        }
        Value::Array(values) => !values.is_empty(),
        Value::Object(_) => true,
    }
}
