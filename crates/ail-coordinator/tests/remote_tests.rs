// ── ail-coordinator remote integration tests ──────────────────────────────
//
// Tests for `Coordinator::verify_remote_submission()`.
//
// # Spec coverage
//
// | Test | Spec scenario |
// |------|---------------|
// | `valid_remote_submission_applies` | Valid signed submission is processed by coordinator |
// | `invalid_signature_rejected_before_submit` | Invalid signature is rejected before coordinator submit |
// | `valid_sig_stale_base_triggers_rebase` | Valid signature with stale base triggers rebase |
// | `local_submit_works_after_remote_submission` | Local submit still works after verify_remote_submission addition |
// | `remote_exchange_push_then_pull_returns_same_bundle` | PullBundle returns a previously accepted bundle |
// | `remote_exchange_reports_missing_pulled_bundle` | PullBundle returns BundleMissing for unknown roots |
// | `remote_exchange_rejects_invalid_bundle_integrity` | Invalid bundles are rejected and not stored |

use ail_change::{
    canonical::{CanonicalChangeSet, CanonicalMeta, CanonicalOp, OpPayload},
    model::{BlockHash, ChangeSetOp, SnapshotId, Timestamp},
};
use ail_coordinator::coordinator::{Coordinator, CoordinatorOutcome};
use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_remote::{
    AgentKeypair, ObjectBundle, RemoteChangeSet, RemoteError, RemoteExchangeRequest,
    RemoteExchangeResponse, RemoteSignerPolicy, RemoteSignerRejectionReason,
    RemoteSubmissionOutcome, SignerTrustTier, TrustedRemoteSigner,
};
use ail_storage::object::ObjectId;
use std::collections::BTreeMap;

// ── helpers ───────────────────────────────────────────────────────────────

fn empty_graph() -> SemanticGraph {
    SemanticGraph {
        nodes: vec![],
        edges: vec![],
    }
}

fn dummy_hash() -> BlockHash {
    BlockHash([0u8; 32])
}

fn meta(author: &str) -> CanonicalMeta {
    CanonicalMeta {
        author: author.into(),
        description: "remote integration test changeset".into(),
        timestamp: Timestamp(0),
    }
}

fn create_node_op(node_ref: u32, name: &str) -> CanonicalOp {
    CanonicalOp {
        kind: ChangeSetOp::Create,
        payload: OpPayload::CreateNode(Box::new(GraphNode::new(
            NodeRef(node_ref),
            NodeKind::Function,
            name,
        ))),
        block_hash: dummy_hash(),
        ..Default::default()
    }
}

fn cs(base: u64, author: &str, ops: Vec<CanonicalOp>) -> CanonicalChangeSet {
    CanonicalChangeSet {
        meta: meta(author),
        base_snapshot_id: SnapshotId(base),
        preconditions: vec![],
        ops,
        ..Default::default()
    }
}

fn signed_rcs(
    base: u64,
    author: &str,
    ops: Vec<CanonicalOp>,
    keypair: &AgentKeypair,
) -> RemoteChangeSet {
    let changeset = cs(base, author, ops);
    RemoteChangeSet::sign(changeset, keypair).expect("sign must succeed")
}

fn coordinator_allowing(keypair: &AgentKeypair) -> Coordinator {
    let identity = keypair.identity();
    let policy =
        RemoteSignerPolicy::from_allowed_signers(vec![TrustedRemoteSigner::from_identity(
            &identity,
            SignerTrustTier::Trusted,
            Some("remote_agent".to_string()),
        )]);
    Coordinator::with_remote_signer_policy(SnapshotId(0), empty_graph(), policy)
}

fn bundle_from_bytes(bytes: &[u8]) -> ObjectBundle {
    let bytes = bytes.to_vec();
    let root = ObjectId::from_bytes(&bytes);
    let mut objects = BTreeMap::new();
    objects.insert(root, bytes);
    ObjectBundle::new(root, objects)
}

// ── Task 6.1a: Valid signed submission → CoordinatorOutcome::Applied ──────
//
// Spec: Valid signed submission is processed by coordinator
//   GIVEN a RemoteChangeSet with a valid signature and base matching coordinator live snapshot
//   WHEN coordinator.verify_remote_submission(rcs) is called
//   THEN it returns Ok(CoordinatorOutcome::Applied { .. })
#[tokio::test]
async fn valid_remote_submission_applies() {
    let kp = AgentKeypair::generate();
    let coord = coordinator_allowing(&kp);

    let rcs = signed_rcs(
        0,
        "remote_agent",
        vec![create_node_op(10, "fn.remote_fn")],
        &kp,
    );
    let result = coord.verify_remote_submission(rcs).await;

    assert!(
        matches!(
            result,
            Ok(CoordinatorOutcome::Applied {
                applied_snapshot_id: SnapshotId(1)
            })
        ),
        "valid signed submission must apply and advance snapshot; got {result:?}"
    );
}

#[tokio::test]
async fn valid_signature_from_disallowed_signer_is_rejected_before_submit() {
    let coord = Coordinator::new(SnapshotId(0), empty_graph());
    let kp = AgentKeypair::generate();
    let identity = kp.identity();

    let rcs = signed_rcs(
        0,
        "remote_agent",
        vec![create_node_op(13, "fn.disallowed")],
        &kp,
    );
    let result = coord.verify_remote_submission(rcs).await;

    match result {
        Err(RemoteError::SignerRejected(rejection)) => {
            assert_eq!(rejection.public_key, identity.public_key);
            assert_eq!(
                rejection.reason,
                RemoteSignerRejectionReason::SignerNotAllowed
            );
        }
        other => panic!("disallowed signer must be rejected distinctly; got {other:?}"),
    }

    let local_result = coord
        .submit(cs(
            0,
            "local_agent",
            vec![create_node_op(14, "fn.after_policy_reject")],
        ))
        .await;
    assert!(
        matches!(
            local_result,
            CoordinatorOutcome::Applied {
                applied_snapshot_id: SnapshotId(1)
            }
        ),
        "live snapshot must still be 0 after policy rejection; got {local_result:?}"
    );
}

// ── Task 6.1b: Invalid signature rejected before submit ───────────────────
//
// Spec: Invalid signature is rejected before coordinator submit
//   GIVEN a RemoteChangeSet with a corrupted signature
//   WHEN coordinator.verify_remote_submission(rcs) is called
//   THEN it returns Err(RemoteError::SignatureInvalid) and live snapshot does NOT advance
#[tokio::test]
async fn invalid_signature_rejected_before_submit() {
    let kp = AgentKeypair::generate();
    let coord = coordinator_allowing(&kp);

    let mut rcs = signed_rcs(
        0,
        "remote_agent",
        vec![create_node_op(11, "fn.tampered")],
        &kp,
    );

    // Corrupt the signature — flip all bits in the first byte.
    rcs.signature[0] ^= 0xFF;

    let result = coord.verify_remote_submission(rcs).await;
    assert_eq!(
        result,
        Err(RemoteError::SignatureInvalid),
        "corrupted signature must return RemoteError::SignatureInvalid; got {result:?}"
    );

    // Verify that the live snapshot has NOT advanced — attempt a local submit
    // at base SnapshotId(0), which must still be the live snapshot.
    let local_cs = cs(
        0,
        "local_agent",
        vec![create_node_op(12, "fn.after_reject")],
    );
    let local_result = coord.submit(local_cs).await;
    assert!(
        matches!(
            local_result,
            CoordinatorOutcome::Applied {
                applied_snapshot_id: SnapshotId(1)
            }
        ),
        "live snapshot must still be 0 after rejected remote submission; got {local_result:?}"
    );
}

// ── Task 6.1c: Valid signature + stale base → RebaseApplied ──────────────
//
// Spec: Valid signature with stale base triggers rebase
//   GIVEN coordinator live snapshot = SnapshotId(1) (agent A applied first)
//   AND   a RemoteChangeSet with valid sig but base_snapshot_id = SnapshotId(0)
//         and non-conflicting ops (different NodeRef)
//   WHEN coordinator.verify_remote_submission(rcs) is called
//   THEN it returns Ok(CoordinatorOutcome::RebaseApplied { .. })
#[tokio::test]
async fn valid_sig_stale_base_triggers_rebase() {
    let kp = AgentKeypair::generate();
    let coord = coordinator_allowing(&kp);

    // Local agent A applies first — advances live snapshot to SnapshotId(1).
    let cs_a = cs(0, "agent_a", vec![create_node_op(20, "fn.first")]);
    let outcome_a = coord.submit(cs_a).await;
    assert!(
        matches!(outcome_a, CoordinatorOutcome::Applied { .. }),
        "agent A must apply cleanly; got {outcome_a:?}"
    );

    // Remote agent signs at base SnapshotId(0) — now stale, disjoint NodeRef.
    let rcs = signed_rcs(
        0,
        "remote_agent",
        vec![create_node_op(21, "fn.remote_stale")],
        &kp,
    );

    let result = coord.verify_remote_submission(rcs).await;
    assert!(
        matches!(
            result,
            Ok(CoordinatorOutcome::RebaseApplied {
                rebased_onto: SnapshotId(1),
                applied_snapshot_id: SnapshotId(2),
            })
        ),
        "stale but valid remote submission must trigger rebase; got {result:?}"
    );
}

// ── Task 6.1d: Local submit still works after a remote submission ─────────
//
// Spec: Multi-Agent Coordination Unchanged
//   GIVEN a coordinator that processed one RemoteChangeSet successfully
//   WHEN a local CanonicalChangeSet is submitted via submit()
//   THEN it returns CoordinatorOutcome::Applied (no regression)
#[tokio::test]
async fn local_submit_works_after_remote_submission() {
    let kp = AgentKeypair::generate();
    let coord = coordinator_allowing(&kp);

    // Remote submission applied first.
    let rcs = signed_rcs(
        0,
        "remote_agent",
        vec![create_node_op(30, "fn.remote_first")],
        &kp,
    );
    let remote_result = coord.verify_remote_submission(rcs).await;
    assert!(
        matches!(remote_result, Ok(CoordinatorOutcome::Applied { .. })),
        "remote submission must apply; got {remote_result:?}"
    );

    // Local submit at the new live snapshot.
    let local_cs = cs(
        1,
        "local_agent",
        vec![create_node_op(31, "fn.local_second")],
    );
    let local_result = coord.submit(local_cs).await;
    assert!(
        matches!(
            local_result,
            CoordinatorOutcome::Applied {
                applied_snapshot_id: SnapshotId(2)
            }
        ),
        "local submit must work after remote submission; got {local_result:?}"
    );
}

#[tokio::test]
async fn remote_exchange_submit_maps_to_submission_response() {
    let kp = AgentKeypair::generate();
    let coord = coordinator_allowing(&kp);
    let rcs = signed_rcs(
        0,
        "remote_agent",
        vec![create_node_op(40, "fn.exchange_submit")],
        &kp,
    );

    let response = coord
        .handle_remote_exchange(RemoteExchangeRequest::SubmitChangeSet(rcs))
        .await;

    assert_eq!(
        response,
        RemoteExchangeResponse::Submission(RemoteSubmissionOutcome::Applied {
            applied_snapshot_id: SnapshotId(1),
        })
    );
}

#[tokio::test]
async fn remote_exchange_reports_disallowed_signer_code() {
    let coord = Coordinator::new(SnapshotId(0), empty_graph());
    let kp = AgentKeypair::generate();
    let rcs = signed_rcs(
        0,
        "remote_agent",
        vec![create_node_op(41, "fn.exchange_rejected")],
        &kp,
    );

    let response = coord
        .handle_remote_exchange(RemoteExchangeRequest::SubmitChangeSet(rcs))
        .await;

    assert!(
        matches!(
            response,
            RemoteExchangeResponse::Error { ref code, ref message }
                if code == "E_SIGNER_NOT_ALLOWED" && message.contains("not allowed")
        ),
        "disallowed exchange submit must return signer policy code; got {response:?}"
    );
}

#[tokio::test]
async fn remote_exchange_accepts_integrity_checked_bundle() {
    let coord = Coordinator::new(SnapshotId(0), empty_graph());
    let bundle = bundle_from_bytes(b"coordinator bundle");
    let root = bundle.root;

    let response = coord
        .handle_remote_exchange(RemoteExchangeRequest::PushBundle(bundle))
        .await;

    assert_eq!(
        response,
        RemoteExchangeResponse::BundleAccepted {
            root,
            object_count: 1,
        }
    );
}

#[tokio::test]
async fn remote_exchange_push_then_pull_returns_same_bundle() {
    let coord = Coordinator::new(SnapshotId(0), empty_graph());
    let bundle = bundle_from_bytes(b"bundle to pull after push");
    let root = bundle.root;

    let push_response = coord
        .handle_remote_exchange(RemoteExchangeRequest::PushBundle(bundle.clone()))
        .await;
    assert_eq!(
        push_response,
        RemoteExchangeResponse::BundleAccepted {
            root,
            object_count: 1,
        }
    );

    let pull_response = coord
        .handle_remote_exchange(RemoteExchangeRequest::PullBundle { root })
        .await;

    assert_eq!(pull_response, RemoteExchangeResponse::Bundle(bundle));
}

#[tokio::test]
async fn remote_exchange_reports_missing_pulled_bundle() {
    let coord = Coordinator::new(SnapshotId(0), empty_graph());
    let root = ObjectId::from_bytes(b"missing bundle root");

    let response = coord
        .handle_remote_exchange(RemoteExchangeRequest::PullBundle { root })
        .await;

    assert_eq!(response, RemoteExchangeResponse::BundleMissing { root });
}

#[tokio::test]
async fn remote_exchange_rejects_invalid_bundle_integrity() {
    let coord = Coordinator::new(SnapshotId(0), empty_graph());
    let root = ObjectId::from_bytes(b"expected root");
    let mut objects = BTreeMap::new();
    objects.insert(root, b"tampered payload".to_vec());
    let bundle = ObjectBundle::new(root, objects);

    let response = coord
        .handle_remote_exchange(RemoteExchangeRequest::PushBundle(bundle))
        .await;

    assert!(
        matches!(
            response,
            RemoteExchangeResponse::Error { ref code, .. } if code == "E_BUNDLE_INVALID"
        ),
        "invalid bundle must be rejected; got {response:?}"
    );

    let pull_response = coord
        .handle_remote_exchange(RemoteExchangeRequest::PullBundle { root })
        .await;

    assert_eq!(
        pull_response,
        RemoteExchangeResponse::BundleMissing { root }
    );
}
