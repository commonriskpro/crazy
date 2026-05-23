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

use ail_storage::SnapshotEnvelope;
use ail_storage::codec::{CborCodec, ContentCodec};
use ail_storage::object::ObjectId;
use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::fs;
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

fn write_raw_object(project_dir: &std::path::Path, bytes: Vec<u8>) -> String {
    let id = ObjectId::from_bytes(&bytes);
    let objects_dir = project_dir.join(".ail").join("store").join("objects");
    fs::create_dir_all(&objects_dir).expect("object directory must be created");
    fs::write(objects_dir.join(id.to_hex()), bytes).expect("object must be written");
    id.to_hex()
}

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
///   THEN stderr lists the ten subcommands; exit code 2
#[test]
fn unknown_subcommand_lists_ten() {
    ail()
        .arg("frobnicate")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains(
            "Available subcommands: context, change, verify, apply, compile, run, eval, \
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

/// Spec scenario: file-backed store persists between CLI invocations.
///   GIVEN `ail init` has created an on-disk store
///   WHEN `ail change` writes a snapshot and `ail compile` runs later
///   THEN compile loads the persisted graph and .ail layout exists
/// Spec scenario: file-backed store persists between CLI invocations.
///   GIVEN `ail init` has created an on-disk store
///   WHEN `ail change --file` creates a draft and `ail apply` applies it,
///   THEN compile loads the persisted graph and .ail layout exists.
///
/// Note: `ail change --file` defaults to draft mode (no snapshot).
/// `ail apply <change_id>` is required to create the snapshot.
#[test]
fn disk_store_persists_change_for_compile() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");

    ail().arg("init").current_dir(dir.path()).assert().success();

    // Step 1: create draft changeset.
    let change_output = ail()
        .args([
            "change",
            "--file",
            sample_acl_path().to_str().expect("path must be UTF-8"),
            "--json",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let change_json = parse_json_output(&change_output);
    assert_eq!(
        change_json["data"]["status"], "draft",
        "change must default to draft"
    );
    let change_id = change_json["data"]["canonical_change"]["change_id"]
        .as_str()
        .expect("change_id must be present")
        .to_string();

    // Step 2: verify the draft from persisted .ail state.
    let verify_output = ail()
        .args(["verify", &change_id, "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let verify_json = parse_json_output(&verify_output);
    assert_eq!(verify_json["status"], "ok");
    assert!(verify_json["data"]["verification_report"].is_object());

    // Step 3: apply the draft to create a snapshot.
    ail()
        .args(["apply", &change_id])
        .current_dir(dir.path())
        .assert()
        .success();

    let compile_output = ail()
        .args(["compile", "--profile", "dev", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let compile_json = parse_json_output(&compile_output);
    assert_eq!(compile_json["status"], "ok");

    let status_output = ail()
        .args(["status", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let status_json = parse_json_output(&status_output);
    assert_eq!(status_json["status"], "ok");
    assert_eq!(status_json["data"]["snapshot_count"], 2);
    assert_eq!(status_json["data"]["graph_nodes"], 1);
    assert!(status_json["data"]["head_snapshot"].is_string());
    assert!(status_json["data"]["last_change_at"].is_string());

    let context_output = ail()
        .args(["context", "fn.sample", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let context_json = parse_json_output(&context_output);
    assert_eq!(context_json["status"], "ok");
    assert_eq!(context_json["data"]["context"]["target"], "fn.sample");
    assert!(context_json["data"]["context"]["nodes"].is_array());

    let run_output = ail()
        .args(["run", "--profile", "dev", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let run_json = parse_json_output(&run_output);
    assert_eq!(run_json["status"], "ok");
    assert_eq!(run_json["data"]["outcome"], "PreflightPassed");

    dir.child(".ail/HEAD").assert(predicate::path::exists());
    dir.child(".ail/refs/branches/main")
        .assert(predicate::path::exists());
    dir.child(".ail/store/objects")
        .assert(predicate::path::is_dir());
}

#[test]
fn init_branch_writes_indirect_head_and_status_shows_branch() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");

    ail()
        .args(["init", "--branch", "feature.disk"])
        .current_dir(dir.path())
        .assert()
        .success();

    let head = std::fs::read_to_string(dir.path().join(".ail").join("HEAD")).expect("read HEAD");
    assert_eq!(head, "ref: refs/branches/feature.disk\n");

    let output = ail()
        .args(["status", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let v = parse_json_output(&output);
    assert_eq!(v["data"]["branch"], "feature.disk");
}

#[test]
fn change_branch_targets_named_branch_ref() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");

    ail().arg("init").current_dir(dir.path()).assert().success();
    // Use --apply to actually create the snapshot on the named branch.
    ail()
        .args([
            "change",
            "branch-specific snapshot",
            "--branch",
            "experiment",
            "--apply",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    let branch_ref = dir
        .path()
        .join(".ail")
        .join("refs")
        .join("branches")
        .join("experiment");
    let value = std::fs::read_to_string(branch_ref).expect("read experiment ref");
    assert_eq!(value.trim().len(), 64, "branch ref must store snapshot id");
}

#[test]
fn doctor_and_gc_report_file_store_object_counts() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");

    ail().arg("init").current_dir(dir.path()).assert().success();
    ail()
        .args(["change", "doctor gc snapshot"])
        .current_dir(dir.path())
        .assert()
        .success();

    let doctor_output = ail()
        .args(["doctor", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let doctor_json = parse_json_output(&doctor_output);
    assert!(
        doctor_json["data"]["objects"]["total"]
            .as_u64()
            .unwrap_or(0)
            > 0
    );

    let gc_output = ail()
        .args(["gc", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let gc_json = parse_json_output(&gc_output);
    assert!(gc_json["data"]["objects_before"].is_number());
    assert!(gc_json["data"]["objects_after"].is_number());
    assert!(gc_json["data"]["bytes_freed"].is_number());
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

/// Spec scenario: inspect unknown snapshot id exits 1.
///   GIVEN <id> not in store (valid format but unknown)
///   WHEN `ail inspect snapshot <id>` runs
///   THEN exit 1; stderr contains "not found"
#[test]
fn inspect_unknown_exits_one() {
    let unknown_id = "dead".repeat(16); // 64 valid hex chars
    ail()
        .args(["inspect", "snapshot", &unknown_id])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("not found"));
}

/// Spec scenario: inspect unknown kind exits 1.
///   GIVEN <kind> is unknown and <id> is not a valid 64-char hex string
///   WHEN `ail inspect <kind> <id>` runs
///   THEN exit 1
#[test]
fn inspect_invalid_format_exits_one() {
    ail()
        .args(["inspect", "unknownkind", "not-a-hex-id"])
        .assert()
        .failure()
        .code(1);
}

/// Spec scenario: diff with missing snapshot exits 1.
///   GIVEN snapshot <a> not in store
///   WHEN `ail diff <a>..<b>` runs (range notation)
///   THEN exit 1
#[test]
fn diff_missing_snapshots_exits_one() {
    let id_a = "aa".repeat(32); // 64 hex chars
    let id_b = "bb".repeat(32);
    let range = format!("{id_a}..{id_b}");
    ail().args(["diff", &range]).assert().failure().code(1);
}

/// Spec scenario: diff with invalid format exits 1.
///   GIVEN <target> contains an invalid hex string in range notation
///   WHEN `ail diff <range>` runs
///   THEN exit 1
#[test]
fn diff_invalid_format_exits_one() {
    ail()
        .args(["diff", "bad-id..also-bad"])
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
        .args([
            "approve",
            &change_id,
            "--for",
            "public_api_changed",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(
        v["data"]["approved"], true,
        "data.approved must be true; got: {v}"
    );
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
    assert_eq!(
        v["data"]["approved"], false,
        "data.approved must be false for reject; got: {v}"
    );
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

// ── Remote submit ─────────────────────────────────────────────────────────

#[test]
fn remote_submit_help_mentions_signer() {
    ail()
        .args(["remote", "submit", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--signer"));
}

#[test]
fn remote_submit_json_uses_in_process_exchange() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");

    ail().arg("init").current_dir(dir.path()).assert().success();

    let change_output = ail()
        .args([
            "change",
            "--file",
            sample_acl_path().to_str().expect("path must be UTF-8"),
            "--json",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let change_json = parse_json_output(&change_output);
    let change_id = change_json["data"]["canonical_change"]["change_id"]
        .as_str()
        .expect("change_id must be present");

    let submit_output = ail()
        .args([
            "remote",
            "submit",
            change_id,
            "--signer",
            "local-dev",
            "--json",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&submit_output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["request"], "SubmitChangeSet");
    assert_eq!(v["data"]["transport"], "in_process");
    assert_eq!(v["data"]["key_source"], "ephemeral_in_process");
    assert_eq!(v["data"]["signer"]["key_ref"], "local-dev");
    assert_eq!(v["data"]["outcome"]["status"], "Applied");
    assert!(
        v["data"]["note"]
            .as_str()
            .expect("note must be a string")
            .contains("no network transport"),
        "remote submit must be honest about local transport; got: {v}"
    );
}

#[test]
fn remote_submit_uses_non_zero_current_snapshot_id() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");

    ail().arg("init").current_dir(dir.path()).assert().success();

    ail()
        .args([
            "change",
            "--file",
            sample_acl_path().to_str().expect("path must be UTF-8"),
            "--apply",
            "--json",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    let base_one_acl = dir.path().join("base-one.acl");
    std::fs::write(
        &base_one_acl,
        r#"change remote_nonzero
author test-author
description remote submit after snapshot one
base 1
end
"#,
    )
    .expect("base-one ACL must be written");

    let change_output = ail()
        .args([
            "change",
            "--file",
            base_one_acl.to_str().expect("path must be UTF-8"),
            "--json",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let change_json = parse_json_output(&change_output);
    let change_id = change_json["data"]["canonical_change"]["change_id"]
        .as_str()
        .expect("change_id must be present");

    let submit_output = ail()
        .args([
            "remote",
            "submit",
            change_id,
            "--signer",
            "local-dev",
            "--json",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&submit_output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["transport"], "in_process");
    assert_eq!(v["data"]["key_source"], "ephemeral_in_process");
    assert_eq!(v["data"]["outcome"]["status"], "Applied");
    assert_eq!(v["data"]["outcome"]["applied_snapshot_id"], 2);
}

#[test]
fn remote_submit_unknown_change_id_fails() {
    let change_id = "ab".repeat(32);

    ail()
        .args([
            "remote",
            "submit",
            &change_id,
            "--signer",
            "local-dev",
            "--json",
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn remote_push_help_mentions_root() {
    ail()
        .args(["remote", "push", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--root"));
}

#[test]
fn remote_push_pull_json_use_local_file_bundle_store() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");

    ail().arg("init").current_dir(dir.path()).assert().success();
    let status_output = ail()
        .args(["status", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let status_json = parse_json_output(&status_output);
    let root = status_json["data"]["graph_root_hash"]
        .as_str()
        .expect("graph_root_hash must be present");

    let push_output = ail()
        .args(["remote", "push", "--root", root, "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let push_json = parse_json_output(&push_output);
    assert_eq!(push_json["status"], "ok");
    assert_eq!(push_json["data"]["request"], "PushBundle");
    assert_eq!(push_json["data"]["root"], root);
    assert_eq!(
        push_json["data"]["transport"],
        "local_file_bundle_store+in_process"
    );
    assert_eq!(push_json["data"]["bundle_scope"], "single_root_object");
    assert_eq!(push_json["data"]["object_count"], 1);
    assert!(
        push_json["data"]["note"]
            .as_str()
            .expect("note must be a string")
            .contains("no network transport"),
        "remote push must be honest about local transport; got: {push_json}"
    );

    let bundle_path = dir
        .path()
        .join(".ail")
        .join("remote")
        .join("bundles")
        .join(format!("{root}.cbor"));
    assert!(
        bundle_path.exists(),
        "push must persist a local bundle file"
    );

    let pull_output = ail()
        .args(["remote", "pull", root, "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let pull_json = parse_json_output(&pull_output);
    assert_eq!(pull_json["status"], "ok");
    assert_eq!(pull_json["data"]["request"], "PullBundle");
    assert_eq!(pull_json["data"]["root"], root);
    assert_eq!(pull_json["data"]["object_count"], 1);
    assert_eq!(pull_json["data"]["bundle_scope"], "single_root_object");
    assert_eq!(
        pull_json["data"]["transport"],
        "local_file_bundle_store+in_process"
    );
}

#[test]
fn remote_push_pull_snapshot_envelope_json_report_dependency_scope() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");

    ail().arg("init").current_dir(dir.path()).assert().success();

    let graph_id = {
        let graph_bytes = b"snapshot envelope graph root".to_vec();
        let graph_id = ObjectId::from_bytes(&graph_bytes);
        write_raw_object(dir.path(), graph_bytes);
        graph_id
    };
    let snapshot = SnapshotEnvelope {
        id: ObjectId::from_bytes(b"cli snapshot envelope"),
        graph_root_hash: graph_id,
        ..Default::default()
    };
    let snapshot_bytes = CborCodec.encode(&snapshot).expect("snapshot must encode");
    let snapshot_root = write_raw_object(dir.path(), snapshot_bytes);

    let push_output = ail()
        .args(["remote", "push", "--root", &snapshot_root, "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let push_json = parse_json_output(&push_output);
    assert_eq!(push_json["status"], "ok");
    assert_eq!(
        push_json["data"]["bundle_scope"],
        "root_with_snapshot_envelope_dependencies"
    );
    assert_eq!(push_json["data"]["object_count"], 2);

    let pull_output = ail()
        .args(["remote", "pull", &snapshot_root, "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let pull_json = parse_json_output(&pull_output);
    assert_eq!(pull_json["status"], "ok");
    assert_eq!(
        pull_json["data"]["bundle_scope"],
        "root_with_snapshot_envelope_dependencies"
    );
    assert_eq!(pull_json["data"]["object_count"], 2);
}

#[test]
fn remote_pull_missing_bundle_fails() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let root = "cd".repeat(32);

    ail().arg("init").current_dir(dir.path()).assert().success();
    ail()
        .args(["remote", "pull", &root, "--json"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("remote bundle not found"));
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
    let checks = v["data"]["checks"]
        .as_array()
        .expect("checks must be array");
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
    for cmd in &[
        "rollback", "rebase", "merge", "refactor", "approve", "reject", "policy", "package",
        "doctor",
    ] {
        assert!(
            err_text.contains(cmd),
            "error message must list '{cmd}'; got:\n{err_text}"
        );
    }
}

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

// ── G31 R2: change with text input ───────────────────────────────────────

/// SC-CH1: change with free-text description creates draft ChangeSet.
#[test]
fn change_text_input_creates_draft() {
    let output = ail()
        .args(["change", "add pure cart_total function", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(
        v["data"]["status"], "draft",
        "change must be draft; got: {v}"
    );
    assert!(
        v["data"]["canonical_change"]["change_id"].is_string(),
        "canonical_change.change_id must be string; got: {v}"
    );
}

/// SC-CH2: change output includes structural_diff preview.
#[test]
fn change_output_includes_structural_diff() {
    let path = sample_acl_path();
    let output = ail()
        .args(["change", "--file", path.to_str().expect("path"), "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    let diff = &v["data"]["structural_diff"];
    assert!(
        diff.is_object(),
        "structural_diff must be an object; got: {v}"
    );
    assert!(
        diff["creates"].is_number(),
        "structural_diff.creates must be a number; got: {diff}"
    );
    assert!(
        diff["modifies"].is_number(),
        "structural_diff.modifies must be a number; got: {diff}"
    );
    assert!(
        diff["deletes"].is_number(),
        "structural_diff.deletes must be a number; got: {diff}"
    );
}

/// SC-CH3: change output includes submitted/parsed/canonical outputs.
#[test]
fn change_output_includes_submitted_parsed_canonical() {
    let path = sample_acl_path();
    let output = ail()
        .args(["change", "--file", path.to_str().expect("path"), "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert!(
        v["data"]["submitted_change"].is_object(),
        "submitted_change must be object; got: {v}"
    );
    assert!(
        v["data"]["parsed_change"].is_object(),
        "parsed_change must be object; got: {v}"
    );
    assert!(
        v["data"]["canonical_change"].is_object(),
        "canonical_change must be object; got: {v}"
    );
}

// ── G31 R2: verify with --profile ─────────────────────────────────────────

/// SC-VER1: verify with --profile dev includes policy_report and approval_requirements.
#[test]
fn verify_profile_dev_has_policy_and_approval() {
    let change_id = compute_sample_change_id();
    let output = ail()
        .args(["verify", &change_id, "--profile", "dev", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["policy_report"].is_object(),
        "policy_report must be object; got: {v}"
    );
    assert!(
        v["data"]["approval_requirements"].is_object(),
        "approval_requirements must be object; got: {v}"
    );
    assert!(
        v["data"]["diagnostics"].is_array(),
        "diagnostics must be array; got: {v}"
    );
    assert!(
        v["data"]["proof_obligations"].is_array(),
        "proof_obligations must be array; got: {v}"
    );
}

/// SC-VER2: verify with --profile prod has approval_requirements.required=true.
#[test]
fn verify_profile_prod_requires_approval() {
    let change_id = compute_sample_change_id();
    let output = ail()
        .args(["verify", &change_id, "--profile", "prod", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(
        v["data"]["approval_requirements"]["required"], true,
        "prod profile must require approval; got: {v}"
    );
    assert_eq!(
        v["data"]["policy_report"]["status"], "approval_required",
        "prod verify JSON must not report policy as ok while approval is required; got: {v}"
    );
    assert_eq!(
        v["data"]["policy_report"]["blocks_apply"], true,
        "prod verify JSON must make approval-required blocking state machine-readable; got: {v}"
    );
    assert_eq!(
        v["data"]["policy_report"]["policy_ok"], false,
        "prod verify JSON must not imply prod is OK before approval; got: {v}"
    );
}

#[test]
fn apply_prod_json_with_yes_marks_operator_confirmation_not_persisted_approval() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let change_id = create_sample_change(dir.path());
    let output = ail()
        .args(["apply", &change_id, "--policy", "prod", "--yes", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(
        v["data"]["pre_apply_gate"]["approval_status"]["required"],
        true
    );
    assert_eq!(
        v["data"]["pre_apply_gate"]["approval_status"]["operator_confirmed"],
        true
    );
    assert_eq!(
        v["data"]["pre_apply_gate"]["approval_status"]["persisted_approval"],
        false
    );
    assert_eq!(
        v["data"]["pre_apply_gate"]["approval_status"]["satisfied_for_this_apply"],
        true
    );
    assert_eq!(
        v["data"]["pre_apply_gate"]["policy_status"]["status"],
        "operator_confirmed"
    );
    assert_eq!(
        v["data"]["pre_apply_gate"]["policy_status"]["approval_source"],
        "operator_confirmation"
    );
    assert_eq!(
        v["data"]["pre_apply_gate"]["policy_status"]["blocks_apply"],
        false
    );
}

// ── G31 R2: apply pre-apply gate ──────────────────────────────────────────

/// SC-APL1: apply --json includes pre_apply_gate with all required fields.
#[test]
fn apply_json_has_pre_apply_gate() {
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
    assert_eq!(v["status"], "ok");
    let gate = &v["data"]["pre_apply_gate"];
    assert!(gate.is_object(), "pre_apply_gate must be object; got: {v}");
    assert!(
        gate["canonical_change_hash"].is_string(),
        "gate.canonical_change_hash must be string; got: {gate}"
    );
    assert!(
        gate["structural_diff"].is_object(),
        "gate.structural_diff must be object; got: {gate}"
    );
    assert!(
        gate["verification_report_status"].is_string(),
        "gate.verification_report_status must be string; got: {gate}"
    );
    assert!(
        gate["policy_status"].is_object(),
        "gate.policy_status must be object; got: {gate}"
    );
    assert!(
        gate["approval_status"].is_object(),
        "gate.approval_status must be object; got: {gate}"
    );
    assert!(
        gate["target_snapshot"].is_string(),
        "gate.target_snapshot must be string; got: {gate}"
    );
}

// ── G31 R2: compile --target ──────────────────────────────────────────────

/// SC-CMP1: compile with --target wasm succeeds.
#[test]
fn compile_with_wasm_target_exits_zero() {
    ail()
        .args(["compile", "--target", "wasm", "--profile", "dev"])
        .assert()
        .success()
        .stdout(predicate::str::contains("wasm").or(predicate::str::contains("profile")));
}

/// SC-CMP2: compile --json includes capabilities_manifest, artifact_manifest, compiler_report.
#[test]
fn compile_json_has_manifests_and_report() {
    let output = ail()
        .args(["compile", "--target", "wasm", "--profile", "dev", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["capabilities_manifest"].is_object(),
        "capabilities_manifest must be object; got: {v}"
    );
    assert!(
        v["data"]["artifact_manifest"].is_object(),
        "artifact_manifest must be object; got: {v}"
    );
    assert!(
        v["data"]["compiler_report"].is_object(),
        "compiler_report must be object; got: {v}"
    );
    assert!(
        v["data"]["semantic_source_map"].is_object(),
        "semantic_source_map must be object; got: {v}"
    );
    assert_eq!(v["data"]["artifact_manifest"]["profile"], "dev");
    assert!(
        v["data"]["artifact_manifest"]["capabilities_manifest_hash"].is_array(),
        "artifact_manifest must come from backend sidecar with capabilities_manifest_hash; got: {v}"
    );
    assert!(
        v["data"]["semantic_source_map"]["entries"].is_array(),
        "semantic_source_map must come from backend sidecar entries; got: {v}"
    );
}

/// SC-CMP3: compile with --target native succeeds.
#[test]
fn compile_with_native_target_exits_zero() {
    ail()
        .args(["compile", "--target", "native", "--profile", "prod"])
        .assert()
        .success();
}

// ── G31 R2: run with module and replay ───────────────────────────────────

/// SC-RUN1: run with module argument succeeds.
#[test]
fn run_with_module_exits_zero() {
    ail()
        .args(["run", "--profile", "dev", "module.checkout"])
        .assert()
        .success()
        .stdout(predicate::str::contains("PreflightPassed").or(predicate::str::contains("module")));
}

/// SC-RUN2: run --json includes runtime_report, audit_log, capability_call_summary, runtime_check_results.
#[test]
fn run_json_has_full_runtime_report() {
    let output = ail()
        .args(["run", "--profile", "dev", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["runtime_report"].is_object(),
        "runtime_report must be object; got: {v}"
    );
    assert!(
        v["data"]["audit_log"].is_object(),
        "audit_log must be object; got: {v}"
    );
    assert!(
        v["data"]["capability_call_summary"].is_array(),
        "capability_call_summary must be array; got: {v}"
    );
    assert!(
        v["data"]["runtime_check_results"].is_object(),
        "runtime_check_results must be object; got: {v}"
    );
}

/// SC-RUN3: run with --replay trace_id includes replay info in JSON.
#[test]
fn run_with_replay_includes_replay_info() {
    let output = ail()
        .args([
            "run",
            "--profile",
            "test",
            "--replay",
            "trace_123",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        !v["data"]["replay"].is_null(),
        "replay info must be present for --replay; got: {v}"
    );
}

// ── G31 R2: init baseline state ───────────────────────────────────────────

/// SC-INIT1: init --json includes branch, policy, runtime_profiles, stdlib_baseline.
#[test]
fn init_json_has_baseline_state() {
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
    assert_eq!(v["data"]["branch"], "main", "branch must be main; got: {v}");
    assert!(
        v["data"]["policy"].is_string(),
        "policy must be string; got: {v}"
    );
    assert!(
        v["data"]["runtime_profiles"].is_array(),
        "runtime_profiles must be array; got: {v}"
    );
    assert!(
        v["data"]["stdlib_baseline"].is_string(),
        "stdlib_baseline must be string; got: {v}"
    );
    assert!(
        v["data"]["package_lock"].is_string(),
        "package_lock must be string; got: {v}"
    );
    assert!(
        v["data"]["context_indexes"].is_string(),
        "context_indexes must be string; got: {v}"
    );
}

// ── G31 R2: status with all fields ───────────────────────────────────────

/// SC-STAT1: status --json includes verification_state, stale_indexes, runtime_profile_status, package_advisories.
#[test]
fn status_json_has_all_required_fields() {
    let output = ail()
        .args(["status", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["verification_state"].is_string(),
        "verification_state must be string; got: {v}"
    );
    assert!(
        v["data"]["stale_indexes"].is_boolean(),
        "stale_indexes must be boolean; got: {v}"
    );
    assert!(
        v["data"]["runtime_profile_status"].is_string(),
        "runtime_profile_status must be string; got: {v}"
    );
    assert!(
        v["data"]["package_advisories"].is_number(),
        "package_advisories must be number; got: {v}"
    );
}

// ── G31 R2: inspect all types ─────────────────────────────────────────────

/// SC-INS1: inspect node returns edges/effects/capabilities/contracts.
#[test]
fn inspect_node_returns_node_metadata() {
    let output = ail()
        .args(["inspect", "node", "fn.checkout", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["type"], "node");
    assert!(
        v["data"]["edges"].is_array(),
        "edges must be array; got: {v}"
    );
    assert!(
        v["data"]["effects"].is_array(),
        "effects must be array; got: {v}"
    );
    assert!(
        v["data"]["capabilities"].is_array(),
        "capabilities must be array; got: {v}"
    );
    assert!(
        v["data"]["contracts"].is_array(),
        "contracts must be array; got: {v}"
    );
}

/// SC-INS2: inspect report returns status/entries/diagnostics.
#[test]
fn inspect_report_returns_report_metadata() {
    let output = ail()
        .args(["inspect", "report", "ver_123", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["type"], "report");
    assert!(
        v["data"]["entries"].is_array(),
        "entries must be array; got: {v}"
    );
    assert!(
        v["data"]["diagnostics"].is_array(),
        "diagnostics must be array; got: {v}"
    );
}

/// SC-INS3: inspect artifact returns name/hash/profile.
#[test]
fn inspect_artifact_returns_artifact_metadata() {
    let output = ail()
        .args(["inspect", "artifact", "checkout.wasm", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["type"], "artifact");
    assert!(
        v["data"]["name"].is_string(),
        "name must be string; got: {v}"
    );
}

/// SC-INS4: inspect capability returns provider/granted/assumptions.
#[test]
fn inspect_capability_returns_capability_metadata() {
    let output = ail()
        .args([
            "inspect",
            "capability",
            "payment.charge:PaymentProvider",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["type"], "capability");
    assert!(
        v["data"]["provider"].is_string(),
        "provider must be string; got: {v}"
    );
    assert!(
        v["data"]["granted"].is_boolean(),
        "granted must be boolean; got: {v}"
    );
    assert!(
        v["data"]["assumptions"].is_array(),
        "assumptions must be array; got: {v}"
    );
}

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

// ── G31 R2: approve full model ────────────────────────────────────────────

/// SC-APR1: approve --json includes record_id, immutable flag, expires_on_canonical_diff_change.
#[test]
fn approve_json_has_full_immutable_record() {
    let change_id = "aa".repeat(32);
    let output = ail()
        .args([
            "approve",
            &change_id,
            "--for",
            "public_api_changed",
            "--role",
            "security",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["approved"], true);
    assert!(
        v["data"]["record_id"].is_string(),
        "record_id must be string; got: {v}"
    );
    assert_eq!(
        v["data"]["immutable"], true,
        "approval must be immutable; got: {v}"
    );
    assert_eq!(
        v["data"]["expires_on_canonical_diff_change"], true,
        "approval must expire on diff change; got: {v}"
    );
    assert_eq!(
        v["data"]["role"], "security",
        "role must be security; got: {v}"
    );
}

// ── G31 R2: reject full immutable model ──────────────────────────────────

/// SC-REJ1: reject --json includes record_id and immutable flag.
#[test]
fn reject_json_has_full_immutable_record() {
    let change_id = "aa".repeat(32);
    let output = ail()
        .args([
            "reject",
            &change_id,
            "--reason",
            "capability too broad",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["approved"], false);
    assert!(
        v["data"]["record_id"].is_string(),
        "record_id must be string; got: {v}"
    );
    assert_eq!(
        v["data"]["immutable"], true,
        "rejection must be immutable; got: {v}"
    );
}

// ── G31 R2: policy real behavior ─────────────────────────────────────────

/// SC-POL1: policy check --json includes violations array and rules_checked.
#[test]
fn policy_check_json_has_violations_and_rules_checked() {
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
        v["data"]["violations"].is_array(),
        "violations must be array; got: {v}"
    );
    assert!(
        v["data"]["rules_checked"].is_array(),
        "rules_checked must be array; got: {v}"
    );
}

/// SC-POL2: policy explain --json includes enforced_on field.
#[test]
fn policy_explain_json_has_enforced_on() {
    let output = ail()
        .args(["policy", "explain", "no_unverified_public_api", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["enforced_on"].is_array(),
        "enforced_on must be array; got: {v}"
    );
}

/// SC-POL3: policy set --json has record_type field.
#[test]
fn policy_set_json_has_record_type() {
    let output = ail()
        .args(["policy", "set", "max_new_capabilities=2", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["record_type"].is_string(),
        "record_type must be string; got: {v}"
    );
}

// ── G31 R2: package full metadata ────────────────────────────────────────

/// SC-PKG1: package add --json includes trust, verification_report, capabilities,
///          assumptions, unsafe_surface, advisories, capabilities_granted=false.
#[test]
fn package_add_json_has_full_metadata() {
    let output = ail()
        .args(["package", "add", "payments.stripe@1.2", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["trust"].is_string(),
        "trust must be string; got: {v}"
    );
    assert!(
        v["data"]["verification_report"].is_string(),
        "verification_report must be string; got: {v}"
    );
    assert!(
        v["data"]["capabilities"].is_array(),
        "capabilities must be array; got: {v}"
    );
    assert!(
        v["data"]["assumptions"].is_array(),
        "assumptions must be array; got: {v}"
    );
    assert!(
        v["data"]["unsafe_surface"].is_array(),
        "unsafe_surface must be array; got: {v}"
    );
    assert!(
        v["data"]["advisories"].is_array(),
        "advisories must be array; got: {v}"
    );
    assert_eq!(
        v["data"]["capabilities_granted"], false,
        "package install must not grant capabilities; got: {v}"
    );
}

/// SC-PKG2: package audit --json includes packages_checked and assumptions_valid.
#[test]
fn package_audit_json_has_full_audit_fields() {
    let output = ail()
        .args(["package", "audit", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["packages_checked"].is_number(),
        "packages_checked must be number; got: {v}"
    );
    assert!(
        v["data"]["assumptions_valid"].is_boolean(),
        "assumptions_valid must be boolean; got: {v}"
    );
    assert!(
        v["data"]["unsafe_surface"].is_array(),
        "unsafe_surface must be array; got: {v}"
    );
}

// ── G31 R2: doctor real checks ────────────────────────────────────────────

/// SC-DOC1: doctor --json has overall field and all 7 required check names.
#[test]
fn doctor_json_has_overall_and_all_check_names() {
    let output = ail()
        .args(["doctor", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["overall"].is_string(),
        "overall must be string; got: {v}"
    );

    let checks = v["data"]["checks"]
        .as_array()
        .expect("checks must be array");
    let check_names: Vec<&str> = checks.iter().filter_map(|c| c["name"].as_str()).collect();

    for required_name in &[
        "graph_integrity",
        "index_freshness",
        "schema_compatibility",
        "artifact_hash_consistency",
        "runtime_profile_validity",
        "package_advisories",
        "assumption_expirations",
    ] {
        assert!(
            check_names.contains(required_name),
            "doctor must include check '{required_name}'; got: {check_names:?}"
        );
    }
}

// ── T7d: LLM agent loop E2E test ─────────────────────────────────────────

/// Spec scenario LL-1a: full LLM agent loop succeeds.
///
/// Exercises the 6-step protocol end-to-end using a file-backed store so that
/// cmd_change persists the CanonicalChangeSet and cmd_verify can load it.
///
/// Steps (matching tooling.md LLM protocol):
///  1. `ail context fn.checkout --json`  → schema_version = "1"
///  2. `ail impact type.CartItem.price --json` → schema_version = "1"
///  3. `ail change --file sample.acl --json` → change_id extracted
///  4. `ail verify <change_id> --profile dev --json` → policy_report present
///  5. `ail diff --semantic change.add_checkout --json` → exits 0
///  6. `ail apply <change_id> --json` → new_snapshot_id present
///
/// All steps assert schema_version == "1".
#[test]
fn llm_agent_loop_e2e_with_schema_version() {
    use assert_fs::TempDir;

    let dir = TempDir::new().expect("temp dir");

    // Initialize the file store so changeset payloads are persisted across calls.
    ail().arg("init").current_dir(dir.path()).assert().success();

    let path = sample_acl_path();

    // ── Step 1: context (no target — lists snapshots after init) ─────────
    let ctx_output = ail()
        .args(["context", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let ctx_json = parse_json_output(&ctx_output);
    assert_eq!(ctx_json["status"], "ok", "step 1 (context) must succeed");
    assert_eq!(
        ctx_json["data"]["schema_version"], "1",
        "step 1 (context): schema_version must be \"1\"; got: {ctx_json}"
    );

    // ── Step 2: impact ───────────────────────────────────────────────────
    let impact_output = ail()
        .args(["impact", "type.CartItem.price", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let impact_json = parse_json_output(&impact_output);
    assert_eq!(impact_json["status"], "ok", "step 2 (impact) must succeed");
    assert_eq!(
        impact_json["data"]["schema_version"], "1",
        "step 2 (impact): schema_version must be \"1\"; got: {impact_json}"
    );

    // ── Step 3: change ───────────────────────────────────────────────────
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
    assert_eq!(change_json["status"], "ok", "step 3 (change) must succeed");
    assert_eq!(
        change_json["data"]["schema_version"], "1",
        "step 3 (change): schema_version must be \"1\"; got: {change_json}"
    );
    let change_id = change_json["data"]["canonical_change"]["change_id"]
        .as_str()
        .expect("canonical_change.change_id must be a string")
        .to_string();
    assert_eq!(change_id.len(), 64, "change-id must be 64 hex chars");

    // ── Step 4: verify with persisted changeset ──────────────────────────
    let verify_output = ail()
        .args(["verify", &change_id, "--profile", "dev", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let verify_json = parse_json_output(&verify_output);
    assert_eq!(verify_json["status"], "ok", "step 4 (verify) must succeed");
    assert_eq!(
        verify_json["data"]["schema_version"], "1",
        "step 4 (verify): schema_version must be \"1\"; got: {verify_json}"
    );
    assert!(
        verify_json["data"]["policy_report"].is_object(),
        "step 4 (verify): policy_report must be present; got: {verify_json}"
    );

    // ── Step 5: diff ─────────────────────────────────────────────────────
    let diff_output = ail()
        .args(["diff", "--semantic", "change.add_checkout", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let diff_json = parse_json_output(&diff_output);
    assert_eq!(diff_json["status"], "ok", "step 5 (diff) must succeed");
    assert_eq!(
        diff_json["data"]["schema_version"], "1",
        "step 5 (diff): schema_version must be \"1\"; got: {diff_json}"
    );

    // ── Step 6: apply ────────────────────────────────────────────────────
    let apply_output = ail()
        .args(["apply", &change_id, "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let apply_json = parse_json_output(&apply_output);
    assert_eq!(apply_json["status"], "ok", "step 6 (apply) must succeed");
    assert_eq!(
        apply_json["data"]["schema_version"], "1",
        "step 6 (apply): schema_version must be \"1\"; got: {apply_json}"
    );
    assert!(
        apply_json["data"]["new_snapshot_id"].is_string(),
        "step 6 (apply): new_snapshot_id must be present; got: {apply_json}"
    );
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

/// Run `ail change --file sample.acl --json` in an initialized project and
/// return the persisted ChangeSet id that `ail apply` can load later.
fn create_sample_change(project_dir: &std::path::Path) -> String {
    let path = sample_acl_path();
    let output = ail()
        .args([
            "change",
            "--file",
            path.to_str().expect("path must be UTF-8"),
            "--json",
        ])
        .current_dir(project_dir)
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    v["data"]["change_id"]
        .as_str()
        .or_else(|| v["data"]["canonical_change"]["change_id"].as_str())
        .expect("change output must include a change_id")
        .to_string()
}
