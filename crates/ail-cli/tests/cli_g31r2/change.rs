use super::common::{ail, parse_json_output, sample_acl_path};

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
