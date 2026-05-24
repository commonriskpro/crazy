use super::*;

// Scenario: cmd_run succeeds when preflight passes (exit 0).
#[tokio::test]
async fn cmd_run_succeeds() {
    use crate::store::memory_store;
    let store = memory_store();
    let result = cmd_run(OutputMode::Human, "dev", "wasm", None, &[], None, &store).await;
    assert!(result.is_ok(), "cmd_run must succeed; got: {result:?}");
}

// Scenario: cmd_run with module succeeds.
#[tokio::test]
async fn cmd_run_with_module_succeeds() {
    use crate::store::memory_store;
    let store = memory_store();
    let result = cmd_run(
        OutputMode::Human,
        "dev",
        "wasm",
        Some("module.checkout"),
        &[],
        None,
        &store,
    )
    .await;
    assert!(
        result.is_ok(),
        "cmd_run with module must succeed; got: {result:?}"
    );
}

// Scenario: cmd_run with replay succeeds.
#[tokio::test]
async fn cmd_run_with_replay_succeeds() {
    use crate::store::memory_store;
    let store = memory_store();
    let result = cmd_run(
        OutputMode::Human,
        "test",
        "wasm",
        None,
        &[],
        Some("trace_123"),
        &store,
    )
    .await;
    assert!(
        result.is_ok(),
        "cmd_run with replay must succeed; got: {result:?}"
    );
}

// Scenario: cmd_run with native target returns explicit Domain error.
//   GIVEN target == "native"
//   WHEN cmd_run is called
//   THEN Err(CliError::Domain(...)) mentioning "native" is returned
#[tokio::test]
async fn cmd_run_native_target_returns_domain_error() {
    use crate::store::memory_store;
    let store = memory_store();
    let result = cmd_run(OutputMode::Human, "dev", "native", None, &[], None, &store).await;
    match &result {
        Err(CliError::Domain(msg)) => assert!(
            msg.contains("native"),
            "error must mention 'native'; got: {msg}"
        ),
        other => panic!("expected Domain error for native target; got: {other:?}"),
    }
}

// Scenario: cmd_run with i32: typed arg succeeds.
//   Verifies parse_runtime_args i32: prefix is accepted end-to-end.
#[tokio::test]
async fn cmd_run_with_i32_arg_succeeds() {
    use crate::store::memory_store;
    let store = memory_store();
    let args = vec!["i32:0".to_string()];
    let result = cmd_run(OutputMode::Human, "dev", "wasm", None, &args, None, &store).await;
    assert!(
        result.is_ok(),
        "cmd_run with i32: arg must succeed; got: {result:?}"
    );
}

// Scenario: cmd_run with f64: typed arg succeeds.
//   Verifies parse_runtime_args f64: prefix is accepted end-to-end.
#[tokio::test]
async fn cmd_run_with_f64_arg_succeeds() {
    use crate::store::memory_store;
    let store = memory_store();
    let args = vec!["f64:0.0".to_string()];
    let result = cmd_run(OutputMode::Human, "dev", "wasm", None, &args, None, &store).await;
    assert!(
        result.is_ok(),
        "cmd_run with f64: arg must succeed; got: {result:?}"
    );
}

// Scenario: cmd_run with an invalid (non-numeric, unrecognized) arg returns ParseError.
//   GIVEN a bare non-numeric string arg with no typed prefix
//   WHEN cmd_run is called
//   THEN Err(CliError::ParseError(...)) is returned before reaching the runtime
#[tokio::test]
async fn cmd_run_with_invalid_arg_returns_parse_error() {
    use crate::store::memory_store;
    let store = memory_store();
    let args = vec!["not_a_number".to_string()];
    let result = cmd_run(OutputMode::Human, "dev", "wasm", None, &args, None, &store).await;
    match &result {
        Err(CliError::ParseError(msg)) => assert!(
            msg.contains("not_a_number"),
            "ParseError must mention the bad arg; got: {msg}"
        ),
        other => panic!("expected ParseError for invalid arg; got: {other:?}"),
    }
}
