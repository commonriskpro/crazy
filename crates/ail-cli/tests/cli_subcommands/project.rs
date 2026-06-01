// Mechanical split from cli_subcommands.rs. Keep behavior-only moves in this module.
use crate::common::ail;
use crate::common::parse_json_output;
use crate::common::sample_acl_path;
use predicates::prelude::*;

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
/// Spec scenario: new creates a project scaffold.
///   GIVEN an empty parent directory
///   WHEN `ail new myapp --json` runs
///   THEN myapp has .ail metadata, starter .ail source, starter ACL, and JSON scaffold fields
#[test]
fn new_creates_project_scaffold_with_starter_source_and_acl() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let output = ail()
        .args(["new", "myapp", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let project = dir.child("myapp");
    project.child(".ail/HEAD").assert(predicate::path::exists());
    project
        .child(".ail/store/objects")
        .assert(predicate::path::is_dir());
    project.child("main.ail").assert(predicate::path::exists());
    project.child("main.acl").assert(predicate::path::exists());

    let source = std::fs::read_to_string(project.path().join("main.ail"))
        .expect("starter source must be readable");
    assert!(source.contains("fn main() -> Int"));
    assert!(source.contains("test main_addition"));

    let acl = std::fs::read_to_string(project.path().join("main.acl"))
        .expect("starter ACL must be readable");
    assert!(acl.contains("op create_function id=fn.main"));
    assert!(acl.contains("op create_test id=test.main_addition"));

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["created"], true);
    assert_eq!(v["data"]["branch"], "main");
    assert!(v["data"]["genesis_snapshot_id"].is_string());
    assert!(v["data"]["starter_source"].is_string());
    assert!(v["data"]["starter_acl"].is_string());
}

/// Spec scenario: new keeps package-style names manifest-safe and starter ACL usable.
///   GIVEN a package-style project name with hyphen/dot punctuation
///   WHEN `ail new my-app.v1 --json` runs
///   THEN project.toml preserves the manifest name and starter ACL uses a valid change id
#[test]
fn new_sanitizes_starter_change_for_package_style_project_name() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let output = ail()
        .args(["new", "my-app.v1", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let project = dir.child("my-app.v1");
    let manifest = std::fs::read_to_string(project.path().join(".ail/project.toml"))
        .expect("project manifest must be readable");
    assert!(
        manifest.contains("name = \"my-app.v1\""),
        "project.toml must preserve the user-facing project name; got:\n{manifest}"
    );

    let acl = std::fs::read_to_string(project.path().join("main.acl"))
        .expect("starter ACL must be readable");
    assert!(
        acl.contains("change my_app_v1_hello"),
        "starter ACL must use an identifier-safe deterministic change id; got:\n{acl}"
    );

    ail()
        .args(["change", "--file", "main.acl", "--json"])
        .current_dir(project.path())
        .assert()
        .success();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["project_name"], "my-app.v1");
    assert_eq!(v["data"]["starter_change"], "my_app_v1_hello");
}

/// Spec scenario: new rejects unsafe project names before writing a scaffold.
///   GIVEN a project name that would produce unsafe TOML/source metadata
///   WHEN `ail new "bad name"` runs
///   THEN the command fails with a clear diagnostic and writes no project directory
#[test]
fn new_rejects_unsafe_project_name_with_clear_diagnostic() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail()
        .args(["new", "bad name", "--json"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid project name 'bad name'"))
        .stderr(predicate::str::contains("Use ASCII letters"));

    dir.child("bad name").assert(predicate::path::missing());
}

/// Spec scenario: new rejects path traversal without leaking local paths.
///   GIVEN a nested workspace directory
///   WHEN `ail new ../escape` runs
///   THEN the command fails with a stable redacted diagnostic and writes no sibling project
#[test]
fn new_rejects_path_traversal_with_redacted_diagnostic() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let workspace = dir.child("workspace");
    std::fs::create_dir_all(workspace.path()).expect("workspace dir");

    ail()
        .args(["new", "../escape", "--json"])
        .current_dir(workspace.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("project.path.traversal"))
        .stderr(predicate::str::contains("<redacted:project-path>"))
        .stderr(predicate::str::contains("../escape").not());

    dir.child("escape").assert(predicate::path::missing());
}

/// Spec scenario: status reports missing project config as a redacted workflow diagnostic.
///   GIVEN an uninitialized directory
///   WHEN `ail status --json` runs
///   THEN status still succeeds but includes a stable project diagnostic
#[test]
fn status_json_reports_missing_project_config_diagnostic() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let output = ail()
        .args(["status", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = std::str::from_utf8(&output.stdout).expect("stdout must be UTF-8");
    assert!(
        !stdout.contains(dir.path().to_string_lossy().as_ref()),
        "diagnostics must not leak absolute temp paths; got:\n{stdout}"
    );

    let v = parse_json_output(&output);
    let diagnostics = v["data"]["project_diagnostics"]["diagnostics"]
        .as_array()
        .expect("project diagnostics must be an array");

    assert_eq!(v["data"]["project_diagnostics"]["status"], "warning");
    assert_eq!(diagnostics[0]["code"], "project.config.missing");
    assert_eq!(diagnostics[0]["subject"], "project-config");
}

/// Spec scenario: status reports invalid layout and duplicate entries in stable order.
///   GIVEN a malformed .ail workspace with duplicate module/package config
///   WHEN `ail status --json` runs
///   THEN diagnostics are redacted and deterministically ordered
#[test]
fn status_json_reports_invalid_layout_and_duplicate_entries_in_order() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    std::fs::create_dir_all(dir.path().join(".ail")).expect(".ail dir");
    std::fs::write(
        dir.path().join(".ail/project.toml"),
        "package = \"pkg.alpha\"\nmodule = \"mod.core\"\npackages = [\"pkg.alpha\"]\nmodules = [\"mod.core\"]\n",
    )
    .expect("project config must be writable");

    let output = ail()
        .args(["status", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let v = parse_json_output(&output);
    let diagnostics = v["data"]["project_diagnostics"]["diagnostics"]
        .as_array()
        .expect("project diagnostics must be an array");
    let codes = diagnostics
        .iter()
        .map(|diagnostic| diagnostic["code"].as_str().expect("code must be string"))
        .collect::<Vec<_>>();

    assert_eq!(v["data"]["project_diagnostics"]["status"], "error");
    assert_eq!(
        codes,
        vec![
            "project.config.duplicate_module_entry",
            "project.config.duplicate_package_entry",
            "project.workspace.invalid_layout",
            "project.workspace.invalid_layout",
        ],
        "diagnostic ordering must be stable"
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
    assert_eq!(
        run_json["data"]["artifact_source"],
        "persisted_wasm_artifact"
    );
    assert_eq!(
        run_json["data"]["runtime_check_results"]["artifact_hash"]["source"],
        "persisted_wasm_artifact"
    );

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
/// Feature-F: inspect report --json returns real Checker-derived status.
///   GIVEN no stored reports (memory store fallback to default graph)
///   WHEN `ail inspect report ver_123 --json` runs
///   THEN exit 0; data.status is a real VerificationState (NOT "accepted")
///   NOTE: old stub returned "accepted" unconditionally — this FAILS with the stub.
#[test]
fn inspect_report_json_has_real_status_not_accepted() {
    let output = ail()
        .args(["--json", "inspect", "report", "ver_123"])
        .output()
        .expect("ail must run");
    assert!(
        output.status.success(),
        "inspect report must succeed; stderr: {}",
        std::str::from_utf8(&output.stderr).unwrap_or("(non-utf8)")
    );
    let v = parse_json_output(&output);
    let status = v["data"]["status"].as_str().unwrap_or("");
    assert_ne!(
        status, "accepted",
        "inspect report status must be a real VerificationState, not the stub 'accepted'; got: {v}"
    );
    assert!(
        !status.is_empty(),
        "inspect report status must not be empty; got: {v}"
    );
}
/// Feature-F: inspect artifact --json returns real non-null hash.
///   GIVEN a compilable default graph
///   WHEN `ail inspect artifact program.wasm --json` runs
///   THEN exit 0; data.hash is a non-null string (real WASM hash)
///   NOTE: old stub returned hash:null — this FAILS with the stub.
#[test]
fn inspect_artifact_json_has_non_null_hash() {
    let output = ail()
        .args(["--json", "inspect", "artifact", "program.wasm"])
        .output()
        .expect("ail must run");
    assert!(
        output.status.success(),
        "inspect artifact must succeed; stderr: {}",
        std::str::from_utf8(&output.stderr).unwrap_or("(non-utf8)")
    );
    let v = parse_json_output(&output);
    assert!(
        !v["data"]["hash"].is_null(),
        "inspect artifact hash must be non-null (real WASM hash); got: {v}"
    );
    let hash_str = v["data"]["hash"].as_str().unwrap_or("");
    assert_eq!(
        hash_str.len(),
        64,
        "inspect artifact hash must be a 64-char hex string; got: {hash_str}"
    );
    assert!(
        v["data"]["semantic_source_map"].is_object(),
        "inspect artifact must expose computed source-map metadata; got: {v}"
    );
    assert!(
        v["data"]["capabilities_manifest"]["entries"].is_array(),
        "inspect artifact capabilities_manifest must use entries schema; got: {v}"
    );
}
/// Feature-F: inspect capability exits 1 for unknown capability.
///   GIVEN a capability name not in any registered package
///   WHEN `ail inspect capability unknown.cap` runs
///   THEN exit 1 with NotFound
///   NOTE: old stub returned exit 0 unconditionally — this FAILS with the stub.
#[test]
fn inspect_capability_unknown_exits_one() {
    ail()
        .args(["inspect", "capability", "unknown.capability.xyz"])
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
        "rollback", "rebase", "merge", "refactor", "approve", "reject", "policy", "package", "fmt",
        "test", "new", "lsp", "doctor",
    ] {
        assert!(
            err_text.contains(cmd),
            "error message must list '{cmd}'; got:\n{err_text}"
        );
    }
}

// ── ail fmt integration tests ─────────────────────────────────────────────
