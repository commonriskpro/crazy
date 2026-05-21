// ── ail-change: canonicalization tests ───────────────────────────────────
//
// Strict TDD — RED phase.
// All tests reference types in `ail_change::canonical` that do NOT exist yet.
// Compilation failure here is the expected RED signal.

use ail_change::{
    canonical::{CanonicalChangeSet, canonicalize},
    model::{BlockHash, ChangeSet, ChangeSetMeta, ChangeSetOp, SnapshotId, Timestamp},
};

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
