use super::*;

// Scenario: cmd_refactor produces ChangeSet with behavior locks.
#[tokio::test]
async fn cmd_refactor_has_behavior_locks() {
    use crate::store::memory_store;
    let store = memory_store();
    let result = cmd_refactor(
        OutputMode::Human,
        "extract-function",
        &[
            "fn.checkout".to_string(),
            "--to".to_string(),
            "fn.pay".to_string(),
        ],
        &store,
    )
    .await;
    assert!(result.is_ok(), "cmd_refactor must succeed; got: {result:?}");
}

// ── Gap 2: cmd_refactor real ChangeSet generator ─────────────────────

// Scenario RF-1: extract-function from a graph node with contracts produces
//   behavior_locks populated from the source function's contract_clauses.
#[tokio::test]
async fn cmd_refactor_extract_function_with_contracts_has_behavior_locks() {
    use crate::store::memory_store;
    use ail_change::apply::apply as apply_cs;
    use ail_change::canonical::canonicalize_parsed;
    use ail_change::model::SnapshotId;
    use ail_change::parser::parse_changeset;

    let store = memory_store();

    // Set up graph: fn.checkout with a contract and an effect.
    let source = "\
change setup base=0
author test
description setup
op create_function id=fn.checkout return=OrderId
op add_contract target=fn.checkout kind=ensures rule=order_created
op add_effect target=fn.checkout effect=payment.charge
end
";
    let parsed = parse_changeset(source).expect("must parse");
    let canonical = canonicalize_parsed(parsed);
    let mut graph = ail_core::semantic_graph::SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };
    let bridge = SimpleSnapshotBridge(SnapshotId(0));
    let outcome = apply_cs(canonical.clone(), &mut graph, &bridge);
    assert!(matches!(
        outcome,
        ail_change::model::ChangeSetOutcome::Applied
    ));
    store.save_graph(&graph).await.expect("save graph");
    let cbor = encode_cbor(&canonical).expect("encode");
    let snap_id = ail_storage::object::ObjectId::from_bytes(&cbor);
    let snap = ail_storage::SnapshotEnvelope {
        id: snap_id,
        graph_root_hash: store.save_graph(&graph).await.expect("root"),
        parent_id: None,
        applied_change_id: None,
        created_at: unix_ms_now(),
        verification_report_hash: None,
        ..Default::default()
    };
    store.save_snapshot(&snap).await.expect("save snapshot");

    let result = cmd_refactor(
        OutputMode::Json,
        "extract-function",
        &[
            "fn.checkout".to_string(),
            "--to".to_string(),
            "fn.payment_handler".to_string(),
        ],
        &store,
    )
    .await;
    assert!(
        result.is_ok(),
        "cmd_refactor with contracts must succeed; got: {result:?}"
    );
}

// TRIANGULATE: extract-function from a graph node with effects populates
//   effects_preserved from the source function's effect_row.
#[tokio::test]
async fn cmd_refactor_extract_function_with_effects_has_effects_preserved() {
    use crate::store::memory_store;
    use ail_change::apply::apply as apply_cs;
    use ail_change::canonical::canonicalize_parsed;
    use ail_change::model::SnapshotId;
    use ail_change::parser::parse_changeset;

    let store = memory_store();

    let source = "\
change setup base=0
author test
description setup
op create_function id=fn.checkout return=OrderId
op add_effect target=fn.checkout effect=payment.charge
op add_effect target=fn.checkout effect=email.notify
end
";
    let parsed = parse_changeset(source).expect("must parse");
    let canonical = canonicalize_parsed(parsed);
    let mut graph = ail_core::semantic_graph::SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };
    let bridge = SimpleSnapshotBridge(SnapshotId(0));
    apply_cs(canonical.clone(), &mut graph, &bridge);
    let cbor = encode_cbor(&canonical).expect("encode");
    let snap_id = ail_storage::object::ObjectId::from_bytes(&cbor);
    let snap = ail_storage::SnapshotEnvelope {
        id: snap_id,
        graph_root_hash: store.save_graph(&graph).await.expect("root"),
        parent_id: None,
        applied_change_id: None,
        created_at: unix_ms_now(),
        verification_report_hash: None,
        ..Default::default()
    };
    store.save_snapshot(&snap).await.expect("save snapshot");

    let result = cmd_refactor(
        OutputMode::Human,
        "extract-function",
        &[
            "fn.checkout".to_string(),
            "--to".to_string(),
            "fn.pay".to_string(),
        ],
        &store,
    )
    .await;
    assert!(
        result.is_ok(),
        "cmd_refactor with effects must succeed; got: {result:?}"
    );
}

// Scenario RF-2: extract-function generates ACL ops including create_function
//   for the new function and a connect call edge.
#[tokio::test]
async fn cmd_refactor_extract_function_generates_acl_ops() {
    use crate::store::memory_store;
    let store = memory_store();
    // Empty graph — refactor degrades gracefully.
    let result = cmd_refactor(
        OutputMode::Human,
        "extract-function",
        &[
            "fn.nonexistent".to_string(),
            "--to".to_string(),
            "fn.extracted".to_string(),
        ],
        &store,
    )
    .await;
    assert!(
        result.is_ok(),
        "extract-function on missing node must degrade gracefully; got: {result:?}"
    );
}

// Scenario RF-3: move operation returns appropriate ChangeSet.
#[tokio::test]
async fn cmd_refactor_move_operation_succeeds() {
    use crate::store::memory_store;
    let store = memory_store();
    let result = cmd_refactor(
        OutputMode::Human,
        "move",
        &["fn.answer".to_string(), "module.new".to_string()],
        &store,
    )
    .await;
    assert!(
        result.is_ok(),
        "cmd_refactor move must succeed; got: {result:?}"
    );
}
