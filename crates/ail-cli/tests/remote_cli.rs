// ── ail-cli remote integration tests ────────────────────────────────────────
//
// Covers remote submit/push/pull spec scenarios.

mod common;

use ail_storage::SnapshotEnvelope;
use ail_storage::codec::{CborCodec, ContentCodec};
use ail_storage::object::ObjectId;
use common::{ail, parse_json_output, sample_acl_path};
use predicates::prelude::*;
use std::fs;

fn write_raw_object(project_dir: &std::path::Path, bytes: Vec<u8>) -> String {
    let id = ObjectId::from_bytes(&bytes);
    let objects_dir = project_dir.join(".ail").join("store").join("objects");
    fs::create_dir_all(&objects_dir).expect("object directory must be created");
    fs::write(objects_dir.join(id.to_hex()), bytes).expect("object must be written");
    id.to_hex()
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
