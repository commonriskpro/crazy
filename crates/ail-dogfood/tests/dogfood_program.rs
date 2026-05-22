use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use ail_change::apply::{SnapshotBridge, apply};
use ail_change::canonical::canonicalize_parsed;
use ail_change::model::{ChangeSetOutcome, SnapshotId};
use ail_change::parser::parse_changeset;
use ail_compiler::core_ir::{LiteralValue, StageHashes};
use ail_compiler::{AnfBinding, AnfExpr, AnfIr, SourceMap, emit_wasm};
use ail_core::semantic_graph::{NodeRef, SemanticGraph};
use ail_runtime::{
    AuditEvent, CapabilityGrant, CapabilityId, CapabilityManifest, Handler, HostResult,
    ResourceLimits, RuntimeHost, RuntimeProfile, RuntimeValue, blake3_hex_of,
};

struct FixedSnapshot;

impl SnapshotBridge for FixedSnapshot {
    fn current_snapshot_id(&self) -> SnapshotId {
        SnapshotId(0)
    }
}

struct CountingLogger {
    calls: AtomicUsize,
    caps: Vec<CapabilityId>,
}

impl CountingLogger {
    fn new(capability: CapabilityId) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            caps: vec![capability],
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Handler for CountingLogger {
    fn name(&self) -> &str {
        "counting-logger"
    }

    fn capabilities(&self) -> &[CapabilityId] {
        &self.caps
    }

    fn handle(
        &self,
        _capability: &CapabilityId,
        _operation: &str,
        payload: &[u8],
    ) -> HostResult<Vec<u8>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut arg = [0u8; 8];
        arg.copy_from_slice(&payload[..8]);
        Ok(i64::from_le_bytes(arg).to_le_bytes().to_vec())
    }
}

fn binding(source_ref: u32, name: &str, expr: AnfExpr) -> AnfBinding {
    AnfBinding {
        source_ref: NodeRef(source_ref),
        name: name.to_string(),
        expr,
    }
}

fn int(value: i64) -> AnfExpr {
    AnfExpr::Literal(LiteralValue::Int(value))
}

fn var(name: &str) -> AnfExpr {
    AnfExpr::Var(name.to_string())
}

fn call(func: &str, args: &[&str]) -> AnfExpr {
    AnfExpr::Call {
        func: func.to_string(),
        args: args.iter().map(|arg| (*arg).to_string()).collect(),
    }
}

fn let_in(name: &str, value: AnfExpr, body: AnfExpr) -> AnfExpr {
    AnfExpr::Let {
        name: name.to_string(),
        value: Box::new(value),
        body: Box::new(body),
    }
}

fn anf_ir(bindings: Vec<AnfBinding>) -> AnfIr {
    AnfIr {
        schema_version: ail_compiler::anf::ANF_SCHEMA_VERSION,
        source_map: SourceMap::from_bindings(&bindings),
        bindings,
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

fn run_zero_arg(anf: &AnfIr, export_name: &str) -> RuntimeValue {
    let artifact = emit_wasm(anf).expect("wasm emission must succeed");
    let manifest = CapabilityManifest {
        module: "dogfood".to_string(),
        requires: vec![],
    };
    let profile = RuntimeProfile::new(
        "test".to_string(),
        blake3_hex_of(&artifact.wasm),
        String::new(),
        manifest.blake3_hex().expect("manifest hash must compute"),
        vec![],
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
        },
    );
    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&artifact.wasm, &manifest, &profile)
        .expect("runtime instantiation must succeed");

    instance
        .invoke(export_name, &[])
        .expect("invoke must succeed")
}

#[test]
fn calculator_sum_of_squares_returns_25() {
    let add = binding(0, "fn.add", call("+", &["a", "b"]));
    let multiply = binding(1, "fn.multiply", call("*", &["a", "b"]));
    let square = binding(2, "fn.square", call("fn.multiply", &["x", "x"]));
    let sum_of_squares = binding(
        3,
        "fn.sum_of_squares",
        let_in(
            "left",
            call("fn.square", &["a"]),
            let_in(
                "right",
                call("fn.square", &["b"]),
                call("fn.add", &["left", "right"]),
            ),
        ),
    );
    let main = binding(
        4,
        "fn.main",
        let_in(
            "three",
            int(3),
            let_in(
                "four",
                int(4),
                call("fn.sum_of_squares", &["three", "four"]),
            ),
        ),
    );

    let value = run_zero_arg(
        &anf_ir(vec![add, multiply, square, sum_of_squares, main]),
        "main",
    );

    assert_eq!(value, RuntimeValue::I64(25));
}

#[test]
fn conditionals_abs_max_and_clamp_return_expected_values() {
    let abs = binding(
        0,
        "fn.abs",
        let_in(
            "zero",
            int(0),
            let_in(
                "is_negative",
                call("<", &["x", "zero"]),
                AnfExpr::If {
                    cond: "is_negative".to_string(),
                    then_branch: Box::new(call("-", &["zero", "x"])),
                    else_branch: Box::new(var("x")),
                },
            ),
        ),
    );
    let max = binding(
        1,
        "fn.max",
        let_in(
            "a_is_greater",
            call(">", &["a", "b"]),
            AnfExpr::If {
                cond: "a_is_greater".to_string(),
                then_branch: Box::new(var("a")),
                else_branch: Box::new(var("b")),
            },
        ),
    );
    let min = binding(
        2,
        "fn.min",
        let_in(
            "a_is_less",
            call("<", &["a", "b"]),
            AnfExpr::If {
                cond: "a_is_less".to_string(),
                then_branch: Box::new(var("a")),
                else_branch: Box::new(var("b")),
            },
        ),
    );
    let clamp = binding(
        3,
        "fn.clamp",
        let_in(
            "upper",
            call("fn.min", &["x", "hi"]),
            call("fn.max", &["lo", "upper"]),
        ),
    );
    let main = binding(
        4,
        "fn.main",
        let_in(
            "neg_five",
            int(-5),
            let_in(
                "abs_value",
                call("fn.abs", &["neg_five"]),
                let_in(
                    "twelve",
                    int(12),
                    let_in(
                        "lo",
                        int(0),
                        let_in(
                            "hi",
                            int(10),
                            let_in(
                                "clamped",
                                call("fn.clamp", &["twelve", "lo", "hi"]),
                                call("+", &["abs_value", "clamped"]),
                            ),
                        ),
                    ),
                ),
            ),
        ),
    );

    let value = run_zero_arg(&anf_ir(vec![abs, max, min, clamp, main]), "main");

    assert_eq!(value, RuntimeValue::I64(15));
}

#[test]
fn effect_dispatches_to_logger_handler_and_records_audit_log() {
    let source = "\
change logger_effect base=0
author tester
description logger effect dogfood
op create_capability id=cap.logger
end
";
    let parsed = parse_changeset(source).expect("capability ACL must parse");
    assert_eq!(parsed.parsed_ops[0].verb, "create_capability");
    let canonical = canonicalize_parsed(parsed);
    let mut graph = SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };
    assert_eq!(
        apply(canonical, &mut graph, &FixedSnapshot),
        ChangeSetOutcome::Applied
    );
    assert!(graph.nodes.iter().any(|node| node.name == "cap.logger"));

    let cap = CapabilityId::new("cap.logger");
    let main = binding(
        0,
        "fn.main",
        let_in(
            "message_code",
            int(7),
            AnfExpr::EffectCall {
                capability: cap.as_str().to_string(),
                func: "log".to_string(),
                args: vec!["message_code".to_string()],
            },
        ),
    );
    let artifact = emit_wasm(&anf_ir(vec![main])).expect("effect wasm emits");
    let manifest = CapabilityManifest {
        module: "dogfood-effects".to_string(),
        requires: vec![cap.clone()],
    };
    let profile = RuntimeProfile::new(
        "test".to_string(),
        blake3_hex_of(&artifact.wasm),
        String::new(),
        manifest.blake3_hex().expect("manifest hash must compute"),
        vec![CapabilityGrant {
            module: manifest.module.clone(),
            capability: cap.clone(),
        }],
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
        },
    )
    .with_handler_binding_required();
    let handler = Arc::new(CountingLogger::new(cap));
    let mut host = RuntimeHost::new().with_handler(handler.clone());
    let mut instance = host
        .validate_and_instantiate(&artifact.wasm, &manifest, &profile)
        .expect("runtime instantiation must succeed");

    let value = instance
        .invoke("main", &[])
        .expect("effect invoke succeeds");

    assert_eq!(value, RuntimeValue::I64(7));
    assert_eq!(handler.calls(), 1);
    assert!(instance.audit_log().events().iter().any(|event| matches!(
        event,
        AuditEvent::CapabilityCallExecuted {
            capability,
            operation,
            handler_name,
            succeeded: true,
            ..
        } if capability.as_str() == "cap.logger"
            && operation == "log"
            && handler_name == "counting-logger"
    )));
}

#[test]
#[ignore = "ACL set_body stores body=@expr as metadata, not CoreExpr; ail run accepts module target only and invokes fn.answer-style exports by last path segment, so a real `ail init/change/compile/run fn.main` program cannot yet round-trip user-defined bodies end-to-end."]
fn full_cli_pipeline_runs_fn_main_from_applied_changeset() {
    // Dogfooding gap: this should eventually create a temp project, run
    // `ail init`, apply an ACL changeset defining fn.main, compile, and run
    // `ail run fn.main`, asserting the computed result. Today the CLI can
    // compile/run literal `value=<int>` graph nodes, but ACL expression bodies
    // are not lowered into executable CoreExpr/ANF.
}

#[test]
#[ignore = "ACL expression blocks are parsed and hashed but not lowered into executable CoreExpr, so text definitions like `fn.add(a, b) = a + b` cannot currently be compiled through parse_changeset -> apply -> lower_to_core_ir."]
fn acl_text_function_bodies_compile_to_executable_wasm() {
    // Dogfooding gap: calculator/conditional bodies should move from block
    // metadata into CoreExpr instead of requiring hand-authored ANF in tests.
}
