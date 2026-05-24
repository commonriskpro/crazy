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
