// ── ail-remote::signing ───────────────────────────────────────────────────
//
// Ed25519-signed envelopes for `ContextResponse` and `CanonicalChangeSet`.
//
// # Signing payload construction
//
// Both `SignedContextSlice` and `RemoteChangeSet` use deterministic CBOR
// (via `CborCodec`) to serialise their signing payload tuples.  The payloads
// are intentionally minimal: only the fields that must be tamper-evident are
// included.
//
// `SignedContextSlice` payload: `CBOR([snapshot_id_bytes, context_hash_bytes, structured_cbor])`
//   - `snapshot_id_bytes`: the 32-byte BLAKE3 id of the snapshot (`response.snapshot.id`)
//   - `context_hash_bytes`: `response.context_hash` (`[u8; 32]`)
//   - `structured_cbor`: `CborCodec.encode(&response.structured)` (`Vec<u8>`)
//
// `RemoteChangeSet` payload: `CBOR([base_snapshot_id_bytes, ops_cbor])`
//   - `base_snapshot_id_bytes`: `cs.base_snapshot_id.0.to_le_bytes()` as `[u8; 8]`
//   - `ops_cbor`: `CborCodec.encode(&cs.ops)` (`Vec<u8>`)
//
// # Why CBOR-of-CBOR for structured / ops?
//
// Pre-encoding the inner collection to bytes before wrapping in the outer
// CBOR tuple lets us avoid generic tuple serialisation issues while keeping
// determinism: `CborCodec` already guarantees stable output for fixed-layout
// serde types, and the inner bytes are a concrete `Vec<u8>` — always stable.

use ail_change::canonical::CanonicalChangeSet;
use ail_context::dto::ContextResponse;
use ail_storage::codec::{CborCodec, ContentCodec};

use crate::identity::{AgentIdentity, AgentKeypair, SigningError};

// ── SignedContextSlice ────────────────────────────────────────────────────

/// A `ContextResponse` envelope with an Ed25519 signature from the producing agent.
///
/// The signature covers `CBOR([snapshot_id_bytes, context_hash_bytes, structured_cbor])`
/// making the snapshot identity, context hash, and structured node list tamper-evident.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignedContextSlice {
    /// The signed context response.
    pub response: ContextResponse,
    /// The agent that signed this slice.
    pub signer: AgentIdentity,
    /// 64-byte Ed25519 signature over the signing payload.
    #[serde(with = "crate::identity::sig_serde")]
    pub signature: [u8; 64],
}

use serde::{Deserialize, Serialize};

impl SignedContextSlice {
    /// Sign `response` with `keypair`, producing a `SignedContextSlice`.
    ///
    /// # Errors
    ///
    /// Returns `SigningError::SerializationError` if the CBOR encoding of the
    /// signing payload fails (should not happen with well-formed inputs).
    pub fn sign(response: ContextResponse, keypair: &AgentKeypair) -> Result<Self, SigningError> {
        let payload = Self::signing_payload(&response)?;
        let signature = keypair.sign_bytes(&payload);
        let signer = keypair.identity();
        Ok(Self {
            response,
            signer,
            signature,
        })
    }

    /// Verify the signature against the response fields using the stored signer identity.
    ///
    /// # Errors
    ///
    /// Returns `SigningError::SignatureInvalid` if verification fails or the
    /// CBOR payload cannot be reconstructed.
    pub fn verify(&self) -> Result<(), SigningError> {
        let payload = Self::signing_payload(&self.response)?;
        self.signer.verify_bytes(&payload, &self.signature)
    }

    /// Build the deterministic CBOR signing payload for a `ContextResponse`.
    ///
    /// Layout: `CBOR([snapshot_id_bytes: [u8;32], context_hash: [u8;32], structured_cbor: Vec<u8>])`
    fn signing_payload(response: &ContextResponse) -> Result<Vec<u8>, SigningError> {
        let codec = CborCodec;

        // Inner structured bytes: deterministic CBOR of the node list.
        let structured_cbor = codec
            .encode(&response.structured)
            .map_err(|e| SigningError::SerializationError(e.to_string()))?;

        // Outer payload: tuple of (snapshot_id_raw, context_hash, structured_cbor).
        let snapshot_id_bytes: &[u8; 32] = response.snapshot.id.as_bytes();
        let context_hash: &[u8; 32] = &response.context_hash;

        let payload_tuple = (snapshot_id_bytes, context_hash, structured_cbor);
        codec
            .encode(&payload_tuple)
            .map_err(|e| SigningError::SerializationError(e.to_string()))
    }
}

// ── RemoteChangeSet ───────────────────────────────────────────────────────

/// A `CanonicalChangeSet` with an Ed25519 signature from the submitting agent.
///
/// The signature covers `CBOR([base_snapshot_id_bytes: [u8;8], ops_cbor: Vec<u8>])`
/// making the base snapshot and operations tamper-evident.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteChangeSet {
    /// The signed changeset.
    pub changeset: CanonicalChangeSet,
    /// The agent that signed this changeset.
    pub agent: AgentIdentity,
    /// 64-byte Ed25519 signature over the signing payload.
    #[serde(with = "crate::identity::sig_serde")]
    pub signature: [u8; 64],
}

impl RemoteChangeSet {
    /// Sign `changeset` with `keypair`, producing a `RemoteChangeSet`.
    ///
    /// # Errors
    ///
    /// Returns `SigningError::SerializationError` if the CBOR encoding fails.
    pub fn sign(
        changeset: CanonicalChangeSet,
        keypair: &AgentKeypair,
    ) -> Result<Self, SigningError> {
        let payload = Self::signing_payload(&changeset)?;
        let signature = keypair.sign_bytes(&payload);
        let agent = keypair.identity();
        Ok(Self {
            changeset,
            agent,
            signature,
        })
    }

    /// Verify the signature against the changeset fields using the stored agent identity.
    ///
    /// # Errors
    ///
    /// Returns `SigningError::SignatureInvalid` if verification fails or the
    /// payload cannot be reconstructed.
    pub fn verify_signature(&self) -> Result<(), SigningError> {
        let payload = Self::signing_payload(&self.changeset)?;
        self.agent.verify_bytes(&payload, &self.signature)
    }

    /// Build the deterministic CBOR signing payload for a `CanonicalChangeSet`.
    ///
    /// Layout: `CBOR([base_snapshot_id_bytes: [u8;8], ops_cbor: Vec<u8>])`
    fn signing_payload(cs: &CanonicalChangeSet) -> Result<Vec<u8>, SigningError> {
        let codec = CborCodec;

        // Inner ops bytes: deterministic CBOR of the ops list.
        let ops_cbor = codec
            .encode(&cs.ops)
            .map_err(|e| SigningError::SerializationError(e.to_string()))?;

        // base_snapshot_id as little-endian bytes.
        let base_snapshot_id_bytes: [u8; 8] = cs.base_snapshot_id.0.to_le_bytes();

        let payload_tuple = (base_snapshot_id_bytes, ops_cbor);
        codec
            .encode(&payload_tuple)
            .map_err(|e| SigningError::SerializationError(e.to_string()))
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use ail_change::canonical::{CanonicalChangeSet, CanonicalMeta};
    use ail_change::model::{SnapshotId, Timestamp};
    use ail_context::dto::ContextResponse;
    use ail_storage::graph::SnapshotEnvelope;
    use ail_storage::object::ObjectId;

    use super::*;
    use crate::identity::AgentKeypair;

    fn make_snapshot() -> SnapshotEnvelope {
        let id = ObjectId::from_bytes(b"test-signing-snap");
        SnapshotEnvelope {
            id,
            graph_root_hash: id,
            parent_id: None,
            applied_change_id: None,
            created_at: 1_000,
            verification_report_hash: None,
            ..Default::default()
        }
    }

    fn make_response() -> ContextResponse {
        use ail_context::dto::{CONTEXT_SCHEMA_V1, ResponseLimits};
        use ail_context::{ContextQuery, QueryScope};
        let snapshot = make_snapshot();
        let structured = Vec::new();
        let codec = CborCodec;
        let query = ContextQuery::Graph {
            scope: QueryScope::Full,
            budget: ail_context::QueryBudget::default(),
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
            summary: String::new(),
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
            impact_info: None,
            refactor_info: None,
        }
    }

    fn make_changeset() -> CanonicalChangeSet {
        CanonicalChangeSet {
            meta: CanonicalMeta {
                author: "test-agent".to_string(),
                description: "test change".to_string(),
                timestamp: Timestamp(0),
            },
            base_snapshot_id: SnapshotId(0),
            preconditions: vec![],
            ops: vec![],
            ..Default::default()
        }
    }

    // ── signed_context_slice_verify_succeeds ──────────────────────────────
    // Task 3.3 / Spec: sign + verify roundtrip succeeds.
    #[test]
    fn signed_context_slice_verify_succeeds() {
        let kp = AgentKeypair::generate();
        let response = make_response();
        let slice = SignedContextSlice::sign(response, &kp).expect("sign must succeed");
        slice.verify().expect("verify must succeed for valid slice");
    }

    // ── tampered_structured_fails_verification ────────────────────────────
    // Task 3.3 / Spec: tampered structured field returns SignatureInvalid.
    #[test]
    fn tampered_structured_fails_verification() {
        use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef};
        let kp = AgentKeypair::generate();
        let response = make_response();
        let mut slice = SignedContextSlice::sign(response, &kp).expect("sign must succeed");
        // Tamper: inject a node into the structured list after signing.
        slice
            .response
            .structured
            .push(GraphNode::new(NodeRef(99), NodeKind::Module, "injected"));
        let result = slice.verify();
        assert_eq!(
            result,
            Err(SigningError::SignatureInvalid),
            "tampered structured must return SignatureInvalid"
        );
    }

    // ── wrong_signer_identity_fails_verification ──────────────────────────
    // Task 3.3 / Spec: wrong identity fails verification.
    #[test]
    fn wrong_signer_identity_fails_verification() {
        let kp_a = AgentKeypair::generate();
        let kp_b = AgentKeypair::generate();
        let response = make_response();
        let mut slice = SignedContextSlice::sign(response, &kp_a).expect("sign must succeed");
        // Replace signer with a different identity.
        slice.signer = kp_b.identity();
        let result = slice.verify();
        assert_eq!(
            result,
            Err(SigningError::SignatureInvalid),
            "wrong identity must return SignatureInvalid"
        );
    }

    // ── remote_changeset_sign_verify_succeeds ─────────────────────────────
    // Task 3.4 / Spec: RemoteChangeSet sign + verify roundtrip succeeds.
    #[test]
    fn remote_changeset_sign_verify_succeeds() {
        let kp = AgentKeypair::generate();
        let cs = make_changeset();
        let rcs = RemoteChangeSet::sign(cs, &kp).expect("sign must succeed");
        rcs.verify_signature()
            .expect("verify must succeed for valid RemoteChangeSet");
    }

    // ── tampered_ops_fails_verification ───────────────────────────────────
    // Task 3.4 / Spec: tampered ops returns SignatureInvalid.
    #[test]
    fn tampered_ops_fails_verification() {
        use ail_change::canonical::{CanonicalOp, OpPayload};
        use ail_change::model::{BlockHash, ChangeSetOp};
        let kp = AgentKeypair::generate();
        let cs = make_changeset();
        let mut rcs = RemoteChangeSet::sign(cs, &kp).expect("sign must succeed");
        // Inject a spurious op after signing.
        rcs.changeset.ops.push(CanonicalOp {
            kind: ChangeSetOp::Create,
            payload: OpPayload::Noop,
            block_hash: BlockHash([0u8; 32]),
            ..Default::default()
        });
        let result = rcs.verify_signature();
        assert_eq!(
            result,
            Err(SigningError::SignatureInvalid),
            "tampered ops must return SignatureInvalid"
        );
    }
}
