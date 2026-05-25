use super::*;

// Scenario: cmd_apply blocks on memory store when no verification report exists.
//
// Wave 9D: Memory backend now enforces the verification gate via an in-process
// report index.  Applying without a prior `ail verify` call returns a Domain
// error for the missing report rather than a NotFound for the missing changeset.
#[tokio::test]
async fn cmd_apply_memory_store_requires_stored_changeset() {
    use crate::store::memory_store;
    let store = memory_store();
    let id = "b".repeat(64);
    let result = cmd_apply(OutputMode::Human, &id, false, None, &store).await;
    match &result {
        Err(CliError::Domain(msg)) => assert!(
            msg.contains("no verification report found"),
            "error must mention missing report; got: {msg}"
        ),
        other => panic!(
            "expected Domain error for missing verification report; got: {other:?}"
        ),
    }
}

// Scenario: change creates a graph snapshot that compile can load.
#[tokio::test]
async fn cmd_change_snapshot_load_compile_flow() {
    use crate::store::memory_store;
    let store = memory_store();

    let change = cmd_change(
        OutputMode::Human,
        Some("record storage-backed compile flow"),
        None,
        false,
        true, // apply_immediately: unit test needs a snapshot created
        None,
        &store,
    )
    .await;
    assert!(change.is_ok(), "cmd_change must apply; got: {change:?}");

    let snapshots = store.list_snapshots().await.expect("list snapshots");
    let snapshot = latest_snapshot(&snapshots).expect("change must create a snapshot");
    let graph = store
        .load_graph(&snapshot.graph_root_hash)
        .await
        .expect("load graph")
        .expect("graph root must exist");
    assert!(graph.validate().is_ok(), "stored graph must validate");

    let compile = cmd_compile(OutputMode::Human, "dev", "wasm", &store).await;
    assert!(
        compile.is_ok(),
        "compile must load stored graph; got: {compile:?}"
    );
}

// Scenario: cmd_apply rejects invalid change-id.
#[tokio::test]
async fn cmd_apply_rejects_invalid_change_id() {
    use crate::store::memory_store;
    let store = memory_store();
    let result = cmd_apply(OutputMode::Human, &"a".repeat(63), false, None, &store).await;
    assert!(matches!(result, Err(CliError::NotFound(_))));
}

// Scenario: cmd_apply blocks on prod profile without --yes.
//   GIVEN a valid change-id and profile=prod
//   WHEN yes=false
//   THEN cmd_apply returns a Domain error mentioning approval
#[tokio::test]
async fn cmd_apply_blocks_prod_without_yes() {
    use crate::store::memory_store;
    let store = memory_store();
    let id = "c".repeat(64);
    let result = cmd_apply(OutputMode::Human, &id, false, Some("prod"), &store).await;
    match &result {
        Err(CliError::Domain(msg)) => assert!(
            msg.contains("approval"),
            "error must mention approval; got: {msg}"
        ),
        other => panic!("expected Domain error; got: {other:?}"),
    }
}

// Scenario: cmd_apply allows prod profile when --yes is set.
//   GIVEN a valid change-id, a saved changeset payload, and a verification report for "prod"
//   WHEN yes=true
//   THEN cmd_apply proceeds (does not return a policy error)
//
// Wave 9D: Memory backend now enforces the verification gate, so the test must
// also save a verification report under the "prod" profile before applying.
#[tokio::test]
async fn cmd_apply_allows_prod_with_yes() {
    use crate::store::memory_store;
    use ail_change::canonical::CanonicalChangeSet;
    use ail_verify::report::VerificationReport;

    let store = memory_store();
    let canonical = CanonicalChangeSet::default();
    let mut cbor_bytes = Vec::new();
    ciborium::into_writer(&canonical, &mut cbor_bytes).expect("CBOR encode must succeed");
    let change_id = ail_storage::object::ObjectId::from_bytes(&cbor_bytes).to_hex();
    store
        .save_changeset_payload(&change_id, &cbor_bytes)
        .await
        .expect("save must succeed");

    // Wave 9D: Memory gate is now enforced; save an accepted report for "prod".
    store
        .save_verification_report(&change_id, "prod", &VerificationReport::default())
        .await
        .expect("save_verification_report must succeed for memory store");

    let result = cmd_apply(OutputMode::Human, &change_id, true, Some("prod"), &store).await;
    assert!(
        result.is_ok(),
        "prod with --yes must succeed; got: {result:?}"
    );
}

// ── Profile-gate enforcement (file-backed store) ──────────────────────────

// Scenario PG-1: verify with "dev", apply with "prod" is blocked by the gate.
//   GIVEN a file store with a changeset verified under "dev" profile
//   WHEN cmd_apply is called with policy_profile = Some("prod")
//   THEN Err(CliError::Domain) is returned mentioning profile mismatch
#[tokio::test]
async fn cmd_apply_blocks_when_verify_profile_mismatches_apply_profile() {
    use crate::store::{file_store, init_file_layout};
    use ail_change::canonical::CanonicalChangeSet;
    use ail_storage::object::ObjectId;

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    std::fs::create_dir_all(ail_dir.join("reports")).expect("create reports dir");

    let store = file_store(ail_dir);

    // Save a changeset.
    let canonical = CanonicalChangeSet::default();
    let mut cbor_bytes = Vec::new();
    ciborium::into_writer(&canonical, &mut cbor_bytes).expect("CBOR encode");
    let change_id = ObjectId::from_bytes(&cbor_bytes).to_hex();
    store
        .save_changeset_payload(&change_id, &cbor_bytes)
        .await
        .expect("save changeset");

    // Verify with "dev" profile — writes sidecar with profile="dev".
    cmd_verify(OutputMode::Human, &change_id, "dev", "simple", &store)
        .await
        .expect("verify must succeed");

    // Apply with "prod" policy — must block: profile mismatch.
    let result = cmd_apply(
        OutputMode::Human,
        &change_id,
        true, // --yes to bypass the prod approval gate
        Some("prod"),
        &store,
    )
    .await;

    match &result {
        Err(CliError::Domain(msg)) => {
            assert!(
                msg.contains("dev") && msg.contains("prod"),
                "error must mention both profiles; got: {msg}"
            );
            assert!(
                msg.contains("profile"),
                "error must mention 'profile'; got: {msg}"
            );
        }
        other => panic!("expected profile-mismatch Domain error; got: {other:?}"),
    }
}

// Scenario PG-2: verify with "prod", apply with "prod" succeeds (same profile).
//   GIVEN a file store with a changeset verified under "prod" profile
//   WHEN cmd_apply is called with policy_profile = Some("prod") and --yes
//   THEN the apply succeeds
#[tokio::test]
async fn cmd_apply_succeeds_when_verify_and_apply_use_same_profile() {
    use crate::store::{file_store, init_file_layout};
    use ail_change::canonical::CanonicalChangeSet;
    use ail_storage::object::ObjectId;

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    std::fs::create_dir_all(ail_dir.join("reports")).expect("create reports dir");

    let store = file_store(ail_dir);

    let canonical = CanonicalChangeSet::default();
    let mut cbor_bytes = Vec::new();
    ciborium::into_writer(&canonical, &mut cbor_bytes).expect("CBOR encode");
    let change_id = ObjectId::from_bytes(&cbor_bytes).to_hex();
    store
        .save_changeset_payload(&change_id, &cbor_bytes)
        .await
        .expect("save changeset");

    // Verify with "prod" profile.
    cmd_verify(OutputMode::Human, &change_id, "prod", "simple", &store)
        .await
        .expect("verify must succeed");

    // Apply with "prod" policy and --yes — must succeed.
    let result = cmd_apply(OutputMode::Human, &change_id, true, Some("prod"), &store).await;
    assert!(
        result.is_ok(),
        "apply must succeed when verify and apply share the same 'prod' profile; got: {result:?}"
    );
}

// Scenario PG-3: verify with "dev", apply with no policy (defaults to "dev") succeeds.
//   GIVEN a file store with a changeset verified under "dev" profile
//   WHEN cmd_apply is called with policy_profile = None (defaults to "dev")
//   THEN the apply succeeds (profiles match)
#[tokio::test]
async fn cmd_apply_succeeds_when_dev_verify_and_default_apply_profile() {
    use crate::store::{file_store, init_file_layout};
    use ail_change::canonical::CanonicalChangeSet;
    use ail_storage::object::ObjectId;

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    std::fs::create_dir_all(ail_dir.join("reports")).expect("create reports dir");

    let store = file_store(ail_dir);

    let canonical = CanonicalChangeSet::default();
    let mut cbor_bytes = Vec::new();
    ciborium::into_writer(&canonical, &mut cbor_bytes).expect("CBOR encode");
    let change_id = ObjectId::from_bytes(&cbor_bytes).to_hex();
    store
        .save_changeset_payload(&change_id, &cbor_bytes)
        .await
        .expect("save changeset");

    // Verify with "dev" profile.
    cmd_verify(OutputMode::Human, &change_id, "dev", "simple", &store)
        .await
        .expect("verify must succeed");

    // Apply with no policy (default = "dev") — must succeed.
    let result = cmd_apply(OutputMode::Human, &change_id, false, None, &store).await;
    assert!(
        result.is_ok(),
        "apply must succeed when verify used 'dev' and apply uses default 'dev'; got: {result:?}"
    );
}

// Scenario PG-4: legacy sidecar (no profile field) is treated as "dev" and
//   satisfies apply when --policy is also "dev".
//   GIVEN a file store with a legacy sidecar (just hash, no profile token)
//   WHEN cmd_apply is called with policy_profile = None (defaults to "dev")
//   THEN the apply succeeds (legacy → "dev" matches "dev")
#[tokio::test]
async fn cmd_apply_accepts_legacy_sidecar_for_dev_apply() {
    use crate::store::{file_store, init_file_layout};
    use ail_change::canonical::CanonicalChangeSet;
    use ail_storage::object::ObjectId;
    use ail_verify::report::VerificationReport;

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let reports_dir = ail_dir.join("reports");
    std::fs::create_dir_all(&reports_dir).expect("create reports dir");

    let store = file_store(ail_dir.clone());

    let canonical = CanonicalChangeSet::default();
    let mut cbor_bytes = Vec::new();
    ciborium::into_writer(&canonical, &mut cbor_bytes).expect("CBOR encode");
    let change_id = ObjectId::from_bytes(&cbor_bytes).to_hex();
    store
        .save_changeset_payload(&change_id, &cbor_bytes)
        .await
        .expect("save changeset");

    // Save a report object directly and write a legacy sidecar (hash only, no profile).
    let report = VerificationReport::default();
    let hash = store
        .save_verification_report(&change_id, "dev", &report)
        .await
        .expect("save report");
    // Overwrite sidecar with legacy format: just the hash, no profile token.
    let sidecar = reports_dir.join(&change_id);
    std::fs::write(&sidecar, format!("{}\n", hash.to_hex())).expect("write legacy sidecar");

    // Apply with default "dev" policy — must succeed (legacy profile treated as "dev").
    let result = cmd_apply(OutputMode::Human, &change_id, false, None, &store).await;
    assert!(
        result.is_ok(),
        "apply must accept legacy sidecar (no profile) when apply profile is 'dev'; got: {result:?}"
    );
}

// Scenario PG-5: legacy sidecar (no profile field) is treated as "dev" and
//   blocks apply when --policy is "prod" (profile mismatch).
//   GIVEN a file store with a legacy sidecar (just hash, no profile token)
//   WHEN cmd_apply is called with policy_profile = Some("prod") and --yes
//   THEN Err(CliError::Domain) is returned mentioning profile mismatch
#[tokio::test]
async fn cmd_apply_blocks_legacy_sidecar_for_prod_apply() {
    use crate::store::{file_store, init_file_layout};
    use ail_change::canonical::CanonicalChangeSet;
    use ail_storage::object::ObjectId;
    use ail_verify::report::VerificationReport;

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let reports_dir = ail_dir.join("reports");
    std::fs::create_dir_all(&reports_dir).expect("create reports dir");

    let store = file_store(ail_dir.clone());

    let canonical = CanonicalChangeSet::default();
    let mut cbor_bytes = Vec::new();
    ciborium::into_writer(&canonical, &mut cbor_bytes).expect("CBOR encode");
    let change_id = ObjectId::from_bytes(&cbor_bytes).to_hex();
    store
        .save_changeset_payload(&change_id, &cbor_bytes)
        .await
        .expect("save changeset");

    // Save a report object and write a legacy sidecar (hash only, no profile).
    let report = VerificationReport::default();
    let hash = store
        .save_verification_report(&change_id, "dev", &report)
        .await
        .expect("save report");
    // Overwrite sidecar with legacy format: just the hash, no profile token.
    let sidecar = reports_dir.join(&change_id);
    std::fs::write(&sidecar, format!("{}\n", hash.to_hex())).expect("write legacy sidecar");

    // Apply with "prod" policy and --yes — must block: legacy → "dev" ≠ "prod".
    let result = cmd_apply(
        OutputMode::Human,
        &change_id,
        true, // --yes to bypass the prod approval gate
        Some("prod"),
        &store,
    )
    .await;

    match &result {
        Err(CliError::Domain(msg)) => {
            assert!(
                msg.contains("dev") && msg.contains("prod"),
                "error must mention both profiles; got: {msg}"
            );
            assert!(
                msg.contains("profile"),
                "error must mention 'profile'; got: {msg}"
            );
        }
        other => panic!(
            "expected profile-mismatch Domain error for legacy sidecar + prod apply; got: {other:?}"
        ),
    }
}

// Scenario: preflight fails on module hash mismatch.
#[test]
fn preflight_fails_on_module_hash_mismatch() {
    use ail_runtime::{CapabilityManifest, ResourceLimits, RuntimeHost, RuntimeProfile};

    let wasm_bytes: &[u8] = b"not-real-wasm";
    let wrong_module_hash = "0".repeat(64);

    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![],
    };
    let manifest_hash = manifest.blake3_hex().expect("manifest hash must succeed");

    let profile = RuntimeProfile::new(
        "test".to_string(),
        wrong_module_hash,
        String::new(),
        manifest_hash,
        vec![],
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
            ..Default::default()
        },
    );

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(wasm_bytes, &manifest, &profile);

    assert!(result.is_err(), "must fail when module_hash mismatches");
    let err_str = format!("{}", result.unwrap_err());
    assert!(
        err_str.contains("preflight failed"),
        "error must mention 'preflight failed'; got: {err_str}"
    );
}

// Spec scenario: stale base rejected.
#[test]
fn apply_stale_base_returns_rebase_required() {
    use ail_change::apply::apply as apply_changeset;
    use ail_change::canonical::{CanonicalChangeSet, CanonicalMeta};
    use ail_change::model::{ChangeSetOutcome, Timestamp};
    use ail_core::semantic_graph::SemanticGraph;

    let bridge = SimpleSnapshotBridge(SnapshotId(1));
    let mut graph = SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };

    let canonical = CanonicalChangeSet {
        meta: CanonicalMeta {
            author: "test".to_string(),
            description: "stale-base test".to_string(),
            timestamp: Timestamp(0),
        },
        base_snapshot_id: SnapshotId(0),
        preconditions: vec![],
        ops: vec![],
        ..Default::default()
    };

    let outcome = apply_changeset(canonical, &mut graph, &bridge);
    assert!(
        matches!(
            outcome,
            ChangeSetOutcome::RebaseRequired {
                current_snapshot_id: SnapshotId(1)
            }
        ),
        "stale base must return RebaseRequired; got: {outcome:?}"
    );
}

// Scenario: cmd_diff with range notation returns semantic diff.
#[tokio::test]
async fn cmd_diff_with_range_fails_gracefully_on_missing_snapshots() {
    use crate::store::memory_store;
    let store = memory_store();
    let a = "a".repeat(64);
    let b = "b".repeat(64);
    let result = cmd_diff(OutputMode::Human, &format!("{a}..{b}"), None, false, &store).await;
    // Both snapshots don't exist — expect NotFound.
    assert!(
        matches!(result, Err(CliError::NotFound(_))),
        "diff of missing snapshots must be NotFound; got: {result:?}"
    );
}

// Scenario: cmd_diff --semantic on a named change returns structural diff.
#[tokio::test]
async fn cmd_diff_semantic_returns_structural_diff() {
    use crate::store::memory_store;
    let store = memory_store();
    let result = cmd_diff(OutputMode::Human, "change.add_checkout", None, true, &store).await;
    assert!(
        result.is_ok(),
        "semantic diff must succeed; got: {result:?}"
    );
}

// Scenario: make_text_changeset creates a ChangeSet from text.
#[test]
fn make_text_changeset_from_description() {
    let cs = make_text_changeset("add pure cart_total function");
    assert_eq!(cs.meta.description, "add pure cart_total function");
    assert_eq!(cs.meta.author, "cli");
}

// Scenario: build_structural_diff_preview reflects op count.
#[test]
fn build_structural_diff_preview_counts_ops() {
    use ail_change::model::ChangeSetOp;
    let ops: Vec<ChangeSetOp> = vec![];
    let diff = build_structural_diff_preview(&ops);
    assert_eq!(diff["creates"], 0);
}
