use crate::codec::{CborCodec, ContentCodec};
use crate::error::StorageResult;
use crate::object::{ObjectId, RawObject};
use crate::retention::EnumerableObjectStore;

use super::issue::{issue_descriptors, sort_issues};
use super::{IntegrityIssue, ObjectIntegrityReport};

/// Verify loadability and content hashes for every enumerated CAS object.
///
/// This checks the executable object-store contract directly: every id returned
/// by `list_object_ids` must be loadable, and loaded bytes must hash back to the
/// same `ObjectId`.
pub async fn verify_object_store_integrity<S>(
    object_store: &S,
) -> StorageResult<ObjectIntegrityReport>
where
    S: EnumerableObjectStore + Send + Sync,
{
    verify_object_store_integrity_with_decoder(object_store, |_| true).await
}

/// Verify object-store integrity and decode each object as `T`.
///
/// This opt-in check is for homogeneous CAS scans where the caller knows the
/// expected CBOR type. Decode failures are reported as redacted corrupt-object
/// diagnostics instead of leaking codec error payloads.
pub async fn verify_decodable_object_store_integrity<S, T>(
    object_store: &S,
) -> StorageResult<ObjectIntegrityReport>
where
    S: EnumerableObjectStore + Send + Sync,
    T: for<'de> serde::Deserialize<'de>,
{
    let codec = CborCodec;
    verify_object_store_integrity_with_decoder(object_store, |raw| {
        codec.decode::<T>(&raw.0).is_ok()
    })
    .await
}

async fn verify_object_store_integrity_with_decoder<S>(
    object_store: &S,
    mut can_decode: impl FnMut(&RawObject) -> bool,
) -> StorageResult<ObjectIntegrityReport>
where
    S: EnumerableObjectStore + Send + Sync,
{
    let ids = object_store.list_object_ids().await?;
    let mut issues = Vec::new();
    let mut seen_ids = std::collections::BTreeSet::new();
    let mut unique_ids = Vec::new();

    for id in &ids {
        if seen_ids.insert(*id) {
            unique_ids.push(*id);
        } else {
            issues.push(IntegrityIssue::DuplicateObjectEntry { id: *id });
        }
    }

    for id in &unique_ids {
        match object_store.get(id).await? {
            None => issues.push(IntegrityIssue::MissingObject { id: *id }),
            Some(raw) => {
                if ObjectId::from_bytes(&raw.0) != *id {
                    issues.push(IntegrityIssue::HashMismatch { id: *id });
                }
                if !can_decode(&raw) {
                    issues.push(IntegrityIssue::CorruptObject { id: *id });
                }
            }
        }
    }

    sort_issues(&mut issues);
    let diagnostics = issue_descriptors(&issues);

    Ok(ObjectIntegrityReport {
        objects_checked: ids.len() as u64,
        passed: issues.is_empty(),
        issues,
        diagnostics,
    })
}
