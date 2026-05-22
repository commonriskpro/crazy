// ── ail-change: acl_version parsing tests (G22 Phase C) ────────────────────
//
// Strict TDD — RED phase for acl_version support.
// Verifies that the parser extracts acl_version from:
//   1. `change x acl=1.0 base=0` (inline attrs)
//   2. `language acl/1.0` directive
//   3. Defaults to "1.0" when absent

use ail_change::canonical::canonicalize_parsed;
use ail_change::parser::parse_changeset;

// ── Scenario: acl version from inline change line attr ───────────────────
// GIVEN `change x acl=1.0 base=0`
// WHEN parsed
// THEN acl_version == "1.0"
#[test]
fn parse_acl_version_from_change_line_attr() {
    let src = "change x acl=1.0 base=0\nauthor Alice\nend\n";
    let result = parse_changeset(src).expect("must parse");
    assert_eq!(result.acl_version, "1.0");
}

// ── Scenario: base from inline change line attr ───────────────────────────
// GIVEN `change x acl=1.0 base=42`
// WHEN parsed
// THEN base_snapshot_id == 42
#[test]
fn parse_base_from_change_line_attr() {
    let src = "change x acl=1.0 base=42\nauthor Bob\nend\n";
    let result = parse_changeset(src).expect("must parse");
    assert_eq!(result.changeset.base_snapshot_id.0, 42);
}

// ── Scenario: language directive sets acl_version ────────────────────────
// GIVEN `language acl/1.0` inside metadata section
// WHEN parsed
// THEN acl_version == "1.0"
#[test]
fn parse_acl_version_from_language_directive_in_metadata() {
    let src = "\
change x
author Alice
base 0
metadata
  language acl/1.0
end
end
";
    let result = parse_changeset(src).expect("must parse");
    assert_eq!(result.acl_version, "1.0");
}

// ── Scenario: language directive at top level ─────────────────────────────
// GIVEN `language acl/1.0` at top level (outside metadata block)
// WHEN parsed
// THEN acl_version == "1.0"
#[test]
fn parse_acl_version_from_language_directive_at_toplevel() {
    let src = "change x\nauthor C\nbase 0\nlanguage acl/1.0\nend\n";
    let result = parse_changeset(src).expect("must parse");
    assert_eq!(result.acl_version, "1.0");
}

// ── Scenario: missing acl declaration defaults to "1.0" ───────────────────
// GIVEN a changeset with no acl declaration
// WHEN parsed
// THEN acl_version defaults to "1.0"
#[test]
fn parse_missing_acl_version_defaults_to_1_0() {
    let src = "change x\nauthor D\nbase 0\nend\n";
    let result = parse_changeset(src).expect("must parse");
    assert_eq!(result.acl_version, "1.0");
}

// ── Scenario: CanonicalChangeSet carries acl_version from parsed ──────────
// GIVEN a ParsedChangeSet with acl_version "1.0"
// WHEN canonicalize_parsed is called
// THEN canonical.acl_version == "1.0"
#[test]
fn canonicalize_parsed_propagates_acl_version() {
    let src = "change x acl=1.0 base=0\nauthor E\nend\n";
    let parsed = parse_changeset(src).expect("must parse");
    let canonical = canonicalize_parsed(parsed);
    assert_eq!(canonical.acl_version, "1.0");
}

// ── TRIANGULATE: inline acl attr takes effect even when base also inline ──
// Both attrs on the same change line.
#[test]
fn parse_both_acl_and_base_inline() {
    let src = "change x acl=1.0 base=7\nauthor F\nend\n";
    let result = parse_changeset(src).expect("must parse");
    assert_eq!(result.acl_version, "1.0");
    assert_eq!(result.changeset.base_snapshot_id.0, 7);
    // author still required
    assert_eq!(result.changeset.meta.author, "F");
}

// ── Scenario: full spec example with acl=1.0 base=snapshot ───────────────
// Proves the canonical example from the spec can be parsed.
#[test]
fn parse_spec_example_with_acl_and_base() {
    let src = "\
change add_cart_total acl=1.0 base=1
author agent
intent \"Add pure cart total calculation\"

requires
  assert_exists 1
  assert_exists 2
end

ops
  op create_function id=fn.cart_total
  op add_param target=fn.cart_total name=cart type=Cart
  op verify
end

end
";
    let result = parse_changeset(src).expect("spec example must parse");
    assert_eq!(result.acl_version, "1.0");
    assert_eq!(result.changeset.base_snapshot_id.0, 1);
    assert_eq!(
        result.changeset.meta.description,
        "Add pure cart total calculation"
    );
    assert_eq!(result.changeset.ops.len(), 3);
    assert_eq!(result.preconditions.len(), 2);
}
