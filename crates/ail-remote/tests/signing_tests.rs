// ── ail-remote / signing_tests ────────────────────────────────────────────
//
// Integration tests for Ed25519 signing envelopes.
//
// Spec scenarios covered:
//   - SignedContextSlice: full sign + verify integration
//   - RemoteChangeSet: full sign + verify integration
//   - AgentIdentity: CBOR roundtrip

use ail_change::canonical::{CanonicalChangeSet, CanonicalMeta};
use ail_change::model::{SnapshotId, Timestamp};
use ail_context::dto::ContextResponse;
use ail_remote::identity::{AgentIdentity, AgentKeypair, SigningError};
use ail_remote::signing::{RemoteChangeSet, SignedContextSlice};
use ail_storage::codec::{CborCodec, ContentCodec};
use ail_storage::graph::SnapshotEnvelope;
use ail_storage::object::ObjectId;

// ── helpers ───────────────────────────────────────────────────────────────

fn make_snapshot(tag: &[u8]) -> SnapshotEnvelope {
    let id = ObjectId::from_bytes(tag);
    SnapshotEnvelope {
        id,
        graph_root_hash: id,
        parent_id: None,
        applied_change_id: None,
        created_at: 42_000,
        verification_report_hash: None,
    }
}

fn make_context_response(tag: &[u8]) -> ContextResponse {
    use ail_context::ContextQuery;
    use ail_context::dto::{CONTEXT_SCHEMA_V1, ResponseLimits};
    let codec = CborCodec;
    let snapshot = make_snapshot(tag);
    let structured = Vec::new();
    let query = ContextQuery::Graph {
        scope: ail_context::QueryScope::Full,
        budget: usize::MAX,
    };
    let query_bytes = codec.encode(&query).expect("encode query");
    let query_hash = *blake3::hash(&query_bytes).as_bytes();
    let structured_bytes = codec.encode(&structured).expect("encode structured");
    let context_hash = *blake3::hash(&structured_bytes).as_bytes();
    let bytes_used = structured_bytes.len();
    ContextResponse {
        schema: CONTEXT_SCHEMA_V1.to_string(),
        graph_root_hash: snapshot.graph_root_hash,
        query_hash,
        context_hash,
        freshness: snapshot.created_at,
        generated_at: 0,
        snapshot,
        structured,
        summary: "integration test".to_string(),
        redacted: false,
        redaction_state: ail_context::RedactionState::None,
        redaction_policy: None,
        truncated: false,
        limits: ResponseLimits {
            budget_bytes: usize::MAX,
            bytes_used,
            truncated: false,
            omitted_sections: Vec::new(),
        },
        history_entries: Vec::new(),
        freshness_status: ail_context::FreshnessStatus::Fresh,
        provenance: ail_context::ProvenanceBlock::default(),
        repair_options: Vec::new(),
    }
}

fn make_changeset(base: u64) -> CanonicalChangeSet {
    CanonicalChangeSet {
        meta: CanonicalMeta {
            author: "integration-agent".to_string(),
            description: "integration test changeset".to_string(),
            timestamp: Timestamp(base),
        },
        base_snapshot_id: SnapshotId(base),
        preconditions: vec![],
        ops: vec![],
    }
}

// ── signed_context_slice_sign_verify_integration ──────────────────────────
// Spec scenario: `SignedContextSlice` full sign + verify integration.
//   GIVEN a ContextResponse and an AgentKeypair
//   WHEN sign() is called and then verify() is called
//   THEN verify returns Ok(())
#[test]
fn signed_context_slice_sign_verify_integration() {
    let kp = AgentKeypair::generate();
    let response = make_context_response(b"integration-snap-a");
    let slice = SignedContextSlice::sign(response, &kp).expect("sign must succeed");
    slice
        .verify()
        .expect("verify must succeed for a freshly signed slice");
}

// ── signed_context_slice_cbor_roundtrip ───────────────────────────────────
// SignedContextSlice survives CBOR encode + decode with signature intact.
#[test]
fn signed_context_slice_cbor_roundtrip() {
    let codec = CborCodec;
    let kp = AgentKeypair::generate();
    let response = make_context_response(b"integration-snap-b");
    let slice = SignedContextSlice::sign(response, &kp).expect("sign must succeed");

    let encoded = codec.encode(&slice).expect("encode slice");
    let decoded: SignedContextSlice = codec.decode(&encoded).expect("decode slice");

    // Verify the decoded slice — signature must survive roundtrip.
    decoded
        .verify()
        .expect("decoded slice signature must still be valid");
    assert_eq!(
        decoded.signer, slice.signer,
        "signer identity must survive roundtrip"
    );
}

// ── remote_changeset_sign_verify_integration ──────────────────────────────
// Spec scenario: `RemoteChangeSet` full sign + verify integration.
//   GIVEN a CanonicalChangeSet and an AgentKeypair
//   WHEN sign() is called and verify_signature() is called
//   THEN verify_signature returns Ok(())
#[test]
fn remote_changeset_sign_verify_integration() {
    let kp = AgentKeypair::generate();
    let cs = make_changeset(1);
    let rcs = RemoteChangeSet::sign(cs, &kp).expect("sign must succeed");
    rcs.verify_signature()
        .expect("verify must succeed for a freshly signed RemoteChangeSet");
}

// ── remote_changeset_cbor_roundtrip ───────────────────────────────────────
// RemoteChangeSet survives CBOR encode + decode with signature intact.
#[test]
fn remote_changeset_cbor_roundtrip() {
    let codec = CborCodec;
    let kp = AgentKeypair::generate();
    let cs = make_changeset(2);
    let rcs = RemoteChangeSet::sign(cs, &kp).expect("sign must succeed");

    let encoded = codec.encode(&rcs).expect("encode rcs");
    let decoded: RemoteChangeSet = codec.decode(&encoded).expect("decode rcs");

    decoded
        .verify_signature()
        .expect("decoded rcs signature must still be valid");
    assert_eq!(
        decoded.agent, rcs.agent,
        "agent identity must survive roundtrip"
    );
}

// ── agent_identity_cbor_roundtrip ─────────────────────────────────────────
// Spec scenario: AgentIdentity CBOR roundtrip.
//   GIVEN an AgentIdentity derived from a generated keypair
//   WHEN serialized to CBOR and deserialized
//   THEN the result equals the original identity
#[test]
fn agent_identity_cbor_roundtrip() {
    let codec = CborCodec;
    let kp = AgentKeypair::generate();
    let identity = kp.identity();

    let encoded = codec.encode(&identity).expect("encode identity");
    let decoded: AgentIdentity = codec.decode(&encoded).expect("decode identity");

    assert_eq!(
        decoded, identity,
        "AgentIdentity must survive CBOR roundtrip"
    );
}

// ── agent_identity_with_label_cbor_roundtrip ──────────────────────────────
// AgentIdentity with optional label survives roundtrip.
#[test]
fn agent_identity_with_label_cbor_roundtrip() {
    let codec = CborCodec;
    let kp = AgentKeypair::generate();
    let mut identity = kp.identity();
    identity.label = Some("integration-agent-label".to_string());

    let encoded = codec.encode(&identity).expect("encode identity with label");
    let decoded: AgentIdentity = codec.decode(&encoded).expect("decode identity");

    assert_eq!(
        decoded, identity,
        "labeled AgentIdentity must survive CBOR roundtrip"
    );
}

// ── remote_changeset_tampered_signature_rejected ──────────────────────────
// Integration guard: a RemoteChangeSet with a manually corrupted signature fails.
#[test]
fn remote_changeset_tampered_signature_rejected() {
    let kp = AgentKeypair::generate();
    let cs = make_changeset(3);
    let mut rcs = RemoteChangeSet::sign(cs, &kp).expect("sign must succeed");

    // Corrupt the first byte of the signature.
    rcs.signature[0] = rcs.signature[0].wrapping_add(1);

    let result = rcs.verify_signature();
    assert_eq!(
        result,
        Err(SigningError::SignatureInvalid),
        "corrupted signature must return SignatureInvalid"
    );
}
