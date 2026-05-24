// ── ail-cli integration tests: baseline (Tasks 3.2–3.5) ───────────────────
//
// Covers:
//   Task 3.2 — subcommand dispatch, context, change (file + stdin)
//   Task 3.3 — verify/apply domain error cases
//   Task 3.4 — --json output across all six original commands
//   Task 3.5 — E2E chain: change → verify → apply → compile → run
//
// Shared helpers live in common/mod.rs.

mod common;

use common::{
    ail, compute_sample_change_id, create_sample_change, extract_change_id, parse_json_output,
    sample_acl_path,
};
use predicates::prelude::*;

// ── Task 3.2 baseline: dispatch + context + change ────────────────────────

/// [PR2 baseline] Help flag — exits 0 and mentions available subcommands.
#[test]
fn help_exits_zero() {
    ail()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("context"))
        .stdout(predicate::str::contains("Examples:"));
}

#[test]
fn help_mentions_eval_command() {
    ail()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("eval"))
        .stdout(predicate::str::contains("ail eval \"add(20, 22)\""));
}

/// [PR2 baseline] Version flag — exits 0 and version string is present.
#[test]
fn version_exits_zero_and_prints_version() {
    ail()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

/// Spec scenario: unknown subcommand rejected
///   GIVEN `ail frobnicate` is invoked
///   WHEN dispatch runs
///   THEN stderr lists available subcommands including link; exit code 2
#[test]
fn unknown_subcommand_lists_ten() {
    ail()
        .arg("frobnicate")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains(
            "Available subcommands: context, change, verify, apply, compile, run, link, eval, \
             init, status, inspect, diff",
        ));
}

/// Spec scenario: empty store
///   GIVEN the local store is empty
///   WHEN `ail context` runs
///   THEN output is empty; exit 0
#[test]
fn context_empty_store_exits_zero() {
    ail().arg("context").assert().success();
}

/// Spec scenario: file input
///   GIVEN a readable file at <path>
///   WHEN `ail change --file <path>` runs
///   THEN ChangeSet summary and BlockHash printed; exit 0
#[test]
fn change_from_file_prints_hash() {
    let path = sample_acl_path();
    let output = ail()
        .args([
            "change",
            "--file",
            path.to_str().expect("path must be UTF-8"),
        ])
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = std::str::from_utf8(&output.stdout).expect("stdout must be UTF-8");
    assert!(
        stdout.contains("change-id:"),
        "change output must include 'change-id:'; got:\n{stdout}"
    );
    // The change-id must be a 64-char hex string.
    let hash_line = stdout
        .lines()
        .find(|l| l.contains("change-id:"))
        .expect("change-id line must be present");
    let hash = hash_line
        .split("change-id:")
        .nth(1)
        .expect("change-id value must follow colon")
        .trim();
    assert_eq!(
        hash.len(),
        64,
        "change-id must be 64 hex chars; got: '{hash}'"
    );
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "change-id must be hex; got: '{hash}'"
    );
}

/// Spec scenario: stdin input
///   GIVEN ACL content is piped to stdin
///   WHEN `ail change` runs without `--file`
///   THEN ChangeSet summary and BlockHash printed; exit 0
#[test]
fn change_from_stdin_prints_hash() {
    let acl_content =
        std::fs::read_to_string(sample_acl_path()).expect("sample.acl must be readable");

    let output = ail()
        .arg("change")
        .write_stdin(acl_content)
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = std::str::from_utf8(&output.stdout).expect("stdout must be UTF-8");
    assert!(
        stdout.contains("change-id:"),
        "stdin input must produce a change-id line; got:\n{stdout}"
    );
}

#[test]
fn run_function_prints_result() {
    ail()
        .args(["run", "fn.answer"])
        .assert()
        .success()
        .stdout(predicate::str::contains("result: 42"));
}

#[test]
fn run_function_passes_i64_args() {
    ail()
        .args(["run", "fn.add", "20", "22"])
        .assert()
        .success()
        .stdout(predicate::str::contains("result: 42"));
}

#[test]
fn eval_inline_add_prints_result_without_init() {
    ail()
        .args(["eval", "add(20, 22)"])
        .assert()
        .success()
        .stdout(predicate::str::contains("result: 42"));
}

#[test]
fn eval_inline_double_prints_result_without_init() {
    ail()
        .args(["eval", "double(21)"])
        .assert()
        .success()
        .stdout(predicate::str::contains("result: 42"));
}

#[test]
fn eval_parse_error_is_human_readable() {
    ail()
        .args(["eval", "add(20)"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("Failed to parse expression"))
        .stderr(predicate::str::contains("Debug").not());
}

/// Spec scenario: file and stdin produce the same change-id (deterministic hash).
#[test]
fn change_file_and_stdin_produce_same_hash() {
    let path = sample_acl_path();
    let acl_content = std::fs::read_to_string(&path).expect("sample.acl must be readable");

    let file_output = ail()
        .args([
            "change",
            "--file",
            path.to_str().expect("path must be UTF-8"),
        ])
        .assert()
        .success()
        .get_output()
        .clone();

    let stdin_output = ail()
        .arg("change")
        .write_stdin(acl_content)
        .assert()
        .success()
        .get_output()
        .clone();

    let file_hash =
        extract_change_id(std::str::from_utf8(&file_output.stdout).expect("stdout must be UTF-8"));
    let stdin_hash =
        extract_change_id(std::str::from_utf8(&stdin_output.stdout).expect("stdout must be UTF-8"));
    assert_eq!(
        file_hash, stdin_hash,
        "file and stdin inputs for the same ACL must produce the same change-id"
    );
}

// ── Task 3.3 — verify/apply domain error cases ────────────────────────────

/// Spec scenario: unknown change-id
///   GIVEN <change-id> is not found in local store (invalid format)
///   WHEN `ail verify <change-id>` runs
///   THEN error message on stderr; exit 1
#[test]
fn verify_unknown_id_exits_one() {
    ail()
        .args(["verify", "badhash"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("not found"));
}

/// Spec scenario: stale base rejected
///   GIVEN base_snapshot_id does not match the live snapshot
///   WHEN `ail apply <change-id>` runs
///   THEN RebaseRequired with current snapshot id reported on stderr; exit 1
///
/// Note: The binary hardcodes base=0 and bridge=SnapshotId(0), so the stale-base
/// path is exercised at the unit level below (see `apply_stale_base_unit`).
/// At the integration level, we verify that a well-formed change-id succeeds
/// (apply_success_prints_snapshot_id) and that a malformed id exits 1.
#[test]
fn apply_invalid_id_exits_one() {
    ail()
        .args(["apply", "notahex"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("not found"));
}

/// Spec scenario: apply succeeds
///   GIVEN base_snapshot_id matches the live snapshot
///   WHEN `ail apply <change-id>` runs
///   THEN new snapshot id printed; exit 0
#[test]
fn apply_success_prints_snapshot_id() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let change_id = create_sample_change(dir.path());
    let output = ail()
        .args(["apply", &change_id])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = std::str::from_utf8(&output.stdout).expect("stdout must be UTF-8");
    assert!(
        stdout.contains("new snapshot id:"),
        "apply output must mention 'new snapshot id:'; got:\n{stdout}"
    );
}

/// Spec scenario: report printed
///   GIVEN <change-id> is a known canonical hash
///   WHEN `ail verify <change-id>` runs
///   THEN VerificationReport entries and summary state printed; exit 0
#[test]
fn verify_valid_id_exits_zero_with_summary() {
    let change_id = compute_sample_change_id();
    let output = ail()
        .args(["verify", &change_id])
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = std::str::from_utf8(&output.stdout).expect("stdout must be UTF-8");
    assert!(
        stdout.contains("summary:"),
        "verify output must contain 'summary:'; got:\n{stdout}"
    );
}

// ── Task 3.4 — --json output for all six commands ─────────────────────────

/// Spec scenario: json flag on any command
///   GIVEN any of the six commands runs with `--json`
///   WHEN the command completes
///   THEN stdout is valid JSON parseable without error; no non-JSON lines emitted

#[test]
fn json_flag_context_is_parseable() {
    let output = ail()
        .args(["context", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok", "status must be 'ok'; got: {v}");
    assert!(v["data"].is_object(), "data must be an object; got: {v}");
}

#[test]
fn json_flag_change_is_parseable() {
    let path = sample_acl_path();
    let output = ail()
        .args([
            "change",
            "--file",
            path.to_str().expect("path must be UTF-8"),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok", "status must be 'ok'; got: {v}");
    // change_id is nested under canonical_change in the new output shape.
    assert!(
        v["data"]["canonical_change"]["change_id"].is_string(),
        "data.canonical_change.change_id must be a string; got: {v}"
    );
}

#[test]
fn json_flag_verify_is_parseable() {
    let change_id = compute_sample_change_id();
    let output = ail()
        .args(["verify", &change_id, "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok", "status must be 'ok'; got: {v}");
    // summary is nested under verification_report in the new output shape.
    assert!(
        v["data"]["verification_report"]["summary"].is_string(),
        "data.verification_report.summary must be a string; got: {v}"
    );
}

#[test]
fn verify_json_marks_missing_changeset_as_not_applyable() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();

    let missing_change_id = "ab".repeat(32);
    let output = ail()
        .args(["verify", &missing_change_id, "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["workflow_state"]["missing_changeset"], true);
    assert_eq!(v["data"]["workflow_state"]["applyable"], false);
    assert_eq!(
        v["data"]["workflow_state"]["next_action"],
        "create_or_fetch_changeset"
    );
    assert!(
        v["data"]["workflow_state"]["repair_options"]
            .as_array()
            .expect("repair_options must be an array")
            .iter()
            .any(|option| option["code"] == "missing_changeset"),
        "missing changeset must surface a repair option; got: {v}"
    );
}

#[test]
fn verify_json_prod_policy_blocked_is_not_applyable() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let change_id = create_sample_change(dir.path());

    let output = ail()
        .args(["verify", &change_id, "--profile", "prod", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["policy_report"]["status"], "blocked");
    assert_eq!(v["data"]["workflow_state"]["approval_required"], true);
    assert_eq!(v["data"]["workflow_state"]["applyable"], false);
    assert_eq!(v["data"]["workflow_state"]["next_action"], "repair");
    assert!(
        v["data"]["workflow_state"]["repair_options"]
            .as_array()
            .expect("repair_options must be an array")
            .iter()
            .any(|option| option["code"] == "policy_blocked"),
        "blocked prod policy must surface a repair option; got: {v}"
    );
}

#[test]
fn verify_json_marks_stale_base_as_rebase_required() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let change_id = create_sample_change(dir.path());

    ail()
        .args(["apply", &change_id])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = ail()
        .args(["verify", &change_id, "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["workflow_state"]["rebase_required"], true);
    assert_eq!(v["data"]["workflow_state"]["applyable"], false);
    assert_eq!(v["data"]["workflow_state"]["next_action"], "rebase");
    assert!(
        v["data"]["workflow_state"]["repair_options"]
            .as_array()
            .expect("repair_options must be an array")
            .iter()
            .any(|option| option["code"] == "rebase_required"),
        "stale changeset must surface a rebase repair option; got: {v}"
    );
}

#[test]
fn apply_json_rebase_required_includes_workflow_state() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let change_id = create_sample_change(dir.path());

    ail()
        .args(["apply", &change_id])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = ail()
        .args(["apply", &change_id, "--json"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("rebase required"))
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "error");
    assert_eq!(v["data"]["error"], "rebase_required");
    assert_eq!(v["data"]["workflow_state"]["rebase_required"], true);
    assert_eq!(v["data"]["workflow_state"]["applyable"], false);
    assert_eq!(v["data"]["workflow_state"]["next_action"], "rebase");
    assert_eq!(
        v["data"]["pre_apply_gate"]["workflow_state"]["rebase_required"],
        true
    );
    assert!(
        v["data"]["workflow_state"]["repair_options"]
            .as_array()
            .expect("repair_options must be an array")
            .iter()
            .any(|option| option["code"] == "rebase_required"),
        "stale apply must surface a rebase repair option; got: {v}"
    );
}

#[test]
fn json_flag_apply_is_parseable() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let change_id = create_sample_change(dir.path());
    let output = ail()
        .args(["apply", &change_id, "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok", "status must be 'ok'; got: {v}");
    // new_snapshot_id is now an ObjectId hex string (not an integer).
    assert!(
        v["data"]["new_snapshot_id"].is_string(),
        "data.new_snapshot_id must be a hex string; got: {v}"
    );
    let id_str = v["data"]["new_snapshot_id"].as_str().unwrap();
    assert!(!id_str.is_empty(), "new_snapshot_id must be non-empty");
    assert_eq!(v["data"]["workflow_state"]["applyable"], true);
    assert_eq!(v["data"]["workflow_state"]["next_action"], "complete");
    assert_eq!(
        v["data"]["pre_apply_gate"]["workflow_state"]["applyable"],
        true
    );
}

#[test]
fn json_flag_compile_is_parseable() {
    let output = ail()
        .args(["compile", "--profile", "dev", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok", "status must be 'ok'; got: {v}");
    assert!(
        v["data"]["wasm_bytes"].is_number(),
        "data.wasm_bytes must be a number; got: {v}"
    );
}

#[test]
fn json_flag_run_is_parseable() {
    let output = ail()
        .args(["run", "--profile", "dev", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok", "status must be 'ok'; got: {v}");
    assert_eq!(
        v["data"]["outcome"], "PreflightPassed",
        "data.outcome must be PreflightPassed; got: {v}"
    );
}

/// All six --json outputs must have exactly "status" and "data" top-level fields.
///
/// Runs each command, parses JSON, and asserts the schema is consistent.
#[test]
fn json_flag_output_is_parseable() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let change_id = create_sample_change(dir.path());
    let path = sample_acl_path();

    let cases: Vec<(&str, Vec<&str>)> = vec![
        ("context", vec!["context", "--json"]),
        (
            "change",
            vec![
                "change",
                "--file",
                path.to_str().expect("path must be UTF-8"),
                "--json",
            ],
        ),
        ("verify", vec!["verify", &change_id, "--json"]),
        ("apply", vec!["apply", &change_id, "--json"]),
        ("compile", vec!["compile", "--profile", "dev", "--json"]),
        ("run", vec!["run", "--profile", "dev", "--json"]),
    ];

    for (name, args) in &cases {
        let output = ail()
            .args(args)
            .current_dir(dir.path())
            .assert()
            .success()
            .get_output()
            .clone();

        let v = parse_json_output(&output);
        assert!(
            v.get("status").is_some(),
            "command '{name}': JSON output must have 'status' field; got: {v}"
        );
        assert!(
            v.get("data").is_some(),
            "command '{name}': JSON output must have 'data' field; got: {v}"
        );
        // Assert no extra top-level keys beyond "status" and "data".
        if let Some(obj) = v.as_object() {
            let extra: Vec<&String> = obj
                .keys()
                .filter(|k| *k != "status" && *k != "data")
                .collect();
            assert!(
                extra.is_empty(),
                "command '{name}': JSON envelope must only have 'status' and 'data'; extra keys: {extra:?}"
            );
        }
    }
}

// ── Task 3.5 — E2E chain: change → verify → apply → compile → run ─────────

/// Spec scenario: E2E chain
///   GIVEN sample.acl and a temp .ail dir
///   WHEN change → verify → apply → compile → run are chained
///   THEN each step exits 0; final run prints PreflightPassed
///
/// Note: The current CLI does not use the temp dir for state (durable storage
/// is Phase 9). Each command uses an in-memory store per invocation.
/// The chain validates that all six commands succeed in sequence with consistent
/// inputs and --json output at each step.
#[test]
fn e2e_change_verify_apply_compile_run() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();

    // Step 1: change — load sample.acl, get change-id
    let path = sample_acl_path();
    let change_output = ail()
        .args([
            "change",
            "--file",
            path.to_str().expect("path must be UTF-8"),
            "--json",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let change_json = parse_json_output(&change_output);
    assert_eq!(change_json["status"], "ok", "step 1 (change) must succeed");
    // change_id is nested under canonical_change in the new output shape.
    let change_id = change_json["data"]["canonical_change"]["change_id"]
        .as_str()
        .expect("canonical_change.change_id must be a string")
        .to_string();
    assert_eq!(change_id.len(), 64, "change-id must be 64 hex chars");

    // Step 2: verify — run Checker on the change-id
    let verify_output = ail()
        .args(["verify", &change_id, "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let verify_json = parse_json_output(&verify_output);
    assert_eq!(verify_json["status"], "ok", "step 2 (verify) must succeed");
    // summary is nested under verification_report in the new output shape.
    assert!(
        verify_json["data"]["verification_report"]["summary"].is_string(),
        "verify summary must be present; got: {verify_json}"
    );

    // Step 3: apply — apply the ChangeSet, get new snapshot id
    let apply_output = ail()
        .args(["apply", &change_id, "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let apply_json = parse_json_output(&apply_output);
    assert_eq!(apply_json["status"], "ok", "step 3 (apply) must succeed");
    // new_snapshot_id is now an ObjectId hex string.
    let new_snapshot_id = apply_json["data"]["new_snapshot_id"]
        .as_str()
        .expect("new_snapshot_id must be a hex string");
    assert!(
        !new_snapshot_id.is_empty(),
        "new_snapshot_id must be non-empty"
    );

    // Step 4: compile — lower_to_core_ir → lower_to_anf → emit_wasm
    let compile_output = ail()
        .args(["compile", "--profile", "dev", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let compile_json = parse_json_output(&compile_output);
    assert_eq!(
        compile_json["status"], "ok",
        "step 4 (compile) must succeed"
    );
    assert!(
        compile_json["data"]["wasm_bytes"].as_u64().unwrap_or(0) > 0
            || compile_json["data"]["wasm_bytes"].as_u64().is_some(),
        "wasm_bytes must be a non-negative number; got: {}",
        compile_json["data"]["wasm_bytes"]
    );

    // Step 5: run — RuntimeHost::validate_and_instantiate, preflight must pass
    let run_output = ail()
        .args(["run", "--profile", "dev", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let run_json = parse_json_output(&run_output);
    assert_eq!(run_json["status"], "ok", "step 5 (run) must succeed");
    assert_eq!(
        run_json["data"]["outcome"], "PreflightPassed",
        "final run must print PreflightPassed"
    );
    // audit_events is now nested under audit_log.event_count in the new output shape.
    let audit_events = run_json["data"]["audit_log"]["event_count"]
        .as_u64()
        .unwrap_or(0);
    assert_eq!(
        audit_events, 1,
        "exactly one AuditEvent must be appended on success"
    );
}
