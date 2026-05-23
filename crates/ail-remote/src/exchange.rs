// ── ail-remote::exchange ──────────────────────────────────────────────────
//
// Transport-agnostic remote exchange DTOs.
//
// These types describe the product/service boundary for remote collaboration
// without choosing HTTP, stdio, MCP, or any other transport.

use ail_change::model::{ConflictReason, SnapshotId};
use ail_storage::object::ObjectId;
use serde::{Deserialize, Serialize};

use crate::{ObjectBundle, RemoteChangeSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RemoteExchangeRequest {
    SubmitChangeSet(Box<RemoteChangeSet>),
    PushBundle(ObjectBundle),
    PullBundle { root: ObjectId },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RemoteExchangeResponse {
    Submission(RemoteSubmissionOutcome),
    BundleAccepted { root: ObjectId, object_count: usize },
    Bundle(ObjectBundle),
    BundleMissing { root: ObjectId },
    Error { code: String, message: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RemoteSubmissionOutcome {
    Applied {
        applied_snapshot_id: SnapshotId,
    },
    RebaseApplied {
        rebased_onto: SnapshotId,
        applied_snapshot_id: SnapshotId,
    },
    ConflictIrresolvable {
        reason: ConflictReason,
    },
    StaleBase {
        current_snapshot_id: SnapshotId,
    },
    Failed {
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ail_storage::codec::{CborCodec, ContentCodec};

    use super::*;

    fn bundle() -> ObjectBundle {
        let bytes = b"remote object".to_vec();
        let root = ObjectId::from_bytes(&bytes);
        let mut objects = BTreeMap::new();
        objects.insert(root, bytes);
        ObjectBundle::new(root, objects)
    }

    #[test]
    fn push_bundle_request_cbor_roundtrip_is_stable() {
        let codec = CborCodec;
        let request = RemoteExchangeRequest::PushBundle(bundle());

        let bytes = codec.encode(&request).expect("encode request");
        let decoded: RemoteExchangeRequest = codec.decode(&bytes).expect("decode request");
        let reencoded = codec.encode(&decoded).expect("re-encode request");

        assert_eq!(decoded, request);
        assert_eq!(reencoded, bytes, "remote request CBOR must be stable");
    }

    #[test]
    fn bundle_accepted_response_cbor_roundtrip_is_stable() {
        let codec = CborCodec;
        let bundle = bundle();
        let response = RemoteExchangeResponse::BundleAccepted {
            root: bundle.root,
            object_count: bundle.objects.len(),
        };

        let bytes = codec.encode(&response).expect("encode response");
        let decoded: RemoteExchangeResponse = codec.decode(&bytes).expect("decode response");
        let reencoded = codec.encode(&decoded).expect("re-encode response");

        assert_eq!(decoded, response);
        assert_eq!(reencoded, bytes, "remote response CBOR must be stable");
    }
}
