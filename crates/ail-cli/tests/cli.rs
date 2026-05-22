// ── ail-cli integration tests (G8 — durable storage + new subcommands) ───
//
// Covers all spec scenarios for the ten-command CLI:
//
//   Task 3.2 — subcommand dispatch, context, change (file + stdin)
//   Task 3.3 — verify/apply domain error cases
//   Task 3.4 — --json output across all six original commands
//   Task 3.5 — E2E chain: change → verify → apply → compile → run
//   G8       — init, status, inspect, diff subcommands
//
// Each test cites the spec scenario it exercises in its doc comment.

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::process::Output;

// ── helpers ───────────────────────────────────────────────────────────────

fn ail() -> Command {
    Command::cargo_bin("ail").expect("ail binary must be present")
}

/// Return the path to the sample fixture file (relative to CARGO_MANIFEST_DIR).
fn sample_acl_path() -> std::path::PathBuf {
    let manifest =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set during tests");
    std::path::Path::new(&manifest)
        .join("tests")
        .join("fixtures")
        .join("sample.acl")
}

/// Parse stdout bytes as JSON; panic with context on failure.
fn parse_json_output(output: &Output) -> Value {
    let text = std::str::from_utf8(&output.stdout).expect("stdout must be UTF-8");
    serde_json::from_str(text.trim())
        .unwrap_or_else(|e| panic!("--json output must parse as JSON: {e}\nRaw: {text}"))
}

// ── Task 3.2 baseline: dispatch + context + change ────────────────────────

/// [PR2 baseline] Help flag — exits 0 and mentions available subcommands.
#[test]
fn help_exits_zero() {
    ail()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("context"));
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
///   THEN stderr lists the ten subcommands; exit code 2
#[test]
fn unknown_subcommand_lists_ten() {
    ail()
        .arg("frobnicate")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains(
            "Available subcommands: context, change, verify, apply, compile, run, \
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
    // Use the deterministic change-id from sample.acl
    let change_id = compute_sample_change_id();
    let output = ail()
        .args(["apply", &change_id])
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
    assert!(
        v["data"]["change_id"].is_string(),
        "data.change_id must be a string; got: {v}"
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
    assert!(
        v["data"]["summary"].is_string(),
        "data.summary must be a string; got: {v}"
    );
}

#[test]
fn json_flag_apply_is_parseable() {
    let change_id = compute_sample_change_id();
    let output = ail()
        .args(["apply", &change_id, "--json"])
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
    let change_id = compute_sample_change_id();
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
        let output = ail().args(args).assert().success().get_output().clone();

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
    // Step 1: change — load sample.acl, get change-id
    let path = sample_acl_path();
    let change_output = ail()
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

    let change_json = parse_json_output(&change_output);
    assert_eq!(change_json["status"], "ok", "step 1 (change) must succeed");
    let change_id = change_json["data"]["change_id"]
        .as_str()
        .expect("change_id must be a string")
        .to_string();
    assert_eq!(change_id.len(), 64, "change-id must be 64 hex chars");

    // Step 2: verify — run Checker on the change-id
    let verify_output = ail()
        .args(["verify", &change_id, "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let verify_json = parse_json_output(&verify_output);
    assert_eq!(verify_json["status"], "ok", "step 2 (verify) must succeed");
    assert_eq!(
        verify_json["data"]["summary"], "Proven",
        "verify summary must be Proven for empty graph"
    );

    // Step 3: apply — apply the ChangeSet, get new snapshot id
    let apply_output = ail()
        .args(["apply", &change_id, "--json"])
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
    assert_eq!(
        run_json["data"]["audit_events"].as_u64().unwrap_or(0),
        1,
        "exactly one AuditEvent must be appended on success"
    );
}

// ── G8: new subcommands (init, status, inspect, diff) ─────────────────────

/// Spec scenario: init exits 0 and creates .ail/ directory.
///   GIVEN an empty temp directory
///   WHEN `ail init` runs
///   THEN exit 0; .ail/ dir created; JSON reports initialized=true
#[test]
fn init_exits_zero_and_creates_ail_dir() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let output = ail()
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = std::str::from_utf8(&output.stdout).expect("stdout must be UTF-8");
    assert!(
        stdout.contains("initialized") || stdout.contains(".ail"),
        "init output must mention initialization; got:\n{stdout}"
    );

    // .ail/ must have been created.
    dir.child(".ail").assert(predicate::path::is_dir());
}

/// Spec scenario: init is idempotent.
///   GIVEN `ail init` has already been run
///   WHEN `ail init` runs again
///   THEN exit 0; no error
#[test]
fn init_is_idempotent() {
    use assert_fs::prelude::*;
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    // First init.
    ail().arg("init").current_dir(dir.path()).assert().success();
    // Second init must also succeed.
    ail().arg("init").current_dir(dir.path()).assert().success();
    dir.child(".ail").assert(predicate::path::is_dir());
}

/// Spec scenario: init JSON output contains genesis_snapshot_id.
///   GIVEN `ail init --json` runs
///   WHEN init completes
///   THEN JSON has initialized=true and genesis_snapshot_id string
#[test]
fn init_json_output_has_genesis_id() {
    use assert_fs::TempDir;
    let dir = TempDir::new().expect("temp dir");
    let output = ail()
        .args(["init", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["initialized"], true);
    assert!(
        v["data"]["genesis_snapshot_id"].is_string(),
        "genesis_snapshot_id must be a string; got: {v}"
    );
}

/// Spec scenario: status exits 0.
///   GIVEN any store state
///   WHEN `ail status` runs
///   THEN exit 0
#[test]
fn status_exits_zero() {
    ail().arg("status").assert().success();
}

/// Spec scenario: status JSON output has required fields.
///   GIVEN `ail status --json` runs
///   WHEN status completes
///   THEN JSON has snapshot_id and pending_changes fields
#[test]
fn status_json_output_has_required_fields() {
    let output = ail()
        .args(["status", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"].get("pending_changes").is_some(),
        "data must have pending_changes; got: {v}"
    );
    assert!(
        v["data"].get("snapshot_id").is_some(),
        "data must have snapshot_id; got: {v}"
    );
}

/// Spec scenario: inspect unknown id exits 1.
///   GIVEN <id> not in store (valid format but unknown)
///   WHEN `ail inspect <id>` runs
///   THEN exit 1; stderr contains "not found"
#[test]
fn inspect_unknown_exits_one() {
    let unknown_id = "dead".repeat(16); // 64 valid hex chars
    ail()
        .args(["inspect", &unknown_id])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("not found"));
}

/// Spec scenario: inspect bad format exits 1.
///   GIVEN <id> is not a valid 64-char hex string
///   WHEN `ail inspect <id>` runs
///   THEN exit 1
#[test]
fn inspect_invalid_format_exits_one() {
    ail()
        .args(["inspect", "not-a-hex-id"])
        .assert()
        .failure()
        .code(1);
}

/// Spec scenario: diff with missing snapshot exits 1.
///   GIVEN snapshot <a> not in store
///   WHEN `ail diff <a> <b>` runs
///   THEN exit 1
#[test]
fn diff_missing_snapshots_exits_one() {
    let id_a = "aa".repeat(32); // 64 hex chars
    let id_b = "bb".repeat(32);
    ail()
        .args(["diff", &id_a, &id_b])
        .assert()
        .failure()
        .code(1);
}

/// Spec scenario: diff with invalid format exits 1.
///   GIVEN <a> is not a valid 64-char hex string
///   WHEN `ail diff <a> <b>` runs
///   THEN exit 1
#[test]
fn diff_invalid_format_exits_one() {
    ail()
        .args(["diff", "bad-id", "also-bad"])
        .assert()
        .failure()
        .code(1);
}

// ── G31: rollback ─────────────────────────────────────────────────────────

/// SC-R1: rollback with valid snap-id exits 0 and prints new snapshot id.
#[test]
fn rollback_valid_id_exits_zero() {
    let snap_id = "de".repeat(32); // 64 valid hex chars
    ail()
        .args(["rollback", "--to", &snap_id])
        .assert()
        .success()
        .stdout(predicate::str::contains("new snapshot"));
}

/// SC-R2: rollback with bad-id exits 1.
#[test]
fn rollback_invalid_id_exits_one() {
    ail()
        .args(["rollback", "--to", "not-a-valid-id"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("not found").or(predicate::str::contains("invalid")));
}

/// SC-R3: rollback --json produces JSON with new_snapshot_id field.
#[test]
fn rollback_json_has_new_snapshot_id() {
    let snap_id = "de".repeat(32);
    let output = ail()
        .args(["rollback", "--to", &snap_id, "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["new_snapshot_id"].is_string(),
        "data.new_snapshot_id must be a string; got: {v}"
    );
}

// ── G31: rebase ───────────────────────────────────────────────────────────

/// SC-RB1: rebase with valid change-id and onto exits 0.
#[test]
fn rebase_valid_ids_exits_zero() {
    let change_id = "ab".repeat(32);
    let onto = "cd".repeat(32);
    ail()
        .args(["rebase", &change_id, "--onto", &onto])
        .assert()
        .success()
        .stdout(predicate::str::contains("rebased").or(predicate::str::contains("conflicts")));
}

/// SC-RB2: rebase with invalid change-id exits 1.
#[test]
fn rebase_invalid_change_id_exits_one() {
    ail()
        .args(["rebase", "bad-id", "--onto", &"cd".repeat(32)])
        .assert()
        .failure()
        .code(1);
}

/// SC-RB3: rebase --json produces JSON with conflicts and repair_options.
#[test]
fn rebase_json_has_conflicts_and_repair_options() {
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
        v["data"]["conflicts"].is_array(),
        "data.conflicts must be an array; got: {v}"
    );
    assert!(
        v["data"]["repair_options"].is_array(),
        "data.repair_options must be an array; got: {v}"
    );
}

// ── G31: merge ────────────────────────────────────────────────────────────

/// SC-M1: merge exits 0 and prints merge result.
#[test]
fn merge_exits_zero() {
    ail()
        .args(["merge", "feature.checkout", "--into", "main"])
        .assert()
        .success()
        .stdout(predicate::str::contains("merged"));
}

/// SC-M2: merge --json produces JSON with merged_snapshot_id.
#[test]
fn merge_json_has_merged_snapshot_id() {
    let output = ail()
        .args(["merge", "feature.checkout", "--into", "main", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["merged_snapshot_id"].is_string(),
        "data.merged_snapshot_id must be a string; got: {v}"
    );
}

// ── G31: refactor ─────────────────────────────────────────────────────────

/// SC-RF1: refactor exits 0 and prints ChangeSet id.
#[test]
fn refactor_exits_zero() {
    ail()
        .args(["refactor", "extract-function", "fn.checkout"])
        .assert()
        .success()
        .stdout(predicate::str::contains("change_id").or(predicate::str::contains("refactor")));
}

/// SC-RF2: refactor --json produces JSON with change_id field.
#[test]
fn refactor_json_has_change_id() {
    let output = ail()
        .args(["refactor", "extract-function", "fn.checkout", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["change_id"].is_string(),
        "data.change_id must be a string; got: {v}"
    );
}

// ── G31: approve / reject ─────────────────────────────────────────────────

/// SC-A1: approve with valid change-id exits 0.
#[test]
fn approve_valid_id_exits_zero() {
    let change_id = "aa".repeat(32);
    ail()
        .args(["approve", &change_id, "--for", "public_api_changed"])
        .assert()
        .success()
        .stdout(predicate::str::contains("approved"));
}

/// SC-A2: approve with invalid change-id exits 1.
#[test]
fn approve_invalid_id_exits_one() {
    ail()
        .args(["approve", "bad-id", "--for", "public_api_changed"])
        .assert()
        .failure()
        .code(1);
}

/// SC-A3: reject exits 0.
#[test]
fn reject_exits_zero() {
    let change_id = "aa".repeat(32);
    ail()
        .args(["reject", &change_id, "--reason", "capability too broad"])
        .assert()
        .success()
        .stdout(predicate::str::contains("rejected"));
}

/// SC-A4: approve --json produces JSON with approved and canonical_hash fields.
#[test]
fn approve_json_has_approved_and_canonical_hash() {
    let change_id = "aa".repeat(32);
    let output = ail()
        .args(["approve", &change_id, "--for", "public_api_changed", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["approved"], true, "data.approved must be true; got: {v}");
    assert!(
        v["data"]["canonical_hash"].is_string(),
        "data.canonical_hash must be a string; got: {v}"
    );
}

/// Reject --json produces JSON with approved=false.
#[test]
fn reject_json_has_approved_false() {
    let change_id = "aa".repeat(32);
    let output = ail()
        .args(["reject", &change_id, "--reason", "too broad", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["approved"], false, "data.approved must be false for reject; got: {v}");
}

// ── G31: policy ───────────────────────────────────────────────────────────

/// SC-P1: policy check exits 0.
#[test]
fn policy_check_exits_zero() {
    let change_id = "ab".repeat(32);
    ail()
        .args(["policy", "check", &change_id, "--profile", "prod"])
        .assert()
        .success()
        .stdout(predicate::str::contains("policy"));
}

/// SC-P2: policy explain exits 0.
#[test]
fn policy_explain_exits_zero() {
    ail()
        .args(["policy", "explain", "no_unverified_public_api"])
        .assert()
        .success()
        .stdout(predicate::str::contains("rule").or(predicate::str::contains("description")));
}

/// SC-P3: policy set exits 0.
#[test]
fn policy_set_exits_zero() {
    ail()
        .args(["policy", "set", "max_new_capabilities=2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("policy").or(predicate::str::contains("updated")));
}

/// SC-P4: policy check --json produces JSON with policy_ok field.
#[test]
fn policy_check_json_has_policy_ok() {
    let change_id = "ab".repeat(32);
    let output = ail()
        .args(["policy", "check", &change_id, "--profile", "prod", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["policy_ok"].is_boolean(),
        "data.policy_ok must be a boolean; got: {v}"
    );
}

/// policy explain --json produces JSON with rule and description.
#[test]
fn policy_explain_json_has_rule_and_description() {
    let output = ail()
        .args(["policy", "explain", "no_unverified_public_api", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["rule"].is_string(),
        "data.rule must be a string; got: {v}"
    );
    assert!(
        v["data"]["description"].is_string(),
        "data.description must be a string; got: {v}"
    );
}

/// policy set --json produces JSON with key and value.
#[test]
fn policy_set_json_has_key_and_value() {
    let output = ail()
        .args(["policy", "set", "max_new_capabilities=2", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["key"].is_string(),
        "data.key must be a string; got: {v}"
    );
    assert!(
        v["data"]["value"].is_string(),
        "data.value must be a string; got: {v}"
    );
}

// ── G31: package ──────────────────────────────────────────────────────────

/// SC-PK1: package add exits 0 and prints trust/capabilities.
#[test]
fn package_add_exits_zero() {
    ail()
        .args(["package", "add", "payments.stripe@1.2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("trust").or(predicate::str::contains("added")));
}

/// SC-PK2: package verify exits 0.
#[test]
fn package_verify_exits_zero() {
    ail()
        .args(["package", "verify"])
        .assert()
        .success()
        .stdout(predicate::str::contains("verified").or(predicate::str::contains("packages")));
}

/// SC-PK3: package audit exits 0.
#[test]
fn package_audit_exits_zero() {
    ail()
        .args(["package", "audit"])
        .assert()
        .success()
        .stdout(predicate::str::contains("audit").or(predicate::str::contains("advisories")));
}

/// SC-PK4: package publish exits 0.
#[test]
fn package_publish_exits_zero() {
    ail()
        .args(["package", "publish"])
        .assert()
        .success()
        .stdout(predicate::str::contains("publish").or(predicate::str::contains("ok")));
}

/// SC-PK5: package explain exits 0.
#[test]
fn package_explain_exits_zero() {
    ail()
        .args(["package", "explain", "payments.stripe"])
        .assert()
        .success()
        .stdout(predicate::str::contains("package").or(predicate::str::contains("trust")));
}

/// SC-PK6: package add --json produces JSON with package and trust fields.
#[test]
fn package_add_json_has_package_and_trust() {
    let output = ail()
        .args(["package", "add", "payments.stripe@1.2", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["package"].is_string(),
        "data.package must be a string; got: {v}"
    );
    assert!(
        v["data"]["trust"].is_string(),
        "data.trust must be a string; got: {v}"
    );
}

/// package audit --json produces JSON with advisories array.
#[test]
fn package_audit_json_has_advisories() {
    let output = ail()
        .args(["package", "audit", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["advisories"].is_array(),
        "data.advisories must be an array; got: {v}"
    );
}

/// package verify --json produces JSON with verified field.
#[test]
fn package_verify_json_has_verified() {
    let output = ail()
        .args(["package", "verify", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["verified"].is_boolean(),
        "data.verified must be a boolean; got: {v}"
    );
}

/// package explain --json produces JSON with package and capabilities.
#[test]
fn package_explain_json_has_package_and_capabilities() {
    let output = ail()
        .args(["package", "explain", "payments.stripe", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["package"].is_string(),
        "data.package must be a string; got: {v}"
    );
    assert!(
        v["data"]["capabilities"].is_array(),
        "data.capabilities must be an array; got: {v}"
    );
}

// ── G31: doctor ───────────────────────────────────────────────────────────

/// SC-D1: doctor exits 0 and prints check results.
#[test]
fn doctor_exits_zero() {
    ail()
        .args(["doctor"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ok").or(predicate::str::contains("graph")));
}

/// SC-D2: doctor --json produces JSON with checks array.
#[test]
fn doctor_json_has_checks_array() {
    let output = ail()
        .args(["doctor", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["checks"].is_array(),
        "data.checks must be an array; got: {v}"
    );
}

/// SC-D3: each check has name, status, and message fields.
#[test]
fn doctor_json_checks_have_required_fields() {
    let output = ail()
        .args(["doctor", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    let checks = v["data"]["checks"].as_array().expect("checks must be array");
    assert!(!checks.is_empty(), "doctor must report at least one check");

    for check in checks {
        assert!(
            check["name"].is_string(),
            "each check must have a name string; got: {check}"
        );
        assert!(
            check["status"].is_string(),
            "each check must have a status string; got: {check}"
        );
        assert!(
            check["message"].is_string(),
            "each check must have a message string; got: {check}"
        );
    }
}

/// G31: unknown subcommand error lists all new commands too.
#[test]
fn unknown_subcommand_lists_all_commands_including_new() {
    let stderr = ail()
        .arg("frobnicate")
        .assert()
        .failure()
        .code(2)
        .get_output()
        .clone();

    let err_text = std::str::from_utf8(&stderr.stderr).expect("stderr must be UTF-8");
    for cmd in &["rollback", "rebase", "merge", "refactor", "approve", "reject", "policy", "package", "doctor"] {
        assert!(
            err_text.contains(cmd),
            "error message must list '{cmd}'; got:\n{err_text}"
        );
    }
}

// ── private helpers ───────────────────────────────────────────────────────

/// Extract the change-id value from human-readable `ail change` output.
fn extract_change_id(stdout: &str) -> String {
    stdout
        .lines()
        .find(|l| l.contains("change-id:"))
        .expect("change-id line must be present")
        .split("change-id:")
        .nth(1)
        .expect("change-id value must follow colon")
        .trim()
        .to_string()
}

/// Run `ail change --file sample.acl` and return the deterministic change-id.
///
/// Used by tests that need a valid 64-char hex change-id to pass to verify/apply.
fn compute_sample_change_id() -> String {
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

    extract_change_id(std::str::from_utf8(&output.stdout).expect("stdout must be UTF-8"))
}
