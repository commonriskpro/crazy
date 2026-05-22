// ── ail-change: parser kv argument tests (G22 Phase A) ────────────────────
//
// Strict TDD — RED phase.
// Tests for ParsedOp, OpArgs, and kv argument parsing in the ACL parser.

use std::collections::BTreeMap;

use ail_change::model::ChangeSetOp;
use ail_change::parser::{OpArgs, ParsedOp, parse_changeset};

// ── Scenario: op with multiple kv args ────────────────────────────────────
// GIVEN `op create_function id=fn.checkout visibility=public`
// WHEN parse_changeset is called
// THEN parsed_ops[0] has verb="create_function", kind=Create,
//      args={"id": "fn.checkout", "visibility": "public"}
#[test]
fn parse_op_kv_args_are_captured() {
    let src = "\
change x
author Alice
base 0
op create_function id=fn.checkout visibility=public
end
";
    let result = parse_changeset(src).expect("must parse");
    assert_eq!(result.parsed_ops.len(), 1);
    let op = &result.parsed_ops[0];
    assert_eq!(op.kind, ChangeSetOp::Create);
    assert_eq!(op.verb, "create_function");
    assert_eq!(op.args.get("id"), Some(&"fn.checkout".to_string()));
    assert_eq!(op.args.get("visibility"), Some(&"public".to_string()));
}

// ── Scenario: op with quoted string value ─────────────────────────────────
// GIVEN `op set_return target=fn.x type="Result<OrderId, Err>"`
// WHEN parse_changeset is called
// THEN args contains the unquoted type value
#[test]
fn parse_op_quoted_value_is_unquoted() {
    let src = "change x\nauthor B\nbase 0\nop set_return target=fn.x type=\"Result<OrderId, Err>\"\nend\n";
    let result = parse_changeset(src).expect("must parse");
    let op = &result.parsed_ops[0];
    assert_eq!(
        op.args.get("type"),
        Some(&"Result<OrderId, Err>".to_string())
    );
    assert_eq!(op.args.get("target"), Some(&"fn.x".to_string()));
}

// ── Scenario: op with no args has empty args map ──────────────────────────
// GIVEN `op verify`
// WHEN parsed
// THEN args is empty
#[test]
fn parse_op_no_args_produces_empty_map() {
    let src = "change x\nauthor C\nbase 0\nop verify\nend\n";
    let result = parse_changeset(src).expect("must parse");
    let op = &result.parsed_ops[0];
    assert_eq!(op.kind, ChangeSetOp::Verify);
    assert_eq!(op.verb, "verify");
    assert!(op.args.is_empty(), "no args expected");
}

// ── Scenario: full verb is stored ─────────────────────────────────────────
// GIVEN `op create_function id=fn.checkout`
// WHEN parsed
// THEN verb="create_function" (not just "create")
#[test]
fn parse_op_full_verb_is_stored() {
    let src = "change x\nauthor D\nbase 0\nop create_function id=fn.x\nend\n";
    let result = parse_changeset(src).expect("must parse");
    assert_eq!(result.parsed_ops[0].verb, "create_function");
}

// ── Scenario: multiple ops produce multiple parsed_ops ────────────────────
// GIVEN 3 op lines
// WHEN parsed
// THEN parsed_ops.len() == 3
#[test]
fn parse_multiple_ops_produces_matching_parsed_ops() {
    let src = "\
change x
author E
base 0
op create_function id=fn.x
op add_param target=fn.x name=a type=Int
op verify
end
";
    let result = parse_changeset(src).expect("must parse");
    assert_eq!(result.parsed_ops.len(), 3);
    assert_eq!(result.parsed_ops[0].verb, "create_function");
    assert_eq!(result.parsed_ops[1].verb, "add_param");
    assert_eq!(result.parsed_ops[2].verb, "verify");
}

// ── Scenario: parsed_ops and ops vecs have the same length ────────────────
// Ensures backward-compat: ops vec still exists alongside parsed_ops.
#[test]
fn parsed_ops_and_ops_have_equal_length() {
    let src = "\
change x
author F
base 0
op create_function id=fn.x
op set_return target=fn.x type=Unit
end
";
    let result = parse_changeset(src).expect("must parse");
    assert_eq!(result.changeset.ops.len(), result.parsed_ops.len());
}

// ── Scenario: ops inside an explicit ops section also produce parsed_ops ───
#[test]
fn ops_in_section_also_produce_parsed_ops() {
    let src = "\
change x
author G
base 0
ops
  op create_function id=fn.y
  op add_param target=fn.y name=b type=Bool
end
end
";
    let result = parse_changeset(src).expect("must parse");
    assert_eq!(result.parsed_ops.len(), 2);
    assert_eq!(
        result.parsed_ops[0].args.get("id"),
        Some(&"fn.y".to_string())
    );
}

// ── Scenario: ref value (starting with @) is stored as-is ─────────────────
#[test]
fn parse_op_ref_value_is_stored_as_is() {
    let src = "change x\nauthor H\nbase 0\nop set_body target=fn.x body=@expr.checkout\nend\n";
    let result = parse_changeset(src).expect("must parse");
    let op = &result.parsed_ops[0];
    assert_eq!(op.args.get("body"), Some(&"@expr.checkout".to_string()));
}

// ── TRIANGULATE: OpArgs is BTreeMap<String, String> ─────────────────────
// Just a type-level check via construction.
#[test]
fn op_args_is_btreemap() {
    let mut args: OpArgs = BTreeMap::new();
    args.insert("key".to_string(), "value".to_string());
    assert_eq!(args.get("key"), Some(&"value".to_string()));
}
