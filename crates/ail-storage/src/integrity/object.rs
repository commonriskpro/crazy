use crate::error::StorageResult;
use crate::object::ObjectId;
use crate::retention::EnumerableObjectStore;

use super::{IntegrityIssue, ObjectIntegrityReport};

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
    let ids = object_store.list_object_ids().await?;
    let mut issues = Vec::new();

    for id in &ids {
        match object_store.get(id).await? {
            None => issues.push(IntegrityIssue::MissingObject { id: *id }),
            Some(raw) => {
                if ObjectId::from_bytes(&raw.0) != *id {
                    issues.push(IntegrityIssue::HashMismatch { id: *id });
                }
            }
        }
    }

    issues.sort_by(|a, b| {
        a.kind_ord()
            .cmp(&b.kind_ord())
            .then(a.id().as_bytes().cmp(b.id().as_bytes()))
    });

    Ok(ObjectIntegrityReport {
        objects_checked: ids.len() as u64,
        passed: issues.is_empty(),
        issues,
    })
}
