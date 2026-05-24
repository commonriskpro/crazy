use super::*;
use crate::model::{ChangeSetOp, SnapshotId};

// Scenario: minimal valid input (author + base, no ops).
//   GIVEN a document with only required fields
//   WHEN parse_changeset is called
//   THEN the returned ParsedChangeSet has the expected author and snapshot id
#[test]
fn parse_minimal_changeset_succeeds() {
    let src = "change minimal\nauthor Alice\nbase 0\nend\n";
    let result = parse_changeset(src).expect("minimal changeset must parse");
    assert_eq!(result.changeset.meta.author, "Alice");
    assert_eq!(result.changeset.base_snapshot_id, SnapshotId(0));
    assert!(result.changeset.ops.is_empty(), "no ops expected");
    assert!(result.preconditions.is_empty(), "no preconditions expected");
}

// Scenario: description defaults to empty when absent.
//   GIVEN no description or intent line
//   WHEN parse_changeset is called
//   THEN description is empty string
#[test]
fn parse_missing_description_defaults_to_empty() {
    let src = "change x\nauthor Bob\nbase 1\nend\n";
    let result = parse_changeset(src).expect("must parse");
    assert_eq!(result.changeset.meta.description, "");
}

// Scenario: intent line sets description.
//   GIVEN `intent "Add cart total"`
//   WHEN parse_changeset is called
//   THEN description equals the unquoted content
#[test]
fn parse_intent_line_sets_description() {
    let src = "change x\nauthor Bob\nbase 1\nintent \"Add cart total\"\nend\n";
    let result = parse_changeset(src).expect("must parse");
    assert_eq!(result.changeset.meta.description, "Add cart total");
}

// Scenario: description line sets description.
#[test]
fn parse_description_line_sets_description() {
    let src = "change x\nauthor Bob\nbase 1\ndescription My change\nend\n";
    let result = parse_changeset(src).expect("must parse");
    assert_eq!(result.changeset.meta.description, "My change");
}

// Scenario: all 7 op verb prefix groups are mapped correctly.
//   GIVEN one representative op for each of the 7 categories
//   WHEN parse_changeset is called
//   THEN ops vec contains the 7 variants in order
#[test]
fn parse_all_seven_op_categories() {
    let src = "\
change test
author Carol
base 2
op create_function id=fn.checkout
op set_return target=fn.checkout type=\"Result\"
op add_param target=fn.checkout name=x type=CartId
op remove_effect target=fn.checkout effect=io
op connect source=fn.checkout relation=uses target=cap.pay
op infer_boundary target=fn.checkout
op verify
end
";
    let result = parse_changeset(src).expect("all 7 ops must parse");
    assert_eq!(
        result.changeset.ops,
        vec![
            ChangeSetOp::Create,
            ChangeSetOp::Set,
            ChangeSetOp::Add,
            ChangeSetOp::Remove,
            ChangeSetOp::Connect,
            ChangeSetOp::Infer,
            ChangeSetOp::Verify,
        ]
    );
}

// Scenario: `delete` maps to Delete and `disconnect` maps to Disconnect.
#[test]
fn parse_delete_and_disconnect_map_to_own_variants() {
    let src = "change x\nauthor D\nbase 0\nop delete target=fn.old\nop disconnect source=a relation=r target=b\nend\n";
    let result = parse_changeset(src).expect("must parse");
    assert_eq!(
        result.changeset.ops,
        vec![ChangeSetOp::Delete, ChangeSetOp::Disconnect]
    );
}

// Scenario: ops inside `ops ... end` section form.
//   GIVEN ops wrapped in an explicit `ops` section
//   WHEN parsed
//   THEN same result as short form
#[test]
fn parse_section_form_ops() {
    let src = "\
change x
author Eve
base 5
ops
  op create_function id=fn.checkout
  op set_return target=fn.checkout type=Unit
end
end
";
    let result = parse_changeset(src).expect("section form must parse");
    assert_eq!(
        result.changeset.ops,
        vec![ChangeSetOp::Create, ChangeSetOp::Set]
    );
    assert_eq!(result.changeset.base_snapshot_id, SnapshotId(5));
}

// Scenario: short form — ops directly under change, no `ops` section.
#[test]
fn parse_short_form_ops() {
    let src = "change x\nauthor Frank\nbase 3\nop create id=fn.x\nop verify\nend\n";
    let result = parse_changeset(src).expect("short form must parse");
    assert_eq!(
        result.changeset.ops,
        vec![ChangeSetOp::Create, ChangeSetOp::Verify]
    );
}

// Scenario: `requires` block with assert_exists.
//   GIVEN `assert_exists <node_id>` inside a requires block
//   WHEN parsed
//   THEN preconditions contains AssertExists with the correct node id
#[test]
fn parse_requires_assert_exists() {
    let src = "\
change x
author Grace
base 0
requires
  assert_exists 42
end
end
";
    let result = parse_changeset(src).expect("assert_exists must parse");
    assert_eq!(result.preconditions.len(), 1);
    let crate::canonical::Precondition::AssertExists(ae) = &result.preconditions[0] else {
        panic!("expected AssertExists precondition");
    };
    assert_eq!(ae.node_id, NodeRef(42));
}

// Scenario: `requires` block with assert_hash.
//   GIVEN `assert_hash <node_id> sig=<64 hex chars>`
//   WHEN parsed
//   THEN preconditions contains AssertHash with the correct node id and decoded hash
#[test]
fn parse_requires_assert_hash() {
    let hex = "a".repeat(64);
    let src =
        format!("change x\nauthor Hank\nbase 0\nrequires\n  assert_hash 7 sig={hex}\nend\nend\n");
    let result = parse_changeset(&src).expect("assert_hash must parse");
    assert_eq!(result.preconditions.len(), 1);
    let crate::canonical::Precondition::AssertHash(ah) = &result.preconditions[0] else {
        panic!("expected AssertHash precondition");
    };
    assert_eq!(ah.node_id, NodeRef(7));
    // 0xaa repeated 32 times.
    assert_eq!(ah.expected_hash.0, [0xaa_u8; 32]);
}

// Scenario: metadata block sets author and description.
//   GIVEN a `metadata ... end` block with author and description
//   WHEN parsed
//   THEN author and description are correctly set
#[test]
fn parse_metadata_block() {
    let src = "\
change x
base 0
metadata
  author Iris
  description From metadata block
end
end
";
    let result = parse_changeset(src).expect("metadata block must parse");
    assert_eq!(result.changeset.meta.author, "Iris");
    assert_eq!(result.changeset.meta.description, "From metadata block");
}

// Scenario: comments and blank lines are ignored.
//   GIVEN a document with # comments and blank lines interspersed
//   WHEN parsed
//   THEN parse succeeds and content lines are processed normally
#[test]
fn parse_ignores_comments_and_blanks() {
    let src = "\
# this is a preamble comment
change x

# set metadata
author Jack
base 3

# one op
op create_function id=fn.x

end
";
    let result = parse_changeset(src).expect("comments and blanks must be ignored");
    assert_eq!(result.changeset.meta.author, "Jack");
    assert_eq!(result.changeset.base_snapshot_id, SnapshotId(3));
    assert_eq!(result.changeset.ops, vec![ChangeSetOp::Create]);
}

// Scenario: missing author → ParseError.
#[test]
fn parse_missing_author_returns_error() {
    let src = "change x\nbase 0\nend\n";
    let err = parse_changeset(src).expect_err("missing author must error");
    assert!(
        err.contains("author"),
        "error must mention 'author'; got: {err}"
    );
}

// Scenario: missing base → ParseError.
#[test]
fn parse_missing_base_returns_error() {
    let src = "change x\nauthor Kim\nend\n";
    let err = parse_changeset(src).expect_err("missing base must error");
    assert!(
        err.contains("base"),
        "error must mention 'base'; got: {err}"
    );
}

// Scenario: invalid base (non-u64) → ParseError.
#[test]
fn parse_invalid_base_returns_error() {
    let src = "change x\nauthor Lee\nbase not_a_number\nend\n";
    let err = parse_changeset(src).expect_err("invalid base must error");
    assert!(
        err.contains("invalid base snapshot id"),
        "error must describe the problem; got: {err}"
    );
}

// Scenario: unknown op verb → ParseError.
#[test]
fn parse_unknown_op_verb_returns_error() {
    let src = "change x\nauthor Mia\nbase 0\nop frobnicate target=fn.x\nend\n";
    let err = parse_changeset(src).expect_err("unknown op verb must error");
    assert!(
        err.contains("frobnicate"),
        "error must name the unknown verb; got: {err}"
    );
}

// Scenario: assert_hash with wrong hex length → ParseError.
#[test]
fn parse_assert_hash_wrong_length_returns_error() {
    let src = "change x\nauthor Ned\nbase 0\nrequires\n  assert_hash 1 sig=deadbeef\nend\nend\n";
    let err = parse_changeset(src).expect_err("short hex must error");
    assert!(
        err.contains("64 hex characters"),
        "error must describe hex length; got: {err}"
    );
}

// Scenario: assert_hash with missing sig= → ParseError.
#[test]
fn parse_assert_hash_missing_sig_returns_error() {
    let src = "change x\nauthor Ned\nbase 0\nrequires\n  assert_hash 1\nend\nend\n";
    let err = parse_changeset(src).expect_err("missing sig must error");
    assert!(err.contains("sig"), "error must mention 'sig'; got: {err}");
}

// Scenario: extract_string_value strips quotes.
#[test]
fn extract_string_value_strips_quotes() {
    assert_eq!(extract_string_value("\"hello world\""), "hello world");
    assert_eq!(extract_string_value("bare"), "bare");
    assert_eq!(extract_string_value("\"\""), "");
}

// Scenario: extract_kv_value finds the right key.
#[test]
fn extract_kv_value_finds_key() {
    assert_eq!(
        extract_kv_value("target=fn.x type=Int", "target"),
        Some("fn.x".to_string())
    );
    assert_eq!(
        extract_kv_value("target=fn.x type=Int", "type"),
        Some("Int".to_string())
    );
    assert_eq!(extract_kv_value("target=fn.x", "missing"), None);
}

#[test]
fn parse_kv_args_keeps_parenthesized_body_with_spaces() {
    let args = parse_kv_args("target=fn.add body=add(x, y) return=Int");

    assert_eq!(args.get("target").map(String::as_str), Some("fn.add"));
    assert_eq!(args.get("body").map(String::as_str), Some("add(x, y)"));
    assert_eq!(args.get("return").map(String::as_str), Some("Int"));
}

// ── Gap 3: Set/list kv grammar ────────────────────────────────────────

// Scenario: set literal value `{a,b}` is captured whole.
//   GIVEN `effects={database.read:Cart,payment.charge:PaymentProvider}`
//   WHEN parse_kv_args is called
//   THEN `effects` maps to the full set literal string
#[test]
fn parse_kv_args_set_literal_captured_whole() {
    let args = parse_kv_args(
        "target=fn.checkout effects={database.read:Cart,payment.charge:PaymentProvider}",
    );
    assert_eq!(args.get("target").map(String::as_str), Some("fn.checkout"));
    assert_eq!(
        args.get("effects").map(String::as_str),
        Some("{database.read:Cart,payment.charge:PaymentProvider}")
    );
}

// Scenario: list literal value `[a,b]` is captured whole.
//   GIVEN `items=[one,two,three]`
//   WHEN parse_kv_args is called
//   THEN `items` maps to the full list literal string
#[test]
fn parse_kv_args_list_literal_captured_whole() {
    let args = parse_kv_args("items=[one,two,three] other=value");
    assert_eq!(
        args.get("items").map(String::as_str),
        Some("[one,two,three]")
    );
    assert_eq!(args.get("other").map(String::as_str), Some("value"));
}

// TRIANGULATE: nested set `{a,{b,c}}` is captured with inner braces intact.
#[test]
fn parse_kv_args_nested_set_literal_captured_whole() {
    let args = parse_kv_args("val={a,{b,c}} key=x");
    assert_eq!(args.get("val").map(String::as_str), Some("{a,{b,c}}"));
}

// ── Gap 4: assert_context precondition ────────────────────────────────

// Scenario: `assert_context` with target and hash is parsed.
//   GIVEN `assert_context fn.checkout hash=abc123`
//   WHEN parsed
//   THEN preconditions contains AssertContext with target and context_hash
#[test]
fn parse_assert_context_with_hash() {
    let src = "\
change x
author A
base 0
requires
  assert_context fn.checkout hash=abc123
end
end
";
    let result = parse_changeset(src).expect("assert_context must parse");
    assert_eq!(result.preconditions.len(), 1);
    match &result.preconditions[0] {
        crate::canonical::Precondition::AssertContext {
            target_name,
            context_hash,
        } => {
            assert_eq!(target_name, "fn.checkout");
            assert_eq!(context_hash.as_deref(), Some("abc123"));
        }
        other => panic!("expected AssertContext, got {other:?}"),
    }
}

// Scenario: `assert_context` without hash is parsed (target-only form).
#[test]
fn parse_assert_context_target_only() {
    let src = "\
change x
author A
base 0
requires
  assert_context type.Cart
end
end
";
    let result = parse_changeset(src).expect("assert_context target-only must parse");
    match &result.preconditions[0] {
        crate::canonical::Precondition::AssertContext {
            target_name,
            context_hash,
        } => {
            assert_eq!(target_name, "type.Cart");
            assert!(context_hash.is_none());
        }
        other => panic!("expected AssertContext, got {other:?}"),
    }
}

// ── Gap 5: Named NodeRefs in assert_exists / assert_hash ─────────────

// Scenario: `assert_exists type.Cart` is parsed as AssertExistsByName.
//   GIVEN `assert_exists type.Cart` inside a requires section
//   WHEN parsed
//   THEN preconditions contains AssertExistsByName("type.Cart")
#[test]
fn parse_assert_exists_named_node_ref() {
    let src = "\
change x
author A
base 0
requires
  assert_exists type.Cart
  assert_exists fn.cart_total
end
end
";
    let result = parse_changeset(src).expect("named assert_exists must parse");
    assert_eq!(result.preconditions.len(), 2);
    match &result.preconditions[0] {
        crate::canonical::Precondition::AssertExistsByName(name) => {
            assert_eq!(name, "type.Cart");
        }
        other => panic!("expected AssertExistsByName, got {other:?}"),
    }
    match &result.preconditions[1] {
        crate::canonical::Precondition::AssertExistsByName(name) => {
            assert_eq!(name, "fn.cart_total");
        }
        other => panic!("expected AssertExistsByName, got {other:?}"),
    }
}

// Scenario: `assert_hash fn.cart_total sig=<hex>` is parsed as AssertHashByName.
//   GIVEN `assert_hash fn.cart_total sig=<64-hex-chars>`
//   WHEN parsed
//   THEN preconditions contains AssertHashByName with the correct name and hash
#[test]
fn parse_assert_hash_named_node_ref() {
    let hex = "b".repeat(64);
    let src = format!(
        "change x\nauthor A\nbase 0\nrequires\n  assert_hash fn.cart_total sig={hex}\nend\nend\n"
    );
    let result = parse_changeset(&src).expect("named assert_hash must parse");
    assert_eq!(result.preconditions.len(), 1);
    match &result.preconditions[0] {
        crate::canonical::Precondition::AssertHashByName {
            name,
            expected_hash,
        } => {
            assert_eq!(name, "fn.cart_total");
            assert_eq!(expected_hash.0, [0xbb_u8; 32]);
        }
        other => panic!("expected AssertHashByName, got {other:?}"),
    }
}

// TRIANGULATE: numeric assert_exists still works (backward-compatible).
#[test]
fn parse_assert_exists_numeric_still_works() {
    let src = "\
change x
author A
base 0
requires
  assert_exists 42
end
end
";
    let result = parse_changeset(src).expect("numeric assert_exists must parse");
    let crate::canonical::Precondition::AssertExists(ae) = &result.preconditions[0] else {
        panic!("expected AssertExists, got {:?}", result.preconditions[0]);
    };
    assert_eq!(ae.node_id, NodeRef(42));
}

// ── Gap 2: Schema versioning fields ───────────────────────────────────

// Scenario: all five schema version directives in metadata section are parsed.
//   GIVEN a metadata section with all five schema version fields
//   WHEN parsed
//   THEN ParsedChangeSet carries correct versions for each field
#[test]
fn parse_all_schema_version_fields() {
    let src = "\
change test_versions
author Agent
base 0
metadata
  acl_version acl/1.0
  op_schema 1
  graph_schema 3
  core_ir_schema 2
  diagnostics_schema 1
  verification_schema 1
end
end
";
    let result = parse_changeset(src).expect("schema versions must parse");
    assert_eq!(result.acl_version, "1.0");
    assert_eq!(result.op_schema_version.as_deref(), Some("1"));
    assert_eq!(result.graph_schema_version.as_deref(), Some("3"));
    assert_eq!(result.core_ir_schema_version.as_deref(), Some("2"));
    assert_eq!(result.diagnostics_schema_version.as_deref(), Some("1"));
    assert_eq!(result.verification_schema_version.as_deref(), Some("1"));
}

// Scenario: schema version fields default to None when absent.
#[test]
fn parse_schema_versions_default_to_none_when_absent() {
    let src = "change x\nauthor A\nbase 0\nend\n";
    let result = parse_changeset(src).expect("must parse");
    assert!(result.op_schema_version.is_none());
    assert!(result.graph_schema_version.is_none());
    assert!(result.core_ir_schema_version.is_none());
    assert!(result.diagnostics_schema_version.is_none());
    assert!(result.verification_schema_version.is_none());
}

// TRIANGULATE: schema versions are carried through canonicalize_parsed.
#[test]
fn schema_versions_carried_through_canonicalize() {
    use crate::canonical::canonicalize_parsed;

    let src = "\
change test
author tester
base 0
metadata
  graph_schema 5
  core_ir_schema 3
end
end
";
    let parsed = parse_changeset(src).expect("must parse");
    let canonical = canonicalize_parsed(parsed);
    assert_eq!(canonical.graph_schema_version.as_deref(), Some("5"));
    assert_eq!(canonical.core_ir_schema_version.as_deref(), Some("3"));
    assert!(canonical.op_schema_version.is_none());
}

// Scenario: identity changeset (no ops) parses successfully.
#[test]
fn parse_identity_changeset_is_valid() {
    let src = "change x\nauthor Olivia\nbase 99\nend\n";
    let result = parse_changeset(src).expect("identity changeset must parse");
    assert!(result.changeset.ops.is_empty());
    assert_eq!(result.changeset.base_snapshot_id, SnapshotId(99));
}

// Scenario: multiple requires assertions are all captured.
#[test]
fn parse_multiple_preconditions() {
    let src = "\
change x
author Paula
base 0
requires
  assert_exists 1
  assert_exists 2
  assert_exists 3
end
end
";
    let result = parse_changeset(src).expect("multiple preconditions must parse");
    assert_eq!(result.preconditions.len(), 3);
}

// Scenario: unclosed section returns ParseError.
#[test]
fn parse_unclosed_section_returns_error() {
    let src = "change x\nauthor Quinn\nbase 0\nops\nop create_function id=fn.x\n";
    let err = parse_changeset(src).expect_err("unclosed section must error");
    assert!(
        err.contains("unclosed"),
        "error must say 'unclosed'; got: {err}"
    );
}

// TRIANGULATE: bare `create`, `set`, `add`, `infer` (no underscore suffix) map correctly.
#[test]
fn parse_bare_verb_variants() {
    let src = "change x\nauthor R\nbase 0\nop create\nop set\nop add\nop infer\nend\n";
    let result = parse_changeset(src).expect("bare verbs must parse");
    assert_eq!(
        result.changeset.ops,
        vec![
            ChangeSetOp::Create,
            ChangeSetOp::Set,
            ChangeSetOp::Add,
            ChangeSetOp::Infer,
        ]
    );
}

// Scenario: all 20 new verbs (phase 1-4 additions) parse to their own variants.
//   GIVEN one representative op for each new verb
//   WHEN parse_changeset is called
//   THEN each verb maps to its dedicated ChangeSetOp variant
#[test]
fn parse_all_new_verb_variants() {
    let src = "\
change test_new_verbs
author TestAgent
base 0
op delete target=fn.old
op disconnect source=fn.a relation=uses target=cap.b
op rename target=fn.old name=fn.new
op move target=fn.util to=module.utils
op replace target=fn.checkout.body with=@expr.v2
op bind_handler capability=payment.charge handler=handler.Stripe profile=prod
op expose target=fn.checkout as=api.checkout
op hide target=fn.internal_helper
op grant target=module.checkout capability=database.read profile=prod
op revoke target=module.checkout capability=file.write profile=prod
op derive_eq target=type.Address mode=structural
op generate_tests target=fn.checkout from=contracts
op assert_exists target=fn.checkout
op lock_behavior target=fn.checkout
op refactor_extract_function from=fn.checkout range=@range.payment to=fn.charge
op migrate_api target=fn.checkout from=sig.v1 to=sig.v2
op approve_inferred_boundary target=fn.checkout version=sig_123
op reject_inferred_boundary target=fn.checkout version=sig_124
op deprecate target=fn.old_checkout replacement=fn.checkout_v2
op annotate target=fn.checkout key=rationale value=\"Checkout must be idempotent\"
end
";
    let result = parse_changeset(src).expect("all new verbs must parse");
    assert_eq!(
        result.changeset.ops,
        vec![
            ChangeSetOp::Delete,
            ChangeSetOp::Disconnect,
            ChangeSetOp::Rename,
            ChangeSetOp::Move,
            ChangeSetOp::Replace,
            ChangeSetOp::Bind,
            ChangeSetOp::Expose,
            ChangeSetOp::Hide,
            ChangeSetOp::Grant,
            ChangeSetOp::Revoke,
            ChangeSetOp::Derive,
            ChangeSetOp::Generate,
            ChangeSetOp::Assert,
            ChangeSetOp::Lock,
            ChangeSetOp::Refactor,
            ChangeSetOp::Migrate,
            ChangeSetOp::Approve,
            ChangeSetOp::Reject,
            ChangeSetOp::Deprecate,
            ChangeSetOp::Annotate,
        ]
    );
}

// Scenario: bare new verbs (no underscore suffix) also map correctly.
#[test]
fn parse_bare_new_verb_variants() {
    let src = "\
change x
author S
base 0
op delete
op disconnect
op rename
op move
op replace
op bind
op expose
op hide
op grant
op revoke
op derive
op generate
op assert
op lock
op refactor
op migrate
op approve
op reject
op deprecate
op annotate
end
";
    let result = parse_changeset(src).expect("bare new verbs must parse");
    assert_eq!(
        result.changeset.ops,
        vec![
            ChangeSetOp::Delete,
            ChangeSetOp::Disconnect,
            ChangeSetOp::Rename,
            ChangeSetOp::Move,
            ChangeSetOp::Replace,
            ChangeSetOp::Bind,
            ChangeSetOp::Expose,
            ChangeSetOp::Hide,
            ChangeSetOp::Grant,
            ChangeSetOp::Revoke,
            ChangeSetOp::Derive,
            ChangeSetOp::Generate,
            ChangeSetOp::Assert,
            ChangeSetOp::Lock,
            ChangeSetOp::Refactor,
            ChangeSetOp::Migrate,
            ChangeSetOp::Approve,
            ChangeSetOp::Reject,
            ChangeSetOp::Deprecate,
            ChangeSetOp::Annotate,
        ]
    );
}
