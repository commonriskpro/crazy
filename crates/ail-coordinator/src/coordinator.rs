// ── ail-coordinator::coordinator ─────────────────────────────────────────
//
// Authoritative coordinator for concurrent ChangeSet serialization.
//
// # Overview
//
// `Coordinator` owns the live `SemanticGraph` and `MemorySnapshotBridge` behind
// a `tokio::sync::Mutex`.  Every `submit()` call acquires the lock exclusively,
// ensuring one-at-a-time serialization of concurrent agents.
//
// # Submit protocol
//
// 1. Acquire the async mutex.
// 2. Check `cs.base_snapshot_id` against the live snapshot id.
//    - Match → apply directly.
//    - Mismatch → attempt semantic rebase.
// 3. On rebase:
//    - Build `StructuralDiff` from committed ops since base.
//    - Call `rebase()` from `crate::rebase`.
//    - `Rebased(cs)` → apply the rebased changeset, return `RebaseApplied`.
//    - `Conflict(_)` → classify via `classify_conflict`, return `ConflictIrresolvable`.
// 4. On apply success → advance snapshot id, update committed diff.
//
// # Borrow note
//
// `apply()` requires `(&mut SemanticGraph, &dyn SnapshotBridge)`.  Both live on
// the same `CoordinatorState` struct, so we cannot borrow them simultaneously
// via the struct.  The solution: call `apply_cs()` — a free function that takes
// `graph` and `bridge` as separate arguments — by splitting the borrow.
//
// # Simplification
//
// Phase 13 stores only the most recent committed `StructuralDiff` (the ops
// from the last applied changeset).  Multi-hop rebase (base > 1 snapshot
// behind) is handled conservatively: the diff from the last applied changeset
// is the only window used for conflict detection.  Audit log is in-memory only.

use std::collections::BTreeSet;
use std::sync::Arc;

use ail_change::{
    apply::{SnapshotBridge, apply},
    canonical::{CanonicalChangeSet, CanonicalOp, OpPayload},
    model::{ChangeSetOutcome, SnapshotId},
    storage_bridge::MemorySnapshotBridge,
};
use ail_core::semantic_graph::{NodeRef, SemanticGraph};
use ail_remote::{
    BundleStore, InMemoryBundleStore, RemoteChangeSet, RemoteError, RemoteExchangeRequest,
    RemoteExchangeResponse, RemoteSignerPolicy, RemoteSubmissionOutcome,
};
use tokio::sync::Mutex;

use crate::{
    conflict::ConflictReason,
    rebase::{RebaseResult, StructuralDiff, classify_conflict, rebase},
};

// ── FixedSnapshotBridge ───────────────────────────────────────────────────

/// A `SnapshotBridge` that always returns the same pre-captured `SnapshotId`.
///
/// Used to break the borrow conflict between `&mut state.graph` and
/// `&state.bridge`: we capture the live id up-front, then pass this cheap
/// value-type bridge to `apply()` so no reference into `state` is needed.
struct FixedSnapshotBridge(SnapshotId);

impl SnapshotBridge for FixedSnapshotBridge {
    fn current_snapshot_id(&self) -> SnapshotId {
        self.0
    }
}

// ── CoordinatorOutcome ────────────────────────────────────────────────────

/// The typed outcome returned to a caller by `Coordinator::submit`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoordinatorOutcome {
    /// The changeset was applied cleanly against the current live snapshot.
    Applied {
        /// The new live snapshot id after this apply.
        applied_snapshot_id: SnapshotId,
    },
    /// The changeset was stale but was rebased successfully and then applied.
    RebaseApplied {
        /// The live snapshot id the changeset was rebased onto.
        rebased_onto: SnapshotId,
        /// The new live snapshot id after the rebased apply.
        applied_snapshot_id: SnapshotId,
    },
    /// The coordinator determined the changeset conflicts irresolvably.
    /// The live snapshot id does NOT advance.
    ConflictIrresolvable {
        /// Why the conflict cannot be resolved by semantic rebase.
        reason: ConflictReason,
    },
    /// The changeset was stale and no rebase was attempted (fallback; not used
    /// in Phase 13 — rebase is always attempted).
    StaleBase {
        /// The current live snapshot id.
        current_snapshot_id: SnapshotId,
    },
    /// An unexpected internal error occurred during apply.
    Failed {
        /// Human-readable reason.
        reason: String,
    },
}

// ── CoordinatorState ─────────────────────────────────────────────────────

/// Mutable interior state guarded by the coordinator's async mutex.
struct CoordinatorState {
    /// The current live graph.
    graph: SemanticGraph,
    /// Snapshot bridge — tracks the live snapshot id and provides the
    /// `SnapshotBridge` impl required by `apply()`.
    bridge: MemorySnapshotBridge,
    /// `StructuralDiff` from the most recently committed changeset.
    /// Used by `rebase()` for conflict detection when a pending changeset is stale.
    committed_diff: StructuralDiff,
    /// `NodeRef`s removed by the most recently committed changeset.
    /// Used by `classify_conflict()` to distinguish deletes from modifies.
    committed_removes: BTreeSet<NodeRef>,
    /// Accepted remote bundles keyed by their root object id.
    bundle_store: InMemoryBundleStore,
}

// ── Coordinator ───────────────────────────────────────────────────────────

/// Authoritative coordinator for multi-agent ChangeSet serialization.
///
/// `Coordinator` is `Clone + Send + Sync` — it can be shared freely across
/// `tokio` tasks.  All mutable state is behind the inner `Arc<Mutex<...>>`.
#[derive(Clone)]
pub struct Coordinator {
    inner: Arc<Mutex<CoordinatorState>>,
    remote_signer_policy: Arc<RemoteSignerPolicy>,
}

impl Coordinator {
    /// Create a new `Coordinator` with an empty graph and the given initial
    /// snapshot id.
    pub fn new(initial_snapshot_id: SnapshotId, graph: SemanticGraph) -> Self {
        Self::with_remote_signer_policy(initial_snapshot_id, graph, RemoteSignerPolicy::deny_all())
    }

    /// Create a new `Coordinator` with an explicit remote signer policy.
    pub fn with_remote_signer_policy(
        initial_snapshot_id: SnapshotId,
        graph: SemanticGraph,
        remote_signer_policy: RemoteSignerPolicy,
    ) -> Self {
        let bridge = MemorySnapshotBridge::new(initial_snapshot_id);
        let state = CoordinatorState {
            graph,
            bridge,
            committed_diff: StructuralDiff {
                touched_nodes: BTreeSet::new(),
            },
            committed_removes: BTreeSet::new(),
            bundle_store: InMemoryBundleStore::new(),
        };
        Self {
            inner: Arc::new(Mutex::new(state)),
            remote_signer_policy: Arc::new(remote_signer_policy),
        }
    }

    /// Submit a `CanonicalChangeSet` for atomic application.
    ///
    /// Acquires the exclusive lock, runs the snapshot-guard → (rebase) → apply
    /// → advance cycle, and returns a `CoordinatorOutcome`.
    #[cfg_attr(
        feature = "otel",
        tracing::instrument(skip_all, name = "coordinator.submit")
    )]
    pub async fn submit(&self, cs: CanonicalChangeSet) -> CoordinatorOutcome {
        let mut state = self.inner.lock().await;

        let live_id = state.bridge.current_snapshot_id();

        if cs.base_snapshot_id == live_id {
            // ── Clean apply path ─────────────────────────────────────────
            let committed_diff = StructuralDiff::from_ops(&cs.ops);
            let committed_removes = extract_removed_nodes(&cs.ops);

            // Use FixedSnapshotBridge so we don't hold &state.bridge
            // while also holding &mut state.graph.
            let fixed_bridge = FixedSnapshotBridge(live_id);
            let outcome = apply(cs, &mut state.graph, &fixed_bridge);

            match outcome {
                ChangeSetOutcome::Applied => {
                    state.bridge.advance_snapshot_id();
                    state.committed_diff = committed_diff;
                    state.committed_removes = committed_removes;
                    let applied_snapshot_id = state.bridge.current_snapshot_id();
                    CoordinatorOutcome::Applied {
                        applied_snapshot_id,
                    }
                }
                ChangeSetOutcome::Failed { reason } => CoordinatorOutcome::Failed { reason },
                ChangeSetOutcome::RebaseRequired { .. } => {
                    // Should not happen when base == live_id; guard is a safety net.
                    CoordinatorOutcome::StaleBase {
                        current_snapshot_id: live_id,
                    }
                }
                ChangeSetOutcome::ConflictIrresolvable { reason } => {
                    CoordinatorOutcome::ConflictIrresolvable { reason }
                }
            }
        } else {
            // ── Stale base → attempt semantic rebase ─────────────────────
            let pending_diff = StructuralDiff::from_ops(&cs.ops);

            match rebase(cs, &state.committed_diff, live_id) {
                RebaseResult::Rebased(rebased_cs) => {
                    // Rebase succeeded — apply the rebased changeset.
                    let rebased_onto = live_id;
                    let new_committed_diff = StructuralDiff::from_ops(&rebased_cs.ops);
                    let new_committed_removes = extract_removed_nodes(&rebased_cs.ops);

                    // live_id captured above; use FixedSnapshotBridge for same reason.
                    let rebased_bridge = FixedSnapshotBridge(live_id);
                    let outcome = apply(rebased_cs, &mut state.graph, &rebased_bridge);

                    match outcome {
                        ChangeSetOutcome::Applied => {
                            state.bridge.advance_snapshot_id();
                            state.committed_diff = new_committed_diff;
                            state.committed_removes = new_committed_removes;
                            let applied_snapshot_id = state.bridge.current_snapshot_id();
                            CoordinatorOutcome::RebaseApplied {
                                rebased_onto,
                                applied_snapshot_id,
                            }
                        }
                        ChangeSetOutcome::Failed { reason } => {
                            CoordinatorOutcome::Failed { reason }
                        }
                        ChangeSetOutcome::RebaseRequired {
                            current_snapshot_id,
                        } => {
                            // Concurrency race — should not happen under mutex.
                            CoordinatorOutcome::StaleBase {
                                current_snapshot_id,
                            }
                        }
                        ChangeSetOutcome::ConflictIrresolvable { reason } => {
                            CoordinatorOutcome::ConflictIrresolvable { reason }
                        }
                    }
                }
                RebaseResult::Conflict(_conservative_reason) => {
                    // Classify with full context using committed removes.
                    let conflicts: BTreeSet<NodeRef> = pending_diff
                        .touched_nodes
                        .intersection(&state.committed_diff.touched_nodes)
                        .copied()
                        .collect();
                    let reason = classify_conflict(&conflicts, &state.committed_removes);
                    CoordinatorOutcome::ConflictIrresolvable { reason }
                }
            }
        }
    }

    /// Verify the Ed25519 signature on a `RemoteChangeSet` and, if valid, submit
    /// the enclosed `CanonicalChangeSet` via the existing [`submit`](Self::submit) protocol.
    ///
    /// # Errors
    ///
    /// - Returns `Err(RemoteError::SignatureInvalid)` if signature verification
    ///   fails.  The coordinator's live snapshot does **not** advance.
    /// - Returns `Err(RemoteError::SignerRejected(_))` if the signature is valid
    ///   but the signer is not allowed by local policy.
    /// - Returns `Err(RemoteError::CoordinatorFailed(reason))` if `submit()`
    ///   returns a [`CoordinatorOutcome::Failed`] outcome.
    pub async fn verify_remote_submission(
        &self,
        rcs: RemoteChangeSet,
    ) -> Result<CoordinatorOutcome, RemoteError> {
        rcs.verify_signature()
            .map_err(|_| RemoteError::SignatureInvalid)?;
        self.remote_signer_policy
            .check_identity(&rcs.agent)
            .map_err(RemoteError::SignerRejected)?;

        let outcome = self.submit(rcs.changeset).await;

        match &outcome {
            CoordinatorOutcome::Failed { reason } => {
                Err(RemoteError::CoordinatorFailed(reason.clone()))
            }
            _ => Ok(outcome),
        }
    }

    /// Handle a transport-agnostic remote exchange request.
    ///
    /// This is the service-shaped boundary for remote collaboration: callers can
    /// put it behind a network transport later, while the coordinator remains an
    /// in-process authority over signed submissions.
    pub async fn handle_remote_exchange(
        &self,
        request: RemoteExchangeRequest,
    ) -> RemoteExchangeResponse {
        match request {
            RemoteExchangeRequest::SubmitChangeSet(rcs) => {
                match self.verify_remote_submission(*rcs).await {
                    Ok(outcome) => {
                        RemoteExchangeResponse::Submission(remote_submission_outcome(outcome))
                    }
                    Err(err) => RemoteExchangeResponse::Error {
                        code: remote_error_code(&err).to_string(),
                        message: err.to_string(),
                    },
                }
            }
            RemoteExchangeRequest::PushBundle(bundle) => match bundle.verify_integrity() {
                Ok(()) => {
                    let root = bundle.root;
                    let object_count = bundle.objects.len();
                    let mut state = self.inner.lock().await;
                    state.bundle_store.put_bundle(bundle);
                    RemoteExchangeResponse::BundleAccepted { root, object_count }
                }
                Err(err) => RemoteExchangeResponse::Error {
                    code: "E_BUNDLE_INVALID".to_string(),
                    message: err.to_string(),
                },
            },
            RemoteExchangeRequest::PullBundle { root } => {
                let state = self.inner.lock().await;
                match state.bundle_store.get_bundle(&root) {
                    Some(bundle) => RemoteExchangeResponse::Bundle(bundle),
                    None => RemoteExchangeResponse::BundleMissing { root },
                }
            }
        }
    }
}

// ── helpers ───────────────────────────────────────────────────────────────

/// Extract the set of `NodeRef`s that were removed by the given ops.
fn extract_removed_nodes(ops: &[CanonicalOp]) -> BTreeSet<NodeRef> {
    ops.iter()
        .filter_map(|op| {
            if let OpPayload::RemoveNode(node_ref) = &op.payload {
                Some(*node_ref)
            } else {
                None
            }
        })
        .collect()
}

fn remote_submission_outcome(outcome: CoordinatorOutcome) -> RemoteSubmissionOutcome {
    match outcome {
        CoordinatorOutcome::Applied {
            applied_snapshot_id,
        } => RemoteSubmissionOutcome::Applied {
            applied_snapshot_id,
        },
        CoordinatorOutcome::RebaseApplied {
            rebased_onto,
            applied_snapshot_id,
        } => RemoteSubmissionOutcome::RebaseApplied {
            rebased_onto,
            applied_snapshot_id,
        },
        CoordinatorOutcome::ConflictIrresolvable { reason } => {
            RemoteSubmissionOutcome::ConflictIrresolvable { reason }
        }
        CoordinatorOutcome::StaleBase {
            current_snapshot_id,
        } => RemoteSubmissionOutcome::StaleBase {
            current_snapshot_id,
        },
        CoordinatorOutcome::Failed { reason } => RemoteSubmissionOutcome::Failed { reason },
    }
}

fn remote_error_code(err: &RemoteError) -> &'static str {
    match err {
        RemoteError::SignatureInvalid => "E_SIGNATURE_INVALID",
        RemoteError::SignerRejected(_) => "E_SIGNER_NOT_ALLOWED",
        RemoteError::CoordinatorFailed(_) => "E_COORDINATOR_FAILED",
    }
}
