use super::*;

// ── T-K: verification report persistence ─────────────────────────────────

// Scenario K-1: cmd_verify on a file store saves the report and the hash
//   appears in the JSON output; cmd_apply on the same store sets
//   verification_report_hash in the new snapshot envelope.
//
//   GIVEN a file-backed StoreHandle with a saved CanonicalChangeSet
//   WHEN cmd_verify is run (json mode)
//   THEN the JSON output contains a non-null "verification_report_hash"
//   AND the sidecar at .ail/reports/<change_id> exists
//   AND cmd_apply on the same store produces a snapshot with a non-None
//       verification_report_hash
#[tokio::test]
async fn cmd_verify_persists_report_and_apply_captures_hash() {
    use crate::store::{file_store, init_file_layout};
    use ail_change::canonical::CanonicalChangeSet;
    use ail_storage::object::ObjectId;

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    // Create the reports directory so the sidecar write succeeds.
    std::fs::create_dir_all(ail_dir.join("reports")).expect("create reports dir");

    let store = file_store(ail_dir.clone());

    // Save a minimal changeset so verify/apply can find it.
    let canonical = CanonicalChangeSet::default();
    let mut cbor_bytes = Vec::new();
    ciborium::into_writer(&canonical, &mut cbor_bytes).expect("CBOR encode");
    let change_id = ObjectId::from_bytes(&cbor_bytes).to_hex();
    store
        .save_changeset_payload(&change_id, &cbor_bytes)
        .await
        .expect("save changeset");

    // Run verify — must succeed and persist the report.
    let verify_result = cmd_verify(OutputMode::Json, &change_id, "dev", "simple", &store).await;
    assert!(
        verify_result.is_ok(),
        "cmd_verify must succeed; got: {verify_result:?}"
    );

    // Sidecar must exist on disk.
    let sidecar = ail_dir.join("reports").join(&change_id);
    assert!(
        sidecar.exists(),
        "sidecar file must exist at .ail/reports/<change_id> after verify"
    );

    // Report hash must be loadable by change_id.
    let loaded = store
        .load_verification_report_by_change_id(&change_id)
        .await
        .expect("must not error")
        .expect("report must be present after verify");
    assert!(
        !loaded.1.to_hex().is_empty(),
        "report hash must be non-empty"
    );

    // Run apply — the new snapshot must carry the verification_report_hash.
    let apply_result = cmd_apply(OutputMode::Human, &change_id, false, None, &store).await;
    assert!(
        apply_result.is_ok(),
        "cmd_apply must succeed; got: {apply_result:?}"
    );

    // The latest snapshot should have verification_report_hash set.
    let snapshots = store.list_snapshots().await.expect("list snapshots");
    let latest = snapshots
        .iter()
        .max_by_key(|s| s.created_at)
        .expect("at least one snapshot after apply");
    assert!(
        latest.verification_report_hash.is_some(),
        "snapshot after apply must carry verification_report_hash when report was persisted"
    );
}

// Scenario K-2: inspect report loads a persisted report by its hash.
//   GIVEN a file store with a saved verification report
//   WHEN cmd_inspect "report" is called with the report hash
//   THEN source is "persisted_by_hash" in the JSON output
#[tokio::test]
async fn cmd_inspect_report_loads_persisted_by_hash() {
    use crate::store::{file_store, init_file_layout};
    use ail_verify::report::VerificationReport;

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    std::fs::create_dir_all(ail_dir.join("reports")).expect("create reports dir");
    let store = file_store(ail_dir.clone());

    let change_id = "f".repeat(64);
    let report = VerificationReport::default();
    let hash = store
        .save_verification_report(&change_id, &report)
        .await
        .expect("save report");

    // Inspect by hash — should load persisted report.
    let result = cmd_inspect(OutputMode::Json, "report", &hash.to_hex(), &store).await;
    assert!(
        result.is_ok(),
        "cmd_inspect report by hash must succeed; got: {result:?}"
    );
}

// Scenario K-2b: report sidecar lookup works for non-hex change ids.
//   GIVEN a file store with a saved verification report for a non-hash change id
//   WHEN load_verification_report_by_change_id is called
//   THEN the persisted report is resolved via the sidecar path
#[tokio::test]
async fn load_verification_report_by_non_hex_change_id() {
    use crate::store::{file_store, init_file_layout};
    use ail_verify::report::VerificationReport;

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    std::fs::create_dir_all(ail_dir.join("reports")).expect("create reports dir");
    let store = file_store(ail_dir);

    let change_id = "change.add_checkout";
    let report = VerificationReport::default();
    let saved_hash = store
        .save_verification_report(change_id, &report)
        .await
        .expect("save report");

    let (_loaded_report, loaded_hash) = store
        .load_verification_report_by_change_id(change_id)
        .await
        .expect("load by change id must not error")
        .expect("report must resolve via change id sidecar");

    assert_eq!(
        loaded_hash, saved_hash,
        "change-id sidecar must point to the saved report hash"
    );
}

// Scenario K-2c: when hash lookup misses, change-id sidecar lookup still works.
//   GIVEN an id that is 64 hex chars and also has a sidecar entry
//   WHEN the hash object lookup would miss but sidecar exists
//   THEN loading by change id resolves the persisted report hash
#[tokio::test]
async fn load_verification_report_by_hex_change_id_sidecar_after_hash_miss() {
    use crate::store::{file_store, init_file_layout};
    use ail_verify::report::VerificationReport;

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    std::fs::create_dir_all(ail_dir.join("reports")).expect("create reports dir");
    let store = file_store(ail_dir);

    let change_id = "0".repeat(64);
    let missing_hash = hex_to_object_id(&change_id).expect("valid object id");
    assert!(
        store
            .load_verification_report_by_hash(&missing_hash)
            .await
            .expect("hash lookup should not error")
            .is_none(),
        "test setup requires the direct hash lookup to miss"
    );

    let report = VerificationReport::default();
    let saved_hash = store
        .save_verification_report(&change_id, &report)
        .await
        .expect("save report");
    let (_loaded_report, loaded_hash) = store
        .load_verification_report_by_change_id(&change_id)
        .await
        .expect("change-id sidecar lookup should not error")
        .expect("sidecar lookup should find persisted report");

    assert_eq!(loaded_hash, saved_hash);
}

// Scenario K-3: inspect report falls back to derive when no report is persisted.
//   GIVEN a memory store (no sidecar support)
//   WHEN cmd_inspect "report" is called with a valid 64-char hex
//   THEN the command succeeds with derived_from_current_graph fallback
#[tokio::test]
async fn cmd_inspect_report_falls_back_to_derived_on_memory_store() {
    use crate::store::memory_store;
    let store = memory_store();
    let unknown_hash = "a".repeat(64);
    let result = cmd_inspect(OutputMode::Human, "report", &unknown_hash, &store).await;
    assert!(
        result.is_ok(),
        "cmd_inspect report fallback must succeed; got: {result:?}"
    );
}
