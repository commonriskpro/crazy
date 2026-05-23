// ── ail-change: ACL versioning / migrators (Gap 1) ──────────────────────────
//
// Strict TDD — RED phase.
//
// Scenarios:
//   1. v1.0 changeset passes through unchanged (current version, no migration).
//   2. v0.9 changeset gets migrated: deprecated verb renamed to current form.
//   3. Unknown version returns MigrateError::UnknownVersion.

use ail_change::{
    acl_migrator::{CURRENT_ACL_VERSION, MigrateError},
    canonical::try_canonicalize_parsed,
    parser::parse_changeset,
};

// ── Scenario 1 ──────────────────────────────────────────────────────────────
// GIVEN a changeset with acl_version == CURRENT_ACL_VERSION
// WHEN try_canonicalize_parsed is called
// THEN the result is Ok and acl_version == CURRENT_ACL_VERSION (no migration)
#[test]
fn v1_0_changeset_passes_through_unchanged() {
    let src = format!("change x acl={CURRENT_ACL_VERSION} base=0\nauthor A\nend\n");
    let parsed = parse_changeset(&src).expect("must parse");
    let canonical = try_canonicalize_parsed(parsed).expect("current version must succeed");
    assert_eq!(canonical.acl_version, CURRENT_ACL_VERSION);
}

// ── Scenario 2 ──────────────────────────────────────────────────────────────
// GIVEN a changeset with acl_version "0.9" using the deprecated verb "create_fn"
// WHEN try_canonicalize_parsed is called
// THEN Ok is returned, acl_version == CURRENT, and verb is normalized to "create_function"
#[test]
fn v0_9_changeset_gets_migrated() {
    let src = "change x acl=0.9 base=0\nauthor A\nop create_fn id=fn.foo\nend\n";
    let parsed = parse_changeset(src).expect("must parse");
    let canonical = try_canonicalize_parsed(parsed).expect("v0.9 must succeed after migration");
    assert_eq!(
        canonical.acl_version, CURRENT_ACL_VERSION,
        "acl_version must be updated to CURRENT after migration"
    );
    assert!(
        canonical.ops.iter().any(|op| op.verb == "create_function"),
        "deprecated 'create_fn' verb must be migrated to 'create_function'"
    );
}

// TRIANGULATE: deprecated "add_field" verb is also normalized in v0.9 → v1.0
#[test]
fn v0_9_add_field_is_migrated_to_add_param() {
    let src =
        "change x acl=0.9 base=0\nauthor A\nop add_field target=fn.foo name=x type=Int\nend\n";
    let parsed = parse_changeset(src).expect("must parse");
    let canonical = try_canonicalize_parsed(parsed).expect("v0.9 must succeed after migration");
    assert!(
        canonical.ops.iter().any(|op| op.verb == "add_param"),
        "deprecated 'add_field' verb must be migrated to 'add_param'"
    );
}

// ── Scenario 3 ──────────────────────────────────────────────────────────────
// GIVEN a changeset with an unknown acl_version (no migrator registered)
// WHEN try_canonicalize_parsed is called
// THEN Err(MigrateError::UnknownVersion) is returned
#[test]
fn unknown_version_returns_migrate_error() {
    let src = "change x acl=99.0 base=0\nauthor A\nend\n";
    let parsed = parse_changeset(src).expect("must parse");
    let result = try_canonicalize_parsed(parsed);
    assert!(
        matches!(result, Err(MigrateError::UnknownVersion(ref v)) if v == "99.0"),
        "expected UnknownVersion(\"99.0\"), got {result:?}"
    );
}

// TRIANGULATE: a "future" version also returns unknown error
#[test]
fn future_version_returns_migrate_error() {
    let src = "change x acl=2.0 base=0\nauthor A\nend\n";
    let parsed = parse_changeset(src).expect("must parse");
    let result = try_canonicalize_parsed(parsed);
    assert!(
        matches!(result, Err(MigrateError::UnknownVersion(_))),
        "future version with no downgrade path must return UnknownVersion"
    );
}
