// ── ail-change: canonicalization tests ───────────────────────────────────
//
// Strict TDD — RED phase.
// All tests reference types in `ail_change::canonical` that do NOT exist yet.
// Compilation failure here is the expected RED signal.

use std::collections::BTreeMap;

use ail_change::{
    canonical::{CanonicalChangeSet, canonicalize, canonicalize_parsed},
    model::{
        AssertExists, BlockHash, ChangeSet, ChangeSetMeta, ChangeSetOp, SnapshotId, Timestamp,
    },
    parser::{OpArgs, ParsedChangeSet, ParsedOp, parse_changeset},
};
use ail_core::semantic_graph::NodeRef;

// ── helpers ───────────────────────────────────────────────────────────────

fn meta(description: &str) -> ChangeSetMeta {
    ChangeSetMeta {
        author: "alice".to_string(),
        description: description.to_string(),
        timestamp: Timestamp(0),
    }
}

fn changeset(ops: Vec<ChangeSetOp>, description: &str) -> ChangeSet {
    ChangeSet {
        meta: meta(description),
        base_snapshot_id: SnapshotId(1),
        ops,
    }
}

// ── Scenario: CBOR identity ───────────────────────────────────────────────
// GIVEN a ChangeSet
// WHEN canonicalize is called twice with identical input
// THEN the CBOR encoding of both outputs is byte-identical
#[test]
fn canonical_output_is_cbor_identical_on_repeated_calls() {
    let cs = changeset(vec![ChangeSetOp::Create, ChangeSetOp::Connect], "hello");

    let a: CanonicalChangeSet = canonicalize(cs.clone());
    let b: CanonicalChangeSet = canonicalize(cs);

    let mut bytes_a = Vec::new();
    let mut bytes_b = Vec::new();
    ciborium::into_writer(&a, &mut bytes_a).expect("encode a");
    ciborium::into_writer(&b, &mut bytes_b).expect("encode b");

    assert_eq!(bytes_a, bytes_b, "canonicalize must be deterministic");
}

// ── Scenario: op ordering ─────────────────────────────────────────────────
// GIVEN a ChangeSet with ops in arbitrary order
// WHEN canonicalize is called
// THEN ops appear in canonical phase order: Create → Set/Add/Remove → Connect → Infer → Verify
#[test]
fn out_of_order_ops_are_sorted_to_canonical_phase_order() {
    let cs = changeset(
        vec![
            ChangeSetOp::Verify,  // phase 4
            ChangeSetOp::Connect, // phase 2
            ChangeSetOp::Create,  // phase 0
            ChangeSetOp::Infer,   // phase 3
            ChangeSetOp::Set,     // phase 1
        ],
        "order test",
    );

    let canonical = canonicalize(cs);
    let kinds: Vec<&ChangeSetOp> = canonical.ops.iter().map(|o| &o.kind).collect();

    assert_eq!(
        kinds,
        vec![
            &ChangeSetOp::Create,
            &ChangeSetOp::Set,
            &ChangeSetOp::Connect,
            &ChangeSetOp::Infer,
            &ChangeSetOp::Verify,
        ],
        "ops must be sorted into canonical phase order"
    );
}

// ── TRIANGULATE: stable sort preserves relative order within same phase ───
// GIVEN a ChangeSet with two ops of the same phase (Set and Add)
// WHEN canonicalize is called
// THEN their relative order is preserved (Set before Add)
#[test]
fn ops_in_same_phase_preserve_relative_order() {
    let cs = changeset(
        vec![ChangeSetOp::Set, ChangeSetOp::Add, ChangeSetOp::Remove],
        "same-phase order",
    );

    let canonical = canonicalize(cs);
    let kinds: Vec<&ChangeSetOp> = canonical.ops.iter().map(|o| &o.kind).collect();

    assert_eq!(
        kinds,
        vec![&ChangeSetOp::Set, &ChangeSetOp::Add, &ChangeSetOp::Remove],
        "stable sort must preserve intra-phase order"
    );
}

// ── Scenario: missing optional field filled with safe default ─────────────
// GIVEN a ChangeSet with an empty description
// WHEN canonicalize is called
// THEN the canonical metadata description is filled with "<no description>"
#[test]
fn empty_description_is_filled_with_default() {
    let cs = changeset(vec![], ""); // empty description

    let canonical = canonicalize(cs);

    assert_eq!(
        canonical.meta.description, "<no description>",
        "empty description must be materialized to default placeholder"
    );
}

// ── TRIANGULATE: non-empty description is preserved unchanged ─────────────
// GIVEN a ChangeSet with a non-empty description
// WHEN canonicalize is called
// THEN the canonical description equals the original
#[test]
fn non_empty_description_is_preserved() {
    let cs = changeset(vec![], "deploy auth service");

    let canonical = canonicalize(cs);

    assert_eq!(canonical.meta.description, "deploy auth service");
}

// ── Scenario: every op block carries a non-empty BlockHash ───────────────
// GIVEN a ChangeSet with one or more ops
// WHEN canonicalize is called
// THEN each CanonicalOp has a BlockHash that is NOT the all-zeros sentinel
#[test]
fn every_op_block_has_non_empty_hash() {
    let cs = changeset(
        vec![ChangeSetOp::Create, ChangeSetOp::Set, ChangeSetOp::Connect],
        "hash test",
    );

    let canonical = canonicalize(cs);

    let zero_hash = BlockHash([0u8; 32]);
    for op in &canonical.ops {
        assert_ne!(
            op.block_hash, zero_hash,
            "op {:?} must have a computed (non-zero) block hash",
            op.kind
        );
    }
}

// ── TRIANGULATE: two different op sequences produce different hashes ──────
// Forces the hash to be content-dependent (not constant).
#[test]
fn different_op_sequences_produce_different_canonical_bytes() {
    let cs_a = changeset(vec![ChangeSetOp::Create], "seq a");
    let cs_b = changeset(vec![ChangeSetOp::Connect], "seq b");

    let canonical_a = canonicalize(cs_a);
    let canonical_b = canonicalize(cs_b);

    let mut bytes_a = Vec::new();
    let mut bytes_b = Vec::new();
    ciborium::into_writer(&canonical_a, &mut bytes_a).expect("encode a");
    ciborium::into_writer(&canonical_b, &mut bytes_b).expect("encode b");

    assert_ne!(
        bytes_a, bytes_b,
        "different changesets must produce different canonical bytes"
    );
}

// ── Scenario: new verbs sort to their correct phases ─────────────────────
// GIVEN a mix of new verbs from phase 1-4
// WHEN canonicalize is called
// THEN they appear sorted into canonical phase order
#[test]
fn new_verbs_sort_to_correct_canonical_phases() {
    let cs = changeset(
        vec![
            ChangeSetOp::Verify,     // phase 4
            ChangeSetOp::Derive,     // phase 3
            ChangeSetOp::Grant,      // phase 2
            ChangeSetOp::Delete,     // phase 1
            ChangeSetOp::Annotate,   // phase 4
            ChangeSetOp::Expose,     // phase 2
            ChangeSetOp::Disconnect, // phase 1
            ChangeSetOp::Generate,   // phase 3
            ChangeSetOp::Create,     // phase 0
            ChangeSetOp::Assert,     // phase 4
            ChangeSetOp::Rename,     // phase 1
            ChangeSetOp::Bind,       // phase 2
        ],
        "new verb phase ordering",
    );

    let canonical = canonicalize(cs);
    let kinds: Vec<&ChangeSetOp> = canonical.ops.iter().map(|o| &o.kind).collect();

    // phase 0 first
    assert_eq!(kinds[0], &ChangeSetOp::Create);
    // phase 1 ops next (Delete, Disconnect, Rename — stable order from input)
    assert_eq!(kinds[1], &ChangeSetOp::Delete);
    assert_eq!(kinds[2], &ChangeSetOp::Disconnect);
    assert_eq!(kinds[3], &ChangeSetOp::Rename);
    // phase 2 ops (Grant, Expose, Bind — stable order from input)
    assert_eq!(kinds[4], &ChangeSetOp::Grant);
    assert_eq!(kinds[5], &ChangeSetOp::Expose);
    assert_eq!(kinds[6], &ChangeSetOp::Bind);
    // phase 3 ops (Derive, Generate — stable order from input)
    assert_eq!(kinds[7], &ChangeSetOp::Derive);
    assert_eq!(kinds[8], &ChangeSetOp::Generate);
    // phase 4 ops last (Verify, Annotate, Assert — stable order from input)
    assert_eq!(kinds[9], &ChangeSetOp::Verify);
    assert_eq!(kinds[10], &ChangeSetOp::Annotate);
    assert_eq!(kinds[11], &ChangeSetOp::Assert);
}

// ── CanonicalMeta is accessible and carries all original fields ───────────
// Ensures the canonical output preserves author and timestamp alongside
// the materialized description.
#[test]
fn canonical_meta_preserves_author_and_timestamp() {
    let cs = ChangeSet {
        meta: ChangeSetMeta {
            author: "bob".to_string(),
            description: "check meta".to_string(),
            timestamp: Timestamp(42),
        },
        base_snapshot_id: SnapshotId(7),
        ops: vec![],
    };

    let canonical = canonicalize(cs);

    assert_eq!(canonical.meta.author, "bob");
    assert_eq!(canonical.meta.timestamp, Timestamp(42));
    assert_eq!(canonical.base_snapshot_id, SnapshotId(7));
}

// ═══════════════════════════════════════════════════════════════════════════
// Phase B: canonicalize_parsed + default materialization (G22)
// ═══════════════════════════════════════════════════════════════════════════

// ── helpers ───────────────────────────────────────────────────────────────

fn make_parsed_op(kind: ChangeSetOp, verb: &str, args: &[(&str, &str)]) -> ParsedOp {
    let mut map = BTreeMap::new();
    for (k, v) in args {
        map.insert(k.to_string(), v.to_string());
    }
    ParsedOp {
        kind,
        verb: verb.to_string(),
        args: map,
    }
}

fn minimal_parsed_changeset(parsed_ops: Vec<ParsedOp>) -> ParsedChangeSet {
    use ail_change::parser::{ApprovalRequirements, ChangeComposition, ExpectClaims};
    ParsedChangeSet {
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

// ── Scenario: canonicalize_parsed carries preconditions ───────────────────
// GIVEN a ParsedChangeSet with 2 preconditions
// WHEN canonicalize_parsed is called
// THEN CanonicalChangeSet.preconditions has 2 entries
#[test]
fn canonicalize_parsed_carries_preconditions() {
    use ail_change::canonical::Precondition;
    let mut pcs = minimal_parsed_changeset(vec![]);
    pcs.preconditions = vec![
        Precondition::AssertExists(AssertExists {
            node_id: NodeRef(1),
        }),
        Precondition::AssertExists(AssertExists {
            node_id: NodeRef(2),
        }),
    ];

    let canonical = canonicalize_parsed(pcs);

    assert_eq!(
        canonical.preconditions.len(),
        2,
        "canonicalize_parsed must carry preconditions"
    );
}

// ── Scenario: create_function without visibility gets private default ──────
// GIVEN a ParsedOp with verb=create_function and no visibility arg
// WHEN canonicalize_parsed is called
// THEN the canonical op args contain visibility=private
#[test]
fn create_function_without_visibility_gets_private_default() {
    let pcs = minimal_parsed_changeset(vec![make_parsed_op(
        ChangeSetOp::Create,
        "create_function",
        &[("id", "fn.x")],
    )]);

    let canonical = canonicalize_parsed(pcs);

    let op = &canonical.ops[0];
    assert_eq!(
        op.args.get("visibility"),
        Some(&"private".to_string()),
        "create_function must default visibility=private"
    );
    assert_eq!(op.args.get("id"), Some(&"fn.x".to_string()));
}

// ── TRIANGULATE: create_function with explicit visibility is NOT overridden ─
#[test]
fn create_function_with_explicit_visibility_is_preserved() {
    let pcs = minimal_parsed_changeset(vec![make_parsed_op(
        ChangeSetOp::Create,
        "create_function",
        &[("id", "fn.pub"), ("visibility", "public")],
    )]);

    let canonical = canonicalize_parsed(pcs);

    assert_eq!(
        canonical.ops[0].args.get("visibility"),
        Some(&"public".to_string()),
        "explicit visibility must not be overridden"
    );
}

// ── Scenario: create_type without visibility gets private default ──────────
#[test]
fn create_type_without_visibility_gets_private_default() {
    let pcs = minimal_parsed_changeset(vec![make_parsed_op(
        ChangeSetOp::Create,
        "create_type",
        &[("id", "type.Foo")],
    )]);

    let canonical = canonicalize_parsed(pcs);
    assert_eq!(
        canonical.ops[0].args.get("visibility"),
        Some(&"private".to_string())
    );
}

// ── Scenario: CanonicalOp carries verb and args ────────────────────────────
// GIVEN a ParsedOp with verb=add_param and args={target, name, type}
// WHEN canonicalize_parsed is called
// THEN the canonical op has verb=add_param and the same args
#[test]
fn canonical_op_carries_verb_and_args() {
    let pcs = minimal_parsed_changeset(vec![make_parsed_op(
        ChangeSetOp::Add,
        "add_param",
        &[("target", "fn.x"), ("name", "a"), ("type", "Int")],
    )]);

    let canonical = canonicalize_parsed(pcs);

    let op = &canonical.ops[0];
    assert_eq!(op.verb, "add_param");
    assert_eq!(op.args.get("target"), Some(&"fn.x".to_string()));
    assert_eq!(op.args.get("name"), Some(&"a".to_string()));
    assert_eq!(op.args.get("type"), Some(&"Int".to_string()));
}

// ── Scenario: acl_version is carried into CanonicalChangeSet ─────────────
#[test]
fn canonicalize_parsed_carries_acl_version() {
    let mut pcs = minimal_parsed_changeset(vec![]);
    pcs.acl_version = "1.0".to_string();

    let canonical = canonicalize_parsed(pcs);

    assert_eq!(canonical.acl_version, "1.0");
}

// ── Scenario: canonicalize_parsed produces same phase ordering as canonicalize ─
#[test]
fn canonicalize_parsed_respects_phase_ordering() {
    let pcs = minimal_parsed_changeset(vec![
        make_parsed_op(ChangeSetOp::Verify, "verify", &[]),
        make_parsed_op(ChangeSetOp::Connect, "connect", &[]),
        make_parsed_op(ChangeSetOp::Create, "create_function", &[("id", "fn.x")]),
        make_parsed_op(ChangeSetOp::Infer, "infer_boundary", &[("target", "fn.x")]),
        make_parsed_op(
            ChangeSetOp::Set,
            "set_return",
            &[("target", "fn.x"), ("type", "Unit")],
        ),
    ]);

    let canonical = canonicalize_parsed(pcs);
    let kinds: Vec<&ChangeSetOp> = canonical.ops.iter().map(|o| &o.kind).collect();

    assert_eq!(
        kinds,
        vec![
            &ChangeSetOp::Create,
            &ChangeSetOp::Set,
            &ChangeSetOp::Connect,
            &ChangeSetOp::Infer,
            &ChangeSetOp::Verify,
        ]
    );
}

// ── Scenario: round-trip parse → canonicalize_parsed preserves args ────────
// Integration: parse an ACL string, then canonicalize_parsed, verify args.
#[test]
fn parse_then_canonicalize_parsed_preserves_args() {
    let src = "\
change x
author Alice
base 1
op create_function id=fn.checkout
op add_param target=fn.checkout name=cartId type=CartId
op verify
end
";
    let parsed = parse_changeset(src).expect("must parse");
    let canonical = canonicalize_parsed(parsed);

    assert_eq!(canonical.ops.len(), 3);
    // create_function should have visibility=private materialized.
    let create_op = canonical
        .ops
        .iter()
        .find(|o| o.verb == "create_function")
        .unwrap();
    assert_eq!(
        create_op.args.get("visibility"),
        Some(&"private".to_string())
    );
    assert_eq!(create_op.args.get("id"), Some(&"fn.checkout".to_string()));
}
