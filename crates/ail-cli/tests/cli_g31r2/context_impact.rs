use super::common::{ail, parse_json_output};
use predicates::prelude::*;

// ── G31 R2: context with target ──────────────────────────────────────────

/// SC-CTX1: context with target returns hash-bound context slice.
#[test]
fn context_with_target_exits_zero() {
    ail()
        .args(["context", "fn.checkout"])
        .assert()
        .success()
        .stdout(predicate::str::contains("target").or(predicate::str::contains("snapshot")));
}

/// SC-CTX2: context with target --json has context with snapshot_id and hash.
#[test]
fn context_with_target_json_has_context_slice() {
    let output = ail()
        .args(["context", "fn.checkout", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    let ctx = &v["data"]["context"];
    assert!(ctx.is_object(), "data.context must be an object; got: {v}");
    assert!(
        ctx["target"].is_string(),
        "context.target must be a string; got: {ctx}"
    );
    assert!(
        ctx["snapshot_id"].is_string(),
        "context.snapshot_id must be a string; got: {ctx}"
    );
    assert!(
        ctx["snapshot_hash"].is_string(),
        "context.snapshot_hash must be a string; got: {ctx}"
    );
}

// ── G31 R2: impact / callers / effects / proofs ───────────────────────────

/// SC-IMP1: impact returns hash-bound affected_nodes.
#[test]
fn impact_exits_zero_with_snapshot_hash() {
    let output = ail()
        .args(["impact", "type.CartItem.price", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["affected_nodes"].is_array(),
        "affected_nodes must be array; got: {v}"
    );
    assert!(
        v["data"]["snapshot_id"].is_string(),
        "snapshot_id must be string; got: {v}"
    );
    assert!(
        v["data"]["snapshot_hash"].is_string(),
        "snapshot_hash must be string; got: {v}"
    );
}

/// SC-CAL1: callers returns hash-bound callers list.
#[test]
fn callers_exits_zero_with_snapshot_hash() {
    let output = ail()
        .args(["callers", "fn.cart_total", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["callers"].is_array(),
        "callers must be array; got: {v}"
    );
    assert!(
        v["data"]["snapshot_hash"].is_string(),
        "snapshot_hash must be string; got: {v}"
    );
}

/// SC-EFF1: effects returns hash-bound effects list.
#[test]
fn effects_exits_zero_with_snapshot_hash() {
    let output = ail()
        .args(["effects", "module.payment", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["effects"].is_array(),
        "effects must be array; got: {v}"
    );
    assert!(
        v["data"]["snapshot_hash"].is_string(),
        "snapshot_hash must be string; got: {v}"
    );
}

/// SC-PRF1: proofs returns hash-bound proof_obligations.
#[test]
fn proofs_exits_zero_with_snapshot_hash() {
    let output = ail()
        .args(["proofs", "invariant.stock_never_negative", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["proof_obligations"].is_array(),
        "proof_obligations must be array; got: {v}"
    );
    assert!(
        v["data"]["snapshot_hash"].is_string(),
        "snapshot_hash must be string; got: {v}"
    );
}
