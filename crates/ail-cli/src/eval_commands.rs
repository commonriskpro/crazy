// ── ail-cli::eval_commands ────────────────────────────────────────────────
//
// `ail eval <expression>` and expression-specific ANF helpers.
//
// Responsibilities:
//   - cmd_eval: compile and run a single expression via the WASM runtime
//   - parse_eval_expression: text → AnfExpr for simple arithmetic/calls
//   - eval_anf: wrap an AnfExpr into a runnable AnfIr
//   - runtime_value_to_string: format a RuntimeValue for display

use ail_compiler::{
    ANF_SCHEMA_VERSION, AnfBinding, AnfExpr, AnfIr, LiteralValue, SourceMap, StageHashes,
    emit_wasm_with_profile,
};
use ail_core::semantic_graph::NodeRef;
use ail_runtime::{
    CapabilityManifest, ResourceLimits, RuntimeHost, RuntimeProfile, RuntimeValue, blake3_hex_of,
};
use serde_json::json;

use crate::error::CliError;
use crate::output::{OutputMode, print_response};

// ── cmd_eval ──────────────────────────────────────────────────────────────

pub(crate) fn cmd_eval(mode: OutputMode, expression: &str) -> Result<(), CliError> {
    let expr = parse_eval_expression(expression)?;
    let anf = eval_anf(expr);
    let artifact = emit_wasm_with_profile(&anf, "dev")
        .map_err(|e| CliError::Domain(format!("Failed to compile expression: {e}")))?;
    let manifest = CapabilityManifest {
        module: "eval".to_string(),
        requires: vec![],
    };
    let module_hash = blake3_hex_of(&artifact.wasm);
    let manifest_hash = manifest
        .blake3_hex()
        .map_err(|e| CliError::Domain(format!("Failed to hash eval manifest: {e}")))?;
    let runtime_profile = RuntimeProfile::new(
        "dev".to_string(),
        module_hash,
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
    let mut instance = host
        .validate_and_instantiate(&artifact.wasm, &manifest, &runtime_profile)
        .map_err(|e| CliError::PreflightFailed(format!("Failed to start eval runtime: {e}")))?;
    let value = instance
        .invoke("eval", &[])
        .map_err(|e| CliError::Domain(format!("Failed to run expression: {e}")))?;
    let result = runtime_value_to_string(&value);

    print_response(
        mode,
        &format!("expression: {expression}\nresult: {result}"),
        json!({
            "expression": expression,
            "result": result,
        }),
    );
    Ok(())
}

// ── ANF builder helpers ───────────────────────────────────────────────────

fn eval_anf(expr: AnfExpr) -> AnfIr {
    let bindings = vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.eval".to_string(),
        expr,
    }];
    let source_map = SourceMap::from_bindings(&bindings);
    AnfIr {
        schema_version: ANF_SCHEMA_VERSION,
        bindings,
        source_map,
        stage_hashes: StageHashes {
            graph_snapshot_hash: [0; 32],
            verification_report_hash: [0; 32],
            core_ir_hash: [0; 32],
            anf_ir_hash: Some([0; 32]),
            wasm_hash: None,
            native_hash: None,
            source_map_hash: None,
            artifact_manifest_hash: None,
        },
    }
}

// ── Expression parser ─────────────────────────────────────────────────────

fn parse_eval_expression(expression: &str) -> Result<AnfExpr, CliError> {
    let trimmed = expression.trim();
    if let Ok(value) = trimmed.parse::<i64>() {
        return Ok(AnfExpr::Literal(LiteralValue::Int(value)));
    }

    let Some(open) = trimmed.find('(') else {
        return Err(CliError::ParseError(
            "Failed to parse expression: expected a number or call like add(20, 22)".to_string(),
        ));
    };
    let Some(close) = trimmed.rfind(')') else {
        return Err(CliError::ParseError(
            "Failed to parse expression: missing closing ')'".to_string(),
        ));
    };
    if close != trimmed.len() - 1 {
        return Err(CliError::ParseError(
            "Failed to parse expression: unexpected text after ')'".to_string(),
        ));
    }

    let op = trimmed[..open].trim();
    let args: Vec<&str> = trimmed[open + 1..close]
        .split(',')
        .map(str::trim)
        .filter(|arg| !arg.is_empty())
        .collect();
    if op == "double" {
        if args.len() != 1 {
            return Err(CliError::ParseError(format!(
                "Failed to parse expression: {op} expects exactly 1 argument"
            )));
        }
        let value = parse_eval_i64(args[0])?;
        return Ok(AnfExpr::Let {
            name: "x".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(value))),
            body: Box::new(AnfExpr::Call {
                func: "i64.add".to_string(),
                args: vec!["x".to_string(), "x".to_string()],
            }),
        });
    }

    if args.len() != 2 {
        return Err(CliError::ParseError(format!(
            "Failed to parse expression: {op} expects exactly 2 arguments"
        )));
    }
    let left = parse_eval_i64(args[0])?;
    let right = parse_eval_i64(args[1])?;
    let func = match op {
        "add" => "i64.add",
        "sub" => "i64.sub",
        "mul" => "i64.mul",
        "div" => "i64.div_s",
        "mod" => "i64.rem_s",
        _ => {
            return Err(CliError::ParseError(format!(
                "Failed to parse expression: unsupported function '{op}'"
            )));
        }
    };

    Ok(AnfExpr::Let {
        name: "a".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(left))),
        body: Box::new(AnfExpr::Let {
            name: "b".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(right))),
            body: Box::new(AnfExpr::Call {
                func: func.to_string(),
                args: vec!["a".to_string(), "b".to_string()],
            }),
        }),
    })
}

fn parse_eval_i64(value: &str) -> Result<i64, CliError> {
    value.parse::<i64>().map_err(|_| {
        CliError::ParseError(format!(
            "Failed to parse expression: '{value}' is not an integer"
        ))
    })
}

// ── Runtime value display ─────────────────────────────────────────────────

fn runtime_value_to_string(value: &RuntimeValue) -> String {
    match value {
        RuntimeValue::I64(value) => value.to_string(),
        RuntimeValue::I32(value) => value.to_string(),
        RuntimeValue::F64(value) => value.to_string(),
        RuntimeValue::Unit => "()".to_string(),
    }
}
