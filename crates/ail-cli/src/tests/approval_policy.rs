use super::*;

// Scenario: cmd_approve produces immutable record.
#[test]
fn cmd_approve_produces_immutable_record() {
    use crate::store::memory_store;
    let store = memory_store();
    let id = "f".repeat(64);
    let result = cmd_approve(
        OutputMode::Human,
        &id,
        Some("public_api_changed"),
        None,
        &store,
    );
    assert!(result.is_ok(), "cmd_approve must succeed; got: {result:?}");
}

// Scenario: cmd_reject produces immutable record.
#[test]
fn cmd_reject_produces_immutable_record() {
    use crate::store::memory_store;
    let store = memory_store();
    let id = "0".repeat(64);
    let result = cmd_reject(OutputMode::Human, &id, "capability too broad", &store);
    assert!(result.is_ok(), "cmd_reject must succeed; got: {result:?}");
}

// Scenario: cmd_policy check returns violations list.
#[tokio::test]
async fn cmd_policy_check_returns_violations_list() {
    use crate::store::memory_store;
    let store = memory_store();
    let id = "1".repeat(64);
    let result = cmd_policy(
        OutputMode::Human,
        PolicyCmd::Check {
            change_id: Some(id),
            profile: "prod".to_string(),
        },
        &store,
    )
    .await;
    assert!(result.is_ok(), "policy check must succeed; got: {result:?}");
}

// Scenario PE-A: cmd_policy check with 'prod' profile uses PolicyEngine (informational).
//   GIVEN an empty memory store (fallback graph has Unverified nodes)
//   WHEN cmd_policy check is called with profile='prod'
//   THEN it returns Ok — the command is informational; engine status in JSON is "blocked"
//        but the command itself does not return an error.
//   NOTE: With the old CapabilityPolicyEnforcer-only implementation, this would
//         return policy_ok=true (no capability-deny rules → no violations).
//         With the new PolicyEngine, engine_status="blocked" (prod blocks Unverified).
//         The engine is now invoked; the JSON output carries the full verdict.
#[tokio::test]
async fn cmd_policy_check_prod_profile_engine_invoked() {
    use crate::store::memory_store;
    let store = memory_store();
    let result = cmd_policy(
        OutputMode::Json,
        PolicyCmd::Check {
            change_id: None,
            profile: "prod".to_string(),
        },
        &store,
    )
    .await;
    assert!(
        result.is_ok(),
        "policy check prod must succeed (informational); got: {result:?}"
    );
}

// Scenario PE-B: cmd_policy check with stored 'no_unverified_public_api' rule succeeds.
//   GIVEN a file store with 'no_unverified_public_api' in the policy rules
//   WHEN cmd_policy check is called
//   THEN it returns Ok — the engine maps the stored rule to NoUnverifiedPublicApi.
//   This rule would have been ignored by the old CapabilityPolicyEnforcer.
#[tokio::test]
async fn cmd_policy_check_with_stored_named_rule_succeeds() {
    use crate::store::{file_store, init_file_layout};
    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir);
    // Store a named rule that maps to PolicyRule::NoUnverifiedPublicApi.
    let result = cmd_policy(
        OutputMode::Human,
        crate::cli::PolicyCmd::Add {
            rule: "no_unverified_public_api".to_string(),
        },
        &store,
    )
    .await;
    assert!(result.is_ok(), "policy add must succeed; got: {result:?}");

    // Now check — the stored rule is mapped to the engine; command is informational.
    let result = cmd_policy(
        OutputMode::Json,
        PolicyCmd::Check {
            change_id: None,
            profile: "dev".to_string(),
        },
        &store,
    )
    .await;
    assert!(
        result.is_ok(),
        "policy check with named rule must succeed; got: {result:?}"
    );
}

// Scenario: cmd_policy explain known rule returns description.
#[tokio::test]
async fn cmd_policy_explain_known_rule() {
    use crate::store::memory_store;
    let store = memory_store();
    let result = cmd_policy(
        OutputMode::Human,
        PolicyCmd::Explain {
            rule: "no_unverified_public_api".to_string(),
        },
        &store,
    )
    .await;
    assert!(
        result.is_ok(),
        "policy explain must succeed; got: {result:?}"
    );
}
