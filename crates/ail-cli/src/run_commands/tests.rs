use ail_compiler::LiteralValue;
use ail_compiler::{ANF_SCHEMA_VERSION, AnfBinding, AnfExpr, AnfIr, SourceMap, StageHashes};
use ail_core::semantic_graph::{CapabilityReqs, GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_runtime::{CapabilityId, PreflightFailure, RuntimeArg, RuntimeError};

use super::{derive_runtime_capability_ids, format_run_preflight_error, parse_runtime_args};

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

#[test]
fn capability_diagnostic_names_target_capability_and_grant() {
    let err = RuntimeError::PreflightFailed(PreflightFailure::CapabilityDenied {
        denied: vec![CapabilityId::new("log.write")],
    });

    let msg = format_run_preflight_error(&err, "fn.print_hello");

    assert!(
        msg.contains("function `fn.print_hello`"),
        "diagnostic must name target function; got: {msg}"
    );
    assert!(
        msg.starts_with("AIL_RUN_CAPABILITY_GRANT_DENIED: run.capability_grant_denied:"),
        "diagnostic must start with stable code and key; got: {msg}"
    );
    assert!(
        msg.contains("capability `log.write`"),
        "diagnostic must name missing capability; got: {msg}"
    );
    assert!(
        msg.contains("denied_count=1"),
        "diagnostic must expose stable denied count; got: {msg}"
    );
    assert!(
        msg.contains("suggestion: add `--grant log.write`"),
        "diagnostic must suggest safe grant flag; got: {msg}"
    );
}

#[test]
fn capability_diagnostic_omits_unsafe_grant_suggestion() {
    let err = RuntimeError::PreflightFailed(PreflightFailure::CapabilityDenied {
        denied: vec![CapabilityId::new("log write")],
    });

    let msg = format_run_preflight_error(&err, "fn.print_hello");

    assert!(
        msg.contains("capability `<redacted:unsafe-capability-id>`"),
        "diagnostic must redact unsafe capability text; got: {msg}"
    );
    assert!(
        !msg.contains("log write"),
        "diagnostic must not leak unsafe capability text; got: {msg}"
    );
    assert!(
        !msg.contains("suggestion:"),
        "diagnostic must not suggest unsafe shell words; got: {msg}"
    );
}

#[test]
fn capability_diagnostic_redacts_secret_capability_ids() {
    let err = RuntimeError::PreflightFailed(PreflightFailure::CapabilityDenied {
        denied: vec![CapabilityId::new("secret.read:ProductionDbPassword")],
    });

    let msg = format_run_preflight_error(&err, "fn.read_secret");

    assert!(
        msg.contains("AIL_RUN_CAPABILITY_GRANT_DENIED"),
        "diagnostic must include stable code; got: {msg}"
    );
    assert!(
        msg.contains("capability `<redacted:secret-capability>`"),
        "diagnostic must expose only redacted secret capability shape; got: {msg}"
    );
    assert!(
        msg.contains("redacted_capabilities=1"),
        "diagnostic must expose redaction count; got: {msg}"
    );
    assert!(
        !msg.contains("ProductionDbPassword"),
        "diagnostic must not leak secret capability suffix; got: {msg}"
    );
    assert!(
        !msg.contains("suggestion:"),
        "diagnostic must not suggest redacted secret grants; got: {msg}"
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
