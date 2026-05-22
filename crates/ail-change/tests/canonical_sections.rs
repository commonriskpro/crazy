// ── ail-change: canonical section materialization tests (G22 Phase F) ────────
//
// Strict TDD — RED first.
// Tests cover:
//   1. expect/approval/composition carried into CanonicalChangeSet
//   2. infer_* expansion into explicit ops
//   3. acl_version-aware canonicalization behavior
//   4. blocks carried into CanonicalChangeSet

use ail_change::acl_migrator::MigrateError;
use ail_change::canonical::{CanonicalChangeSet, canonicalize_parsed, try_canonicalize_parsed};
use ail_change::model::{ChangeSet, ChangeSetMeta, ChangeSetOp, SnapshotId, Timestamp};
use ail_change::parser::{ChangeComposition, parse_changeset};

// ── helpers ───────────────────────────────────────────────────────────────

fn minimal_parsed(
    parsed_ops: Vec<ail_change::parser::ParsedOp>,
) -> ail_change::parser::ParsedChangeSet {
    use std::collections::BTreeMap;
    ail_change::parser::ParsedChangeSet {
        changeset: ChangeSet {
            meta: ChangeSetMeta {
                author: "alice".to_string(),
                description: "test".to_string(),
                timestamp: Timestamp(0),
            },
            base_snapshot_id: SnapshotId(1),
            ops: parsed_ops.iter().map(|o| o.kind.clone()).collect(),
        },
        preconditions: vec![],
        parsed_ops,
        acl_version: "1.0".to_string(),
        expect: None,
        approval: None,
        composition: ChangeComposition::default(),
        blocks: vec![],
        verify: vec![],
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// expect carried into CanonicalChangeSet
// ═══════════════════════════════════════════════════════════════════════════

// ── Scenario: expect claims are carried into CanonicalChangeSet ────────────
// GIVEN a ParsedChangeSet with expect claims
// WHEN canonicalize_parsed is called
// THEN canonical.expect contains the claims
#[test]
fn canonicalize_carries_expect_claims() {
    use ail_change::parser::ExpectClaims;
    let mut pcs = minimal_parsed(vec![]);
    pcs.expect = Some(ExpectClaims(vec![
        "creates fn.checkout".to_string(),
        "no_unsafe".to_string(),
    ]));

    let canonical = canonicalize_parsed(pcs);

    let expect = canonical.expect.expect("canonical must carry expect");
    assert_eq!(expect.0.len(), 2);
    assert!(expect.0.contains(&"creates fn.checkout".to_string()));
}

// ── Scenario: None expect → None in canonical ─────────────────────────────
#[test]
fn canonicalize_carries_none_expect() {
    let pcs = minimal_parsed(vec![]);
    let canonical = canonicalize_parsed(pcs);
    assert!(canonical.expect.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// approval carried into CanonicalChangeSet
// ═══════════════════════════════════════════════════════════════════════════

// ── Scenario: approval requirements are carried into CanonicalChangeSet ────
#[test]
fn canonicalize_carries_approval_requirements() {
    use ail_change::parser::ApprovalRequirements;
    let mut pcs = minimal_parsed(vec![]);
    pcs.approval = Some(ApprovalRequirements(vec![
        "require_if public_api_changed".to_string(),
    ]));

    let canonical = canonicalize_parsed(pcs);

    let approval = canonical.approval.expect("canonical must carry approval");
    assert_eq!(approval.0.len(), 1);
}

// ── Scenario: None approval → None in canonical ───────────────────────────
#[test]
fn canonicalize_carries_none_approval() {
    let pcs = minimal_parsed(vec![]);
    let canonical = canonicalize_parsed(pcs);
    assert!(canonical.approval.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// composition carried into CanonicalChangeSet
// ═══════════════════════════════════════════════════════════════════════════

// ── Scenario: composition depends_on is carried into CanonicalChangeSet ────
#[test]
fn canonicalize_carries_composition() {
    let mut pcs = minimal_parsed(vec![]);
    pcs.composition
        .depends_on
        .push("change.add_cart_types".to_string());
    pcs.composition.supersedes.push("change.old".to_string());

    let canonical = canonicalize_parsed(pcs);

    assert_eq!(
        canonical.composition.depends_on,
        vec!["change.add_cart_types".to_string()]
    );
    assert_eq!(
        canonical.composition.supersedes,
        vec!["change.old".to_string()]
    );
}

// ── Scenario: empty composition produces empty canonical composition ────────
#[test]
fn canonicalize_carries_empty_composition() {
    let pcs = minimal_parsed(vec![]);
    let canonical = canonicalize_parsed(pcs);
    assert!(canonical.composition.depends_on.is_empty());
    assert!(canonical.composition.supersedes.is_empty());
    assert!(canonical.composition.conflicts_with.is_empty());
    assert!(canonical.composition.part_of.is_empty());
    assert!(canonical.composition.blocks.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// blocks carried into CanonicalChangeSet
// ═══════════════════════════════════════════════════════════════════════════

// ── Scenario: blocks are carried into CanonicalChangeSet ──────────────────
#[test]
fn canonicalize_carries_blocks() {
    use ail_change::parser::ParsedBlock;
    let mut pcs = minimal_parsed(vec![]);
    pcs.blocks = vec![ParsedBlock {
        kind: "expr".to_string(),
        block_ref: "@expr.checkout_body".to_string(),
        content: "let x = 1\nreturn x".to_string(),
        hash: Some("abc123".to_string()),
    }];

    let canonical = canonicalize_parsed(pcs);

    assert_eq!(canonical.blocks.len(), 1);
    let b = &canonical.blocks[0];
    assert_eq!(b.kind, "expr");
    assert_eq!(b.block_ref, "@expr.checkout_body");
    assert_eq!(b.hash, Some("abc123".to_string()));
}

// ── Scenario: no blocks → empty canonical blocks ───────────────────────────
#[test]
fn canonicalize_carries_empty_blocks() {
    let pcs = minimal_parsed(vec![]);
    let canonical = canonicalize_parsed(pcs);
    assert!(canonical.blocks.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// verify carried into CanonicalChangeSet
// ═══════════════════════════════════════════════════════════════════════════

// ── Scenario: verify entries are carried into CanonicalChangeSet ──────────
#[test]
fn canonicalize_carries_verify() {
    let mut pcs = minimal_parsed(vec![]);
    pcs.verify = vec![
        "target fn.checkout".to_string(),
        "contracts required".to_string(),
    ];

    let canonical = canonicalize_parsed(pcs);

    assert_eq!(canonical.verify.len(), 2);
    assert!(canonical.verify.contains(&"target fn.checkout".to_string()));
}

// ═══════════════════════════════════════════════════════════════════════════
// infer_* expansion
// ═══════════════════════════════════════════════════════════════════════════

// ── Scenario: infer_boundary is expanded to a marker in canonical ops ─────
// The spec says infer_boundary materializes into set_return + add_effect.
// Without a live semantic graph, the canonicalizer emits an InferMarker op
// that downstream can expand with graph context.
//
// For the canonicalization phase alone:
//   infer_boundary target=fn.checkout
// becomes:
//   infer_boundary target=fn.checkout  [with infer_expanded=pending marker]
//
// Full expansion with actual types requires the verifier (graph context).
// The canonicalizer records that expansion is needed.
#[test]
fn infer_boundary_is_marked_for_expansion() {
    use ail_change::parser::{ParsedChangeSet, ParsedOp};
    use std::collections::BTreeMap;

    let mut args = BTreeMap::new();
    args.insert("target".to_string(), "fn.checkout".to_string());
    let parsed_op = ParsedOp {
        kind: ChangeSetOp::Infer,
        verb: "infer_boundary".to_string(),
        args,
    };

    let pcs = ail_change::parser::ParsedChangeSet {
        changeset: ChangeSet {
            meta: ChangeSetMeta {
                author: "alice".to_string(),
                description: "test".to_string(),
                timestamp: Timestamp(0),
            },
            base_snapshot_id: SnapshotId(0),
            ops: vec![ChangeSetOp::Infer],
        },
        preconditions: vec![],
        parsed_ops: vec![parsed_op],
        acl_version: "1.0".to_string(),
        expect: None,
        approval: None,
        composition: ChangeComposition::default(),
        blocks: vec![],
        verify: vec![],
    };

    let canonical = canonicalize_parsed(pcs);

    // The infer_boundary op must be present in canonical form.
    let infer_op = canonical.ops.iter().find(|o| o.verb == "infer_boundary");
    assert!(
        infer_op.is_some(),
        "infer_boundary must be in canonical ops"
    );

    // It must be marked as needing expansion.
    let infer_op = infer_op.unwrap();
    assert_eq!(
        infer_op.args.get("infer_pending"),
        Some(&"true".to_string()),
        "infer_boundary must be marked with infer_pending=true"
    );
}

// ── Scenario: infer_effects is also marked for expansion ──────────────────
#[test]
fn infer_effects_is_marked_for_expansion() {
    use ail_change::parser::ParsedOp;
    use std::collections::BTreeMap;

    let mut args = BTreeMap::new();
    args.insert("target".to_string(), "fn.checkout".to_string());
    let parsed_op = ParsedOp {
        kind: ChangeSetOp::Infer,
        verb: "infer_effects".to_string(),
        args,
    };

    let pcs = ail_change::parser::ParsedChangeSet {
        changeset: ChangeSet {
            meta: ChangeSetMeta {
                author: "alice".to_string(),
                description: "test".to_string(),
                timestamp: Timestamp(0),
            },
            base_snapshot_id: SnapshotId(0),
            ops: vec![ChangeSetOp::Infer],
        },
        preconditions: vec![],
        parsed_ops: vec![parsed_op],
        acl_version: "1.0".to_string(),
        expect: None,
        approval: None,
        composition: ChangeComposition::default(),
        blocks: vec![],
        verify: vec![],
    };

    let canonical = canonicalize_parsed(pcs);
    let infer_op = canonical
        .ops
        .iter()
        .find(|o| o.verb == "infer_effects")
        .unwrap();
    assert_eq!(
        infer_op.args.get("infer_pending"),
        Some(&"true".to_string())
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// acl_version-aware canonicalization
// ═══════════════════════════════════════════════════════════════════════════

// ── Scenario: acl_version "1.0" is recorded in canonical form ────────────
#[test]
fn canonicalize_records_acl_version_1_0() {
    let src = "change x acl=1.0 base=0\nauthor Alice\nend\n";
    let parsed = parse_changeset(src).expect("must parse");
    let canonical = canonicalize_parsed(parsed);
    assert_eq!(canonical.acl_version, "1.0");
}

// ── Scenario: unknown acl_version returns MigrateError ───────────────────
// With version-aware migration, unknown/future versions cannot be processed.
#[test]
fn canonicalize_unknown_acl_version_returns_error() {
    use ail_change::parser::ParsedChangeSet;

    let mut pcs = minimal_parsed(vec![]);
    pcs.acl_version = "2.0".to_string();

    let result = try_canonicalize_parsed(pcs);
    assert!(
        matches!(result, Err(MigrateError::UnknownVersion(ref v)) if v == "2.0"),
        "unknown version must return UnknownVersion error, got {result:?}"
    );
}

// ── Scenario: full round-trip of the spec's end-to-end example ────────────
// The complete submitted-form example from the spec must parse and canonicalize.
#[test]
fn spec_end_to_end_example_round_trips() {
    let src = "\
change add_cart_total acl=1.0 base=snapshot_001
author agent
intent \"Add pure cart total calculation\"

requires
  assert_exists 1
  assert_exists 2
  assert_exists 3
  assert_exists 4
  assert_exists 5
end

expect
  creates fn.cart_total
  modifies module.cart
  no_new_public_api_except fn.cart_total
  no_unsafe
  no_unverified
end

ops
  op create_function id=fn.cart_total
  op add_param target=fn.cart_total name=cart type=Cart
  op infer_boundary target=fn.cart_total
  op add_contract target=fn.cart_total kind=ensures rule=\"result >= Decimal.zero\"
  op set_body target=fn.cart_total body=@expr.cart_total
  op expose target=fn.cart_total as=api.cart_total
end

block expr @expr.cart_total
  let total = List.fold(cart.items, Decimal.zero, fn.add_line_total)
  return total
end

verify
  target fn.cart_total
  contracts required
  effects required
  refinements required
end

approval
  require_if public_api_changed
  require_if unsafe_added
end

end
";
    let parsed = parse_changeset(src).expect("spec example must parse");
    let canonical = canonicalize_parsed(parsed);

    // Metadata
    assert_eq!(canonical.acl_version, "1.0");
    assert_eq!(canonical.base_snapshot_id.0, 1); // snapshot_001 → 1

    // ops
    assert_eq!(canonical.ops.len(), 6);

    // create_function gets visibility=private
    let create_op = canonical
        .ops
        .iter()
        .find(|o| o.verb == "create_function")
        .unwrap();
    assert_eq!(
        create_op.args.get("visibility"),
        Some(&"private".to_string())
    );

    // expect carried
    assert!(canonical.expect.is_some());
    assert_eq!(canonical.expect.unwrap().0.len(), 5);

    // approval carried
    assert!(canonical.approval.is_some());

    // blocks carried
    assert_eq!(canonical.blocks.len(), 1);
    assert_eq!(canonical.blocks[0].block_ref, "@expr.cart_total");

    // verify carried
    assert_eq!(canonical.verify.len(), 4);

    // preconditions
    assert_eq!(canonical.preconditions.len(), 5);
}
