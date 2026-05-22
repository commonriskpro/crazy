// ── ail-change: block parsing + verify forms tests (G22 Phase E) ────────────
//
// Strict TDD — RED first.
// Tests cover:
//   1. Typed block sections (`block expr @ref ... end`)
//   2. verify short form (`verify target=fn.x`)
//   3. verify block form (`verify ... end`)
//   4. Snapshot id syntax (`base snapshot_123`)
//   5. create_type derive=none default
//   6. ID normalization (Fn.CartTotal → fn.cart_total)
//   7. Literal/value normalization

use ail_change::canonical::canonicalize_parsed;
use ail_change::parser::{ParsedBlock, parse_changeset};

// ═══════════════════════════════════════════════════════════════════════════
// Block parsing
// ═══════════════════════════════════════════════════════════════════════════

// ── Scenario: typed block `block expr @ref ... end` is parsed ────────────
// GIVEN a block with kind=expr, ref=@expr.checkout_body
// WHEN parsed
// THEN ParsedChangeSet.blocks has one entry with correct kind and ref
#[test]
fn parse_block_expr_produces_parsed_block() {
    let src = "\
change x
author Alice
base 0
block expr @expr.checkout_body
  let x = 1
  return x
end
end
";
    let result = parse_changeset(src).expect("block must parse");
    assert_eq!(result.blocks.len(), 1);
    let b = &result.blocks[0];
    assert_eq!(b.kind, "expr");
    assert_eq!(b.block_ref, "@expr.checkout_body");
    assert!(b.content.contains("let x = 1"));
}

// ── Scenario: block with inline hash attr ────────────────────────────────
// GIVEN `block expr @expr.foo hash=abc123`
// WHEN parsed
// THEN block.hash == Some("abc123")
#[test]
fn parse_block_with_hash_attr() {
    let src = "\
change x
author Alice
base 0
block expr @expr.foo hash=abc123
  content line
end
end
";
    let result = parse_changeset(src).expect("must parse");
    let b = &result.blocks[0];
    assert_eq!(b.hash, Some("abc123".to_string()));
}

// ── Scenario: multiple blocks are all captured ────────────────────────────
#[test]
fn parse_multiple_blocks() {
    let src = "\
change x
author Alice
base 0
block expr @expr.a
  line a
end
block schema @schema.b
  line b
end
end
";
    let result = parse_changeset(src).expect("must parse");
    assert_eq!(result.blocks.len(), 2);
    assert_eq!(result.blocks[0].kind, "expr");
    assert_eq!(result.blocks[0].block_ref, "@expr.a");
    assert_eq!(result.blocks[1].kind, "schema");
    assert_eq!(result.blocks[1].block_ref, "@schema.b");
}

// ── Scenario: block with no hash produces None ────────────────────────────
#[test]
fn parse_block_without_hash_produces_none() {
    let src = "\
change x
author Alice
base 0
block doc @doc.notes
  some doc content
end
end
";
    let result = parse_changeset(src).expect("must parse");
    assert_eq!(result.blocks[0].hash, None);
}

// ═══════════════════════════════════════════════════════════════════════════
// Verify forms
// ═══════════════════════════════════════════════════════════════════════════

// ── Scenario: verify short form `verify target=fn.x` is parsed ────────────
// GIVEN `verify target=fn.checkout` at top level
// WHEN parsed
// THEN ParsedChangeSet.verify has one entry: target=fn.checkout
#[test]
fn parse_verify_short_form_produces_verify_entry() {
    let src = "\
change x
author Alice
base 0
verify target=fn.checkout
end
";
    let result = parse_changeset(src).expect("verify short form must parse");
    assert_eq!(result.verify.len(), 1);
    assert!(result.verify[0].contains("target=fn.checkout"));
}

// ── Scenario: verify block form `verify ... end` is parsed ────────────────
// GIVEN `verify ... end` with multiple lines
// WHEN parsed
// THEN ParsedChangeSet.verify has all the lines
#[test]
fn parse_verify_block_form_produces_verify_entries() {
    let src = "\
change x
author Alice
base 0
verify
  target fn.checkout
  contracts required
  effects required
end
end
";
    let result = parse_changeset(src).expect("verify block form must parse");
    assert_eq!(result.verify.len(), 3);
    assert!(result.verify.contains(&"target fn.checkout".to_string()));
    assert!(result.verify.contains(&"contracts required".to_string()));
    assert!(result.verify.contains(&"effects required".to_string()));
}

// ── Scenario: verify with bare words `verify impact target=fn.old_checkout` ─
#[test]
fn parse_verify_with_bare_words() {
    let src = "\
change x
author Alice
base 0
verify impact target=fn.old_checkout
end
";
    let result = parse_changeset(src).expect("must parse");
    assert_eq!(result.verify.len(), 1);
    assert!(result.verify[0].contains("impact"));
    assert!(result.verify[0].contains("target=fn.old_checkout"));
}

// ── Scenario: no verify produces empty vec ─────────────────────────────────
#[test]
fn parse_no_verify_produces_empty_vec() {
    let src = "change x\nauthor A\nbase 0\nend\n";
    let result = parse_changeset(src).expect("must parse");
    assert!(result.verify.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// Snapshot id syntax
// ═══════════════════════════════════════════════════════════════════════════

// ── Scenario: `base snapshot_123` (doc-style) is accepted ────────────────
// GIVEN `base snapshot_123`
// WHEN parsed
// THEN base_snapshot_id is parsed from the numeric suffix
#[test]
fn parse_base_snapshot_doc_style() {
    let src = "change x\nauthor Alice\nbase snapshot_123\nend\n";
    let result = parse_changeset(src).expect("snapshot_ prefix must be accepted");
    assert_eq!(result.changeset.base_snapshot_id.0, 123);
}

// ── Scenario: `base=snapshot_001` (inline attr doc-style) is accepted ─────
#[test]
fn parse_base_snapshot_inline_attr_doc_style() {
    let src = "change x acl=1.0 base=snapshot_001\nauthor Alice\nend\n";
    let result = parse_changeset(src).expect("inline base=snapshot_ must be accepted");
    assert_eq!(result.changeset.base_snapshot_id.0, 1);
}

// ── Scenario: numeric base still works ───────────────────────────────────
#[test]
fn parse_base_numeric_still_works() {
    let src = "change x\nauthor Alice\nbase 42\nend\n";
    let result = parse_changeset(src).expect("numeric base must still work");
    assert_eq!(result.changeset.base_snapshot_id.0, 42);
}

// ═══════════════════════════════════════════════════════════════════════════
// ID normalization
// ═══════════════════════════════════════════════════════════════════════════

// ── Scenario: `Fn.CartTotal` is normalized to `fn.cart_total` ─────────────
// ID normalization happens at canonicalization time.
#[test]
fn canonicalize_normalizes_pascal_namespace_id() {
    use ail_change::canonical::normalize_id;
    assert_eq!(normalize_id("Fn.CartTotal"), "fn.cart_total");
}

// ── Scenario: `fn.cart-total` (kebab) is normalized to `fn.cart_total` ────
#[test]
fn canonicalize_normalizes_kebab_id() {
    use ail_change::canonical::normalize_id;
    assert_eq!(normalize_id("fn.cart-total"), "fn.cart_total");
}

// ── Scenario: `fn.cart_total` (already canonical) is unchanged ─────────────
#[test]
fn canonicalize_leaves_canonical_id_unchanged() {
    use ail_change::canonical::normalize_id;
    assert_eq!(normalize_id("fn.cart_total"), "fn.cart_total");
}

// ── Scenario: `type.CartItem` (type namespace with PascalCase) unchanged ───
// PascalCase in the type. namespace is the canonical form per spec.
#[test]
fn canonicalize_leaves_type_pascal_case_unchanged() {
    use ail_change::canonical::normalize_id;
    assert_eq!(normalize_id("type.CartItem"), "type.CartItem");
}

// ── Scenario: op args with ID values are normalized by canonicalize_parsed ─
#[test]
fn canonicalize_parsed_normalizes_id_args() {
    use ail_change::model::{ChangeSet, ChangeSetMeta, ChangeSetOp, SnapshotId, Timestamp};
    use ail_change::parser::ParsedOp;
    use ail_change::parser::{ChangeComposition, ParsedChangeSet};
    use std::collections::BTreeMap;

    let mut args = BTreeMap::new();
    args.insert("id".to_string(), "Fn.CartTotal".to_string());
    let parsed_op = ParsedOp {
        kind: ChangeSetOp::Create,
        verb: "create_function".to_string(),
        args,
    };

    let pcs = ParsedChangeSet {
        changeset: ChangeSet {
            meta: ChangeSetMeta {
                author: "alice".to_string(),
                description: "test".to_string(),
                timestamp: Timestamp(0),
            },
            base_snapshot_id: SnapshotId(0),
            ops: vec![ChangeSetOp::Create],
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
    // `id` arg should be normalized
    assert_eq!(
        canonical.ops[0].args.get("id"),
        Some(&"fn.cart_total".to_string()),
        "id arg must be normalized"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// create_type derive=none default
// ═══════════════════════════════════════════════════════════════════════════

// ── Scenario: create_type without derive gets derive=none default ──────────
#[test]
fn create_type_without_derive_gets_derive_none_default() {
    use ail_change::model::{ChangeSet, ChangeSetMeta, ChangeSetOp, SnapshotId, Timestamp};
    use ail_change::parser::ParsedOp;
    use ail_change::parser::{ChangeComposition, ParsedChangeSet};
    use std::collections::BTreeMap;

    let mut args = BTreeMap::new();
    args.insert("id".to_string(), "type.Address".to_string());
    let parsed_op = ParsedOp {
        kind: ChangeSetOp::Create,
        verb: "create_type".to_string(),
        args,
    };

    let pcs = ParsedChangeSet {
        changeset: ChangeSet {
            meta: ChangeSetMeta {
                author: "alice".to_string(),
                description: "test".to_string(),
                timestamp: Timestamp(0),
            },
            base_snapshot_id: SnapshotId(0),
            ops: vec![ChangeSetOp::Create],
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
    assert_eq!(
        canonical.ops[0].args.get("derive"),
        Some(&"none".to_string()),
        "create_type must default derive=none"
    );
}

// ── TRIANGULATE: create_type with explicit derive is NOT overridden ─────────
#[test]
fn create_type_with_explicit_derive_is_preserved() {
    use ail_change::model::{ChangeSet, ChangeSetMeta, ChangeSetOp, SnapshotId, Timestamp};
    use ail_change::parser::ParsedOp;
    use ail_change::parser::{ChangeComposition, ParsedChangeSet};
    use std::collections::BTreeMap;

    let mut args = BTreeMap::new();
    args.insert("id".to_string(), "type.Foo".to_string());
    args.insert("derive".to_string(), "eq".to_string());
    let parsed_op = ParsedOp {
        kind: ChangeSetOp::Create,
        verb: "create_type".to_string(),
        args,
    };

    let pcs = ParsedChangeSet {
        changeset: ChangeSet {
            meta: ChangeSetMeta {
                author: "alice".to_string(),
                description: "test".to_string(),
                timestamp: Timestamp(0),
            },
            base_snapshot_id: SnapshotId(0),
            ops: vec![ChangeSetOp::Create],
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
    assert_eq!(
        canonical.ops[0].args.get("derive"),
        Some(&"eq".to_string()),
        "explicit derive must not be overridden"
    );
}
