// ── ail-verify::pipeline_depth (integration) ─────────────────────────────
//
// TASK-29: Integration test — full pipeline depth.
//
// Builds a SemanticGraph exercising ALL new checker paths added in
// ola3-verify-depth simultaneously, runs VerificationPipeline::run_with_changeset,
// and asserts every depth-extension assertion in a single pipeline execution:
//
//  1. blocking=true  for every Failed entry
//  2. Structural diff entries present (stable_fn type_facts changed)
//  3. Resource lifecycle edge detection fires (resource_db, no edge)
//  4. Concurrency scope boundary fires (orphan_task, no parent scope)
//  5. Assumption lifecycle catches expired boundary (expired_api)
//  6. Semantic compose_check upgrades target_fn obligation to RuntimeChecked

use ail_core::semantic_graph::{
    ContractClauses, GraphNode, NodeKind, NodeRef, SemanticGraph, TrustLevel, TrustMetadata,
    TypeFacts,
};
use ail_verify::pipeline::{PipelineContext, VerificationPipeline};
use ail_verify::proof::ObligationState;
use ail_verify::report::VerificationState;
use ail_verify::solver::SimpleSolver;

// ── Graph builders ────────────────────────────────────────────────────────

/// Target graph — exercises all new checker paths:
///
/// | NodeRef | Name          | What it exercises                                      |
/// |---------|---------------|--------------------------------------------------------|
/// | 0       | `stable_fn`   | Structural diff (type_facts changes Int → String)      |
/// | 1       | `resource_db` | ResourceChecker edge detection (no lifecycle edge/tag) |
/// | 2       | `orphan_task` | ConcurrencyChecker scope boundary (no parent edge)     |
/// | 3       | `expired_api` | BoundaryChecker assumption lifecycle (expired)         |
/// | 4       | `peer_fn`     | Compose source: ensures "x >= 1"                      |
/// | 5       | `target_fn`   | Compose target: requires "x > 0" → RuntimeChecked     |
fn make_target_graph() -> SemanticGraph {
    // NodeRef(0): changed function — diff vs base_graph triggers Unverified diff entry
    let mut stable_fn = GraphNode::new(NodeRef(0), NodeKind::Function, "stable_fn");
    stable_fn.type_facts = Some(TypeFacts {
        nominal: "String".into(), // changed from "Int" in base
        generics: vec![],
    });

    // NodeRef(1): linear resource with no lifecycle tag and no Consumes/Releases edge
    // → ResourceChecker emits Unverified with E_RESOURCE_NO_LIFECYCLE_EDGE (T-18)
    let mut resource_db = GraphNode::new(NodeRef(1), NodeKind::Type, "resource_db");
    resource_db.trust_metadata = Some(TrustMetadata {
        level: TrustLevel::Custom("resource:linear".into()),
        tags: vec![], // no "released" or violation tags
    });

    // NodeRef(2): task with no lifecycle tag and no SpawnedBy/ChildOf edge to TaskGroup
    // → ConcurrencyChecker emits Unverified "potential orphan scope" (T-20)
    let mut orphan_task = GraphNode::new(NodeRef(2), NodeKind::Type, "orphan_task");
    orphan_task.trust_metadata = Some(TrustMetadata {
        level: TrustLevel::Custom("task".into()),
        tags: vec![], // no "awaited"/"cancelled"/"transferred"
    });

    // NodeRef(3): boundary with has-assumption-expired
    // → BoundaryChecker emits Failed, blocking=true (T-22)
    let mut expired_api = GraphNode::new(NodeRef(3), NodeKind::Boundary, "expired_api");
    expired_api.trust_metadata = Some(TrustMetadata {
        level: TrustLevel::Custom("boundary".into()),
        tags: vec!["has-assumption-expired".into()],
    });

    // NodeRef(4): peer function ensuring "x >= 1"
    // → ProofObligationPipeline compose stage: semantic_implies("x > 0", "x >= 1") → true
    let mut peer_fn = GraphNode::new(NodeRef(4), NodeKind::Function, "peer_fn");
    peer_fn.contract_clauses = Some(ContractClauses {
        requires: vec![],
        ensures: vec!["x >= 1".into()],
    });

    // NodeRef(5): target function requiring "x > 0"
    // → SimpleSolver returns Unsupported; compose finds peer_fn ensures "x >= 1"
    //   which semantically implies "x > 0"; obligation upgraded to RuntimeChecked (T-26)
    let mut target_fn = GraphNode::new(NodeRef(5), NodeKind::Function, "target_fn");
    target_fn.contract_clauses = Some(ContractClauses {
        requires: vec!["x > 0".into()],
        ensures: vec![],
    });

    SemanticGraph {
        nodes: vec![
            stable_fn,
            resource_db,
            orphan_task,
            expired_api,
            peer_fn,
            target_fn,
        ],
        edges: vec![],
    }
}

/// Base graph — only contains `stable_fn` with old type_facts (nominal: "Int").
/// Diffing this against the target triggers a structural diff Unverified entry.
fn make_base_graph() -> SemanticGraph {
    let mut stable_fn = GraphNode::new(NodeRef(0), NodeKind::Function, "stable_fn");
    stable_fn.type_facts = Some(TypeFacts {
        nominal: "Int".into(),
        generics: vec![],
    });
    SemanticGraph {
        nodes: vec![stable_fn],
        edges: vec![],
    }
}

fn make_ctx<'a>(graph: &'a SemanticGraph, solver: &'a SimpleSolver) -> PipelineContext<'a> {
    PipelineContext {
        graph,
        manifests: &[],
        profile: "test",
        solver,
        approvals: &[],
        rules: &[],
        structural_diff: None,
        capability_grants: &[],
        public_api_changes: &[],
        package_trust_metadata: &[],
        artifacts: &[],
        manifest_caps: &[],
        artifact_manifest_hash: None,
    }
}

// ── Helper: run the pipeline once with base graph ─────────────────────────

fn run_full_pipeline() -> ail_verify::report::VerificationReport {
    let target = make_target_graph();
    let base = make_base_graph();
    let solver = SimpleSolver;
    let ctx = make_ctx(&target, &solver);
    VerificationPipeline::run_with_changeset(&ctx, None, Some(&base))
}

// ── T-29 assertions ───────────────────────────────────────────────────────

// 1. blocking=true for every Failed entry
#[test]
fn depth_blocking_true_for_all_failed_entries() {
    let report = run_full_pipeline();

    let failed: Vec<_> = report
        .entries
        .iter()
        .filter(|e| e.state == VerificationState::Failed)
        .collect();

    assert!(!failed.is_empty(), "must have at least one Failed entry in the full-depth run");

    for entry in &failed {
        assert!(
            entry.blocking,
            "Failed entry (claim='{}', scope='{}') must have blocking=true",
            entry.claim,
            entry.scope,
        );
    }
}

// 2. Structural diff entries present — stable_fn type_facts changed → Unverified
#[test]
fn depth_structural_diff_entries_present() {
    let report = run_full_pipeline();

    let diff_entries: Vec<_> = report
        .entries
        .iter()
        .filter(|e| e.claim == "05-build-semantic-diff")
        .collect();

    assert!(
        !diff_entries.is_empty(),
        "must have at least one entry from stage 05-build-semantic-diff"
    );

    // stable_fn has different type_facts (Int → String) → Unverified diff entry
    let changed = diff_entries
        .iter()
        .find(|e| e.scope == "stable_fn" && e.state == VerificationState::Unverified);
    assert!(
        changed.is_some(),
        "must have Unverified diff entry for 'stable_fn' (type_facts changed Int → String)"
    );
}

// 3. Resource lifecycle edge detection fires — resource_db linear, no edge → Unverified
#[test]
fn depth_resource_lifecycle_edge_detection_fires() {
    let report = run_full_pipeline();

    // ResourceChecker uses claim = "resource-lifecycle[<level>]"
    let resource_entry = report
        .entries
        .iter()
        .find(|e| {
            e.claim == "resource-lifecycle[resource:linear]"
                && e.scope == "resource_db"
                && e.state == VerificationState::Unverified
        });

    assert!(
        resource_entry.is_some(),
        "must have Unverified resource-lifecycle entry for 'resource_db' (no lifecycle edge or tag)"
    );

    let evidence = resource_entry.unwrap().evidence.as_deref().unwrap_or("");
    assert!(
        evidence.contains("E_RESOURCE_NO_LIFECYCLE_EDGE"),
        "resource_db evidence must cite E_RESOURCE_NO_LIFECYCLE_EDGE; got: {evidence}"
    );
}

// 4. Concurrency scope boundary fires — orphan_task, no lifecycle tag/edge → Unverified
#[test]
fn depth_concurrency_scope_boundary_fires() {
    let report = run_full_pipeline();

    // ConcurrencyChecker uses claim = "concurrency-safety[<level>]"
    let task_entry = report
        .entries
        .iter()
        .find(|e| {
            e.claim == "concurrency-safety[task]"
                && e.scope == "orphan_task"
                && e.state == VerificationState::Unverified
        });

    assert!(
        task_entry.is_some(),
        "must have Unverified concurrency-safety entry for 'orphan_task' (no lifecycle tag or parent scope)"
    );

    let evidence = task_entry.unwrap().evidence.as_deref().unwrap_or("");
    assert!(
        evidence.contains("potential orphan scope"),
        "orphan_task evidence must cite 'potential orphan scope'; got: {evidence}"
    );
}

// 5. Assumption lifecycle catches expired boundary — expired_api → Failed + blocking
#[test]
fn depth_assumption_lifecycle_catches_expired_boundary() {
    let report = run_full_pipeline();

    let boundary_entry = report
        .entries
        .iter()
        .find(|e| e.scope == "expired_api" && e.state == VerificationState::Failed);

    assert!(
        boundary_entry.is_some(),
        "must have Failed entry for 'expired_api' (boundary assumption expired)"
    );

    assert!(
        boundary_entry.unwrap().blocking,
        "expired_api Failed entry must have blocking=true"
    );
}

// 6. Semantic compose_check upgrades target_fn "x > 0" obligation to RuntimeChecked
//    because peer_fn ensures "x >= 1" and semantic_implies("x > 0", "x >= 1") is true
#[test]
fn depth_semantic_compose_upgrades_obligation_to_runtime_checked() {
    let report = run_full_pipeline();

    let upgraded = report.proof_obligations.iter().find(|o| {
        o.obligation.scope == "target_fn"
            && o.obligation.predicate == "x > 0"
            && o.state == ObligationState::RuntimeChecked
    });

    assert!(
        upgraded.is_some(),
        "target_fn 'x > 0' obligation must be RuntimeChecked: \
         peer_fn ensures 'x >= 1' semantically implies 'x > 0' (integer arithmetic)"
    );
}
