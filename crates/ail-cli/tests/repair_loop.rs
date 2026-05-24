// ── ail-cli integration tests: repair loop workflow ──────────────────────
//
// Integration coverage for the AI-native repair loop:
//
//   RL-1  `ail verify --json` surfaces repair_options (empty when applyable)
//   RL-2  `ail apply`   persists state to the durable file store
//   RL-3  Re-verify     observes the updated state (stale base → rebase_required)
//
// Each test uses a temp dir with `ail init` to exercise the file-backed store,
// which is the only backend that persists state across process invocations.
// All JSON shapes follow the `workflow_state` schema defined in
// crates/ail-cli/src/workflow_commands.rs.

mod common;

use common::{ail, create_sample_change, parse_json_output};

// ── RL-1: verify happy path surfaces empty repair_options ─────────────────

/// Repair loop scenario RL-1: applyable state has no repair options.
///
///   GIVEN an initialised project with a persisted changeset
///   WHEN  `ail verify <change-id> --json` runs
///   THEN  workflow_state.applyable is true
///    AND  workflow_state.repair_options is an empty array
///    AND  workflow_state.next_action is "apply"
#[test]
fn repair_loop_verify_applyable_has_empty_repair_options() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let change_id = create_sample_change(dir.path());

    let output = ail()
        .args(["verify", &change_id, "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(
        v["data"]["workflow_state"]["applyable"], true,
        "fresh changeset in initialised project must be applyable; got: {v}"
    );
    assert_eq!(
        v["data"]["workflow_state"]["next_action"], "apply",
        "next_action must be 'apply' when applyable; got: {v}"
    );
    let repair_options = v["data"]["workflow_state"]["repair_options"]
        .as_array()
        .expect("workflow_state.repair_options must be an array");
    assert!(
        repair_options.is_empty(),
        "applyable state must surface no repair_options; got: {v}"
    );
}

// ── RL-2: apply persists state; re-verify observes stale base ────────────

/// Repair loop scenario RL-2: full state-transition cycle.
///
///   GIVEN an initialised project with a persisted changeset
///   WHEN  (a) `ail verify <change-id> --json` confirms the change is applyable
///    AND  (b) `ail apply  <change-id>`        persists a new snapshot to store
///    AND  (c) `ail verify <change-id> --json`  re-runs (re-verify)
///   THEN  re-verify workflow_state.rebase_required is true
///    AND  workflow_state.applyable is false
///    AND  workflow_state.repair_options contains an entry with code "rebase_required"
///    AND  workflow_state.next_action is "rebase"
///
/// This test proves that apply's persisted snapshot is observed by the next
/// verify invocation, closing the AI repair loop.
#[test]
fn repair_loop_apply_persists_state_reverify_observes_stale_base() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let change_id = create_sample_change(dir.path());

    // Step 1 — pre-apply verify: confirm the change is applyable.
    let pre_out = ail()
        .args(["verify", &change_id, "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let pre = parse_json_output(&pre_out);
    assert_eq!(
        pre["data"]["workflow_state"]["applyable"], true,
        "pre-apply verify must be applyable; got: {pre}"
    );

    // Step 2 — apply: persist the snapshot to the file store.
    ail()
        .args(["apply", &change_id])
        .current_dir(dir.path())
        .assert()
        .success();

    // Step 3 — re-verify: the base snapshot has advanced; rebase required.
    let post_out = ail()
        .args(["verify", &change_id, "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let post = parse_json_output(&post_out);

    assert_eq!(
        post["data"]["workflow_state"]["rebase_required"], true,
        "re-verify after apply must flag rebase_required; got: {post}"
    );
    assert_eq!(
        post["data"]["workflow_state"]["applyable"], false,
        "re-verify must not be applyable after stale base; got: {post}"
    );
    assert_eq!(
        post["data"]["workflow_state"]["next_action"], "rebase",
        "next_action must be 'rebase' after stale base; got: {post}"
    );
    let opts = post["data"]["workflow_state"]["repair_options"]
        .as_array()
        .expect("workflow_state.repair_options must be an array");
    assert!(
        opts.iter().any(|o| o["code"] == "rebase_required"),
        "re-verify must surface a rebase_required repair option; got: {post}"
    );
}

// ── RL-3: re-verify repair option carries a non-null current_snapshot_id ──

/// Repair loop scenario RL-3: repair option exposes current_snapshot_id after apply.
///
///   GIVEN an initialised project with a persisted changeset
///   WHEN  `ail apply  <change-id>`        persists a new snapshot to the store
///    AND  `ail verify <change-id> --json` re-runs after apply
///   THEN  the rebase_required repair option carries a non-null current_snapshot_id
///
/// Verifies that the repair option exposes a machine-readable current_snapshot_id
/// field so automation can reference the obstacle snapshot without parsing
/// free-form text.  Identity between apply's new_snapshot_id (ObjectId) and
/// the option's current_snapshot_id (SnapshotId/u64) is not asserted here
/// because the two fields use different schema types.
#[test]
fn repair_loop_reverify_repair_option_has_current_snapshot_id() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let change_id = create_sample_change(dir.path());

    // Apply: persist the snapshot to the file store.
    ail()
        .args(["apply", &change_id])
        .current_dir(dir.path())
        .assert()
        .success();

    // Re-verify and locate the rebase_required repair option.
    let verify_out = ail()
        .args(["verify", &change_id, "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let verify_json = parse_json_output(&verify_out);
    let opts = verify_json["data"]["workflow_state"]["repair_options"]
        .as_array()
        .expect("workflow_state.repair_options must be an array");
    let rebase_opt = opts
        .iter()
        .find(|o| o["code"] == "rebase_required")
        .expect("rebase_required repair option must be present after apply");

    // current_snapshot_id is the SnapshotId(u64) recorded by the apply outcome.
    // It must be present and non-null so automation can pass it to a rebase call.
    assert!(
        !rebase_opt["current_snapshot_id"].is_null(),
        "rebase_required repair option must carry a current_snapshot_id; got: {verify_json}"
    );
}

// ── RL-4: re-verify repair option next_action is "rebase" ────────────────

/// Repair loop scenario RL-4: repair option next_action is machine-readable.
///
///   GIVEN an initialised project after a successful apply
///   WHEN  re-verify runs
///   THEN  the rebase_required repair option next_action is "rebase"
///    AND  the repair option description is non-empty
///
/// Validates that the repair option carries all fields an AI agent needs to
/// decide its next step without parsing free-form text.
#[test]
fn repair_loop_reverify_repair_option_is_actionable() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let change_id = create_sample_change(dir.path());

    ail()
        .args(["apply", &change_id])
        .current_dir(dir.path())
        .assert()
        .success();

    let out = ail()
        .args(["verify", &change_id, "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let v = parse_json_output(&out);
    let opts = v["data"]["workflow_state"]["repair_options"]
        .as_array()
        .expect("repair_options must be an array");
    let rebase_opt = opts
        .iter()
        .find(|o| o["code"] == "rebase_required")
        .expect("rebase_required option must be present");

    assert_eq!(
        rebase_opt["next_action"], "rebase",
        "next_action must be 'rebase'; got: {rebase_opt}"
    );
    let desc = rebase_opt["description"]
        .as_str()
        .expect("description must be a string");
    assert!(
        !desc.is_empty(),
        "description must be non-empty for AI agent guidance"
    );
}
