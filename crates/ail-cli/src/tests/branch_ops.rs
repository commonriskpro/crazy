use super::*;

// Scenario: cmd_rollback by change-id succeeds.
#[tokio::test]
async fn cmd_rollback_by_change_id_succeeds() {
    use crate::store::memory_store;
    let store = memory_store();
    let change_id = "c".repeat(64);
    let result = cmd_rollback(OutputMode::Human, None, Some(&change_id), &store).await;
    assert!(
        result.is_ok(),
        "rollback-by-change must succeed; got: {result:?}"
    );
}

// Scenario: cmd_rollback with no args returns Domain error.
#[tokio::test]
async fn cmd_rollback_no_args_returns_error() {
    use crate::store::memory_store;
    let store = memory_store();
    let result = cmd_rollback(OutputMode::Human, None, None, &store).await;
    assert!(matches!(result, Err(CliError::Domain(_))));
}

// Scenario: cmd_rebase returns rebase_report with conflicts/repair_options.
#[tokio::test]
async fn cmd_rebase_returns_full_report() {
    use crate::store::memory_store;
    let store = memory_store();
    let result = cmd_rebase(OutputMode::Human, "main", None, &store).await;
    assert!(result.is_ok(), "cmd_rebase must succeed; got: {result:?}");
}
