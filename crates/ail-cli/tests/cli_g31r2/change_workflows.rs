use super::common::{ail, parse_json_output};
use predicates::prelude::*;

// ── G31 R2: diff semantic ─────────────────────────────────────────────────

/// SC-DIF1: diff --semantic returns full structural diff with all categories.
#[test]
fn diff_semantic_returns_full_structural_diff() {
    let output = ail()
        .args(["diff", "change.add_checkout", "--semantic", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    let diff = &v["data"]["structural_diff"];
    assert!(diff.is_object(), "structural_diff must be object; got: {v}");
    // Verify all semantic diff categories are present.
    for field in &[
        "creates",
        "modifies",
        "deletes",
        "tombstones",
        "connects",
        "disconnects",
        "exposes",
        "hides",
        "effects_changed",
        "contracts_changed",
        "capabilities_changed",
    ] {
        assert!(
            diff[field].is_array(),
            "structural_diff.{field} must be array; got: {diff}"
        );
    }
}

// ── G31 R2: rollback by change ────────────────────────────────────────────

/// SC-RBK1: rollback with change-id (rollback-by-change) exits 0.
#[test]
fn rollback_by_change_id_exits_zero() {
    let change_id = "ef".repeat(32);
    ail()
        .args(["rollback", &change_id])
        .assert()
        .success()
        .stdout(predicate::str::contains("new snapshot").or(predicate::str::contains("rollback")));
}

/// SC-RBK2: rollback-by-change --json has rollback_type=by_change and reversed_change_id.
#[test]
fn rollback_by_change_json_has_rollback_type() {
    let change_id = "ef".repeat(32);
    let output = ail()
        .args(["rollback", &change_id, "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(
        v["data"]["rollback_type"], "by_change",
        "rollback_type must be by_change; got: {v}"
    );
    assert!(
        v["data"]["reversed_change_id"].is_string(),
        "reversed_change_id must be string; got: {v}"
    );
    assert_eq!(
        v["data"]["history_preserved"], true,
        "history must be preserved; got: {v}"
    );
}

// ── G31 R2: rebase full report ────────────────────────────────────────────

/// SC-REB1: rebase --json has rebase_report with full shape.
#[test]
fn rebase_json_has_rebase_report() {
    let change_id = "ab".repeat(32);
    let onto = "cd".repeat(32);
    let output = ail()
        .args(["rebase", &change_id, "--onto", &onto, "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["rebase_report"].is_object(),
        "rebase_report must be object; got: {v}"
    );
    assert!(
        v["data"]["conflicts"].is_array(),
        "conflicts must be array; got: {v}"
    );
    assert!(
        v["data"]["repair_options"].is_array(),
        "repair_options must be array; got: {v}"
    );
}

// ── G31 R2: merge full conflict workflow ─────────────────────────────────

/// SC-MRG1: merge --json includes rebase_report with conflict info.
#[test]
fn merge_json_has_rebase_report_with_conflicts() {
    let output = ail()
        .args(["merge", "feature.checkout", "--into", "main", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["rebase_report"].is_object(),
        "rebase_report must be object; got: {v}"
    );
    assert!(
        v["data"]["conflicts"].is_array(),
        "conflicts must be array; got: {v}"
    );
    assert!(
        v["data"]["repair_options"].is_array(),
        "repair_options must be array; got: {v}"
    );
}

// ── G31 R2: refactor behavior locks ──────────────────────────────────────

/// SC-REF1: refactor --json has behavior_locks, contracts_preserved, effects_preserved, proofs_to_rerun.
#[test]
fn refactor_json_has_full_behavior_metadata() {
    let output = ail()
        .args(["refactor", "extract-function", "fn.checkout", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["behavior_locks"].is_array(),
        "behavior_locks must be array; got: {v}"
    );
    assert!(
        v["data"]["contracts_preserved"].is_array(),
        "contracts_preserved must be array; got: {v}"
    );
    assert!(
        v["data"]["effects_preserved"].is_array(),
        "effects_preserved must be array; got: {v}"
    );
    assert!(
        v["data"]["proofs_to_rerun"].is_array(),
        "proofs_to_rerun must be array; got: {v}"
    );
    assert_eq!(
        v["data"]["status"], "draft",
        "refactor ChangeSet must be draft; got: {v}"
    );
}
