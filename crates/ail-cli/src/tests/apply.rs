use super::*;

// Scenario: cmd_apply refuses valid-looking ids when no ChangeSet payload exists.
#[tokio::test]
async fn cmd_apply_memory_store_requires_stored_changeset() {
    use crate::store::memory_store;
    let store = memory_store();
    let id = "b".repeat(64);
    let result = cmd_apply(OutputMode::Human, &id, false, None, &store).await;
    assert!(
        matches!(result, Err(CliError::NotFound(_))),
        "cmd_apply must reject missing ChangeSet payload; got: {result:?}"
    );
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
//   GIVEN a valid change-id and profile=prod
//   WHEN yes=true
//   THEN cmd_apply proceeds (does not return a policy error)
#[tokio::test]
async fn cmd_apply_allows_prod_with_yes() {
    use crate::store::memory_store;
    use ail_change::canonical::CanonicalChangeSet;

    let store = memory_store();
    let canonical = CanonicalChangeSet::default();
    let mut cbor_bytes = Vec::new();
    ciborium::into_writer(&canonical, &mut cbor_bytes).expect("CBOR encode must succeed");
    let change_id = ail_storage::object::ObjectId::from_bytes(&cbor_bytes).to_hex();
    store
        .save_changeset_payload(&change_id, &cbor_bytes)
        .await
        .expect("save must succeed");

    let result = cmd_apply(OutputMode::Human, &change_id, true, Some("prod"), &store).await;
    assert!(
        result.is_ok(),
        "prod with --yes must succeed; got: {result:?}"
    );
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
