// ── ail-change: expect/approval/composition section tests (G22 Phase D) ────
//
// Strict TDD — tests for expect, approval, and composition sections in the
// ACL parser.

use ail_change::parser::parse_changeset;

// ═══════════════════════════════════════════════════════════════════════════
// Expect section
// ═══════════════════════════════════════════════════════════════════════════

// ── Scenario: expect section is parsed into ExpectClaims ──────────────────
// GIVEN `expect ... end` with two claim lines
// WHEN parsed
// THEN ParsedChangeSet.expect is Some with those 2 claims
#[test]
fn parse_expect_section_produces_expect_claims() {
    let src = "\
change x
author Alice
base 0
expect
  creates fn.checkout
  no_unsafe
end
end
";
    let result = parse_changeset(src).expect("must parse");
    let expect = result.expect.expect("expect must be Some");
    assert_eq!(expect.0.len(), 2);
    assert!(expect.0.contains(&"creates fn.checkout".to_string()));
    assert!(expect.0.contains(&"no_unsafe".to_string()));
}

// ── Scenario: expect section absent → None ────────────────────────────────
// GIVEN no expect section
// WHEN parsed
// THEN expect is None
#[test]
fn parse_no_expect_section_produces_none() {
    let src = "change x\nauthor B\nbase 0\nend\n";
    let result = parse_changeset(src).expect("must parse");
    assert!(result.expect.is_none());
}

// ── Scenario: multiple expect claims ──────────────────────────────────────
#[test]
fn parse_expect_section_with_many_claims() {
    let src = "\
change x
author Alice
base 0
expect
  creates fn.checkout
  modifies module.cart
  no_new_public_api_except fn.checkout
  no_unsafe
  no_unverified
end
end
";
    let result = parse_changeset(src).expect("must parse");
    let expect = result.expect.expect("expect must be Some");
    assert_eq!(expect.0.len(), 5);
}

// ═══════════════════════════════════════════════════════════════════════════
// Approval section
// ═══════════════════════════════════════════════════════════════════════════

// ── Scenario: approval section is parsed into ApprovalRequirements ────────
// GIVEN `approval ... end` with `require_if public_api_changed`
// WHEN parsed
// THEN ParsedChangeSet.approval is Some with that requirement
#[test]
fn parse_approval_section_produces_approval_requirements() {
    let src = "\
change x
author Alice
base 0
approval
  require_if public_api_changed
  require_if unsafe_added
end
end
";
    let result = parse_changeset(src).expect("must parse");
    let approval = result.approval.expect("approval must be Some");
    assert_eq!(approval.0.len(), 2);
    assert!(
        approval
            .0
            .contains(&"require_if public_api_changed".to_string())
    );
    assert!(approval.0.contains(&"require_if unsafe_added".to_string()));
}

// ── Scenario: approval section absent → None ──────────────────────────────
#[test]
fn parse_no_approval_section_produces_none() {
    let src = "change x\nauthor B\nbase 0\nend\n";
    let result = parse_changeset(src).expect("must parse");
    assert!(result.approval.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// Composition directives
// ═══════════════════════════════════════════════════════════════════════════

// ── Scenario: depends_on in metadata section ──────────────────────────────
// GIVEN `depends_on change.add_cart_types` in metadata
// WHEN parsed
// THEN composition.depends_on = ["change.add_cart_types"]
#[test]
fn parse_depends_on_in_metadata() {
    let src = "\
change x
author Alice
base 0
metadata
  depends_on change.add_cart_types
end
end
";
    let result = parse_changeset(src).expect("must parse");
    assert_eq!(
        result.composition.depends_on,
        vec!["change.add_cart_types".to_string()]
    );
}

// ── Scenario: supersedes in metadata section ──────────────────────────────
#[test]
fn parse_supersedes_in_metadata() {
    let src = "\
change x
author Alice
base 0
metadata
  supersedes change.old_attempt
end
end
";
    let result = parse_changeset(src).expect("must parse");
    assert_eq!(
        result.composition.supersedes,
        vec!["change.old_attempt".to_string()]
    );
}

// ── Scenario: conflicts_with in metadata section ──────────────────────────
#[test]
fn parse_conflicts_with_in_metadata() {
    let src = "\
change x
author Alice
base 0
metadata
  conflicts_with change.rewrite_checkout
end
end
";
    let result = parse_changeset(src).expect("must parse");
    assert_eq!(
        result.composition.conflicts_with,
        vec!["change.rewrite_checkout".to_string()]
    );
}

// ── Scenario: multiple composition directives ─────────────────────────────
#[test]
fn parse_multiple_composition_directives() {
    let src = "\
change x
author Alice
base 0
metadata
  depends_on change.a
  depends_on change.b
  supersedes change.old
  conflicts_with change.c
end
end
";
    let result = parse_changeset(src).expect("must parse");
    assert_eq!(result.composition.depends_on.len(), 2);
    assert!(
        result
            .composition
            .depends_on
            .contains(&"change.a".to_string())
    );
    assert!(
        result
            .composition
            .depends_on
            .contains(&"change.b".to_string())
    );
    assert_eq!(
        result.composition.supersedes,
        vec!["change.old".to_string()]
    );
    assert_eq!(
        result.composition.conflicts_with,
        vec!["change.c".to_string()]
    );
}

// ── Scenario: composition fields default to empty when absent ─────────────
#[test]
fn parse_no_composition_directives_defaults_to_empty() {
    let src = "change x\nauthor D\nbase 0\nend\n";
    let result = parse_changeset(src).expect("must parse");
    assert!(result.composition.depends_on.is_empty());
    assert!(result.composition.supersedes.is_empty());
    assert!(result.composition.conflicts_with.is_empty());
    assert!(result.composition.part_of.is_empty());
    assert!(result.composition.blocks.is_empty());
}

// ── Scenario: full spec-example with expect + approval + composition ───────
#[test]
fn parse_full_spec_example_all_sections() {
    let src = "\
change add_checkout acl=1.0 base=1
author agent
intent \"Add checkout flow with payment and order creation\"

metadata
  depends_on change.add_cart_types
  supersedes change.old_checkout_attempt
end

requires
  assert_exists 1
  assert_exists 2
end

expect
  creates fn.checkout
  no_unsafe
  no_unverified
end

ops
  op create_function id=fn.checkout
  op add_param target=fn.checkout name=cartId type=CartId
  op verify
end

approval
  require_if public_api_changed
end

end
";
    let result = parse_changeset(src).expect("full spec example must parse");

    assert_eq!(result.acl_version, "1.0");
    assert_eq!(result.changeset.base_snapshot_id.0, 1);
    assert_eq!(
        result.changeset.meta.description,
        "Add checkout flow with payment and order creation"
    );
    assert_eq!(result.preconditions.len(), 2);
    assert_eq!(result.changeset.ops.len(), 3);

    let expect = result.expect.expect("expect must be Some");
    assert_eq!(expect.0.len(), 3);

    let approval = result.approval.expect("approval must be Some");
    assert_eq!(approval.0.len(), 1);

    assert_eq!(
        result.composition.depends_on,
        vec!["change.add_cart_types".to_string()]
    );
    assert_eq!(
        result.composition.supersedes,
        vec!["change.old_checkout_attempt".to_string()]
    );
}

// ── Scenario: unclosed expect section returns error ───────────────────────
#[test]
fn parse_unclosed_expect_section_returns_error() {
    let src = "change x\nauthor E\nbase 0\nexpect\n  creates fn.x\n";
    let err = parse_changeset(src).expect_err("unclosed expect must error");
    assert!(
        err.contains("unclosed"),
        "error must say 'unclosed'; got: {err}"
    );
}

// ── Scenario: unclosed approval section returns error ─────────────────────
#[test]
fn parse_unclosed_approval_section_returns_error() {
    let src = "change x\nauthor F\nbase 0\napproval\n  require_if unsafe_added\n";
    let err = parse_changeset(src).expect_err("unclosed approval must error");
    assert!(
        err.contains("unclosed"),
        "error must say 'unclosed'; got: {err}"
    );
}
