use super::*;
use crate::parser::parse_changeset;
use ail_core::semantic_graph::{EdgeKind, NodeKind, Visibility, WorkflowState};

fn minimal_change(op: &str) -> String {
    format!("change e2e base=0\nauthor tester\ndescription e2e\nop {op}\nend\n")
}

#[test]
fn canonicalize_parsed_materializes_function_create_payload() {
    let parsed = parse_changeset(&minimal_change(
        "create_function id=fn.answer return=Int value=42",
    ))
    .expect("fixture must parse");

    let canonical = canonicalize_parsed(parsed);

    match &canonical.ops[0].payload {
        OpPayload::CreateNode(node) => {
            assert_eq!(node.kind, NodeKind::Function);
            assert_eq!(node.name, "fn.answer");
            assert_eq!(node.return_type.as_deref(), Some("Int"));
            assert_eq!(
                node.runtime_checks
                    .as_ref()
                    .map(|checks| checks[0].predicate.as_str()),
                Some("literal:i64=42")
            );
        }
        other => panic!("expected function CreateNode payload, got {other:?}"),
    }
}

#[test]
fn canonicalize_parsed_materializes_function_body_expr_on_create() {
    let parsed = parse_changeset(&minimal_change(
        "create_function id=fn.add return=Int body=add(x, y)",
    ))
    .expect("fixture must parse");

    let canonical = canonicalize_parsed(parsed);

    match &canonical.ops[0].payload {
        OpPayload::CreateNode(node) => {
            assert_eq!(node.body_expr.as_deref(), Some("add(x, y)"));
        }
        other => panic!("expected function CreateNode payload, got {other:?}"),
    }
}

#[test]
fn canonicalize_parsed_materializes_type_create_payload() {
    let parsed =
        parse_changeset(&minimal_change("create_type id=type.Answer")).expect("fixture must parse");

    let canonical = canonicalize_parsed(parsed);

    match &canonical.ops[0].payload {
        OpPayload::CreateNode(node) => {
            assert_eq!(node.kind, NodeKind::Type);
            assert_eq!(node.name, "type.Answer");
            assert_eq!(
                node.type_facts.as_ref().map(|facts| facts.nominal.as_str()),
                Some("Answer")
            );
        }
        other => panic!("expected type CreateNode payload, got {other:?}"),
    }
}

#[test]
fn canonicalize_parsed_materializes_representable_op_payloads() {
    let source = "\
change e2e base=0
author tester
description e2e
op create_module id=module.checkout
op create_capability id=cap.payment.charge
op create_function id=fn.checkout
op add_param target=fn.checkout name=cart_id type=CartId
op set_return target=fn.checkout type=OrderId
op set_body target=fn.checkout body=@expr.checkout
op add_effect target=fn.checkout effect=payment.charge
op add_contract target=fn.checkout kind=ensures rule=order_created
op connect source=fn.checkout relation=uses target=cap.payment.charge
op disconnect source=fn.checkout relation=uses target=cap.payment.charge
op grant target=module.checkout capability=payment.charge
op revoke target=module.checkout capability=payment.charge
op rename target=fn.checkout name=fn.checkout_v2
op move target=fn.checkout_v2 to=module.checkout
op deprecate target=fn.checkout_v2 replacement=fn.checkout_v3
op annotate target=fn.checkout_v2 key=rationale value=idempotent
op remove_effect target=fn.checkout_v2 effect=payment.charge
op remove_contract target=fn.checkout_v2 rule=order_created
op delete target=fn.checkout_v2
end
";
    let canonical = canonicalize_parsed(parse_changeset(source).expect("fixture must parse"));

    assert!(matches!(canonical.ops[0].payload, OpPayload::CreateNode(_)));
    assert!(
        canonical
            .ops
            .iter()
            .any(|op| matches!(op.payload, OpPayload::AddParamByName { .. }))
    );
    assert!(
        canonical
            .ops
            .iter()
            .any(|op| matches!(op.payload, OpPayload::SetReturnByName { .. }))
    );
    assert!(
        canonical
            .ops
            .iter()
            .any(|op| matches!(op.payload, OpPayload::AddEffectByName { .. }))
    );
    assert!(
        canonical
            .ops
            .iter()
            .any(|op| matches!(op.payload, OpPayload::AddContractByName { .. }))
    );
    assert!(canonical.ops.iter().any(|op| matches!(
        op.payload,
        OpPayload::AddEdgeByName {
            kind: EdgeKind::DependsOn,
            ..
        }
    )));
    assert!(
        canonical
            .ops
            .iter()
            .any(|op| matches!(op.payload, OpPayload::RemoveEdgeByName { .. }))
    );
    assert!(
        canonical
            .ops
            .iter()
            .any(|op| matches!(op.payload, OpPayload::AddCapabilityReqByName { .. }))
    );
    assert!(
        canonical
            .ops
            .iter()
            .any(|op| matches!(op.payload, OpPayload::RemoveCapabilityReqByName { .. }))
    );
    assert!(
        canonical
            .ops
            .iter()
            .any(|op| matches!(op.payload, OpPayload::RenameNodeByName { .. }))
    );
    assert!(
        canonical
            .ops
            .iter()
            .any(|op| matches!(op.payload, OpPayload::SetBodyByName { .. }))
    );
    assert!(
        canonical
            .ops
            .iter()
            .any(|op| matches!(op.payload, OpPayload::RemoveEffectByName { .. }))
    );
    assert!(
        canonical
            .ops
            .iter()
            .any(|op| matches!(op.payload, OpPayload::RemoveContractByName { .. }))
    );
    assert!(
        canonical
            .ops
            .iter()
            .any(|op| matches!(op.payload, OpPayload::RemoveNodeByName(_)))
    );
}

// ── Gap 1: infer_boundary materialization tests ───────────────────────

// Scenario IB-1: infer_boundary with explicit type arg materializes set_return op.
//   GIVEN a changeset with `op infer_boundary target=fn.checkout type=OrderId`
//   WHEN canonicalize_parsed is called
//   THEN the canonical ops include both AddInferredFactByName AND SetReturnByName
#[test]
fn infer_boundary_with_type_emits_set_return_op() {
    let parsed = parse_changeset(&minimal_change(
        "infer_boundary target=fn.checkout type=OrderId",
    ))
    .expect("fixture must parse");

    let canonical = canonicalize_parsed(parsed);

    assert!(
        canonical
            .ops
            .iter()
            .any(|op| matches!(op.payload, OpPayload::AddInferredFactByName { .. })),
        "must retain AddInferredFactByName as documentation"
    );
    assert!(
        canonical.ops.iter().any(
            |op| matches!(op.payload, OpPayload::SetReturnByName { ref target, ref ty }
                    if target == "fn.checkout" && ty == "OrderId")
        ),
        "must emit explicit SetReturnByName for the inferred return type"
    );
}

// TRIANGULATE: infer_boundary with explicit effect arg materializes add_effect op.
#[test]
fn infer_boundary_with_effect_emits_add_effect_op() {
    let parsed = parse_changeset(&minimal_change(
        "infer_boundary target=fn.checkout effect=payment.charge",
    ))
    .expect("fixture must parse");

    let canonical = canonicalize_parsed(parsed);

    assert!(
        canonical.ops.iter().any(
            |op| matches!(op.payload, OpPayload::AddEffectByName { ref target, ref effect }
                    if target == "fn.checkout" && effect == "payment.charge")
        ),
        "must emit explicit AddEffectByName for the inferred effect"
    );
}

// Scenario IB-2: infer_boundary picks up return type from create_function in same changeset.
//   GIVEN a changeset with create_function + infer_boundary for the same target
//   WHEN canonicalize_parsed is called
//   THEN canonical ops include SetReturnByName derived from create_function's return arg
#[test]
fn infer_boundary_derives_return_from_create_function() {
    let source = "\
change e2e base=0
author tester
description e2e
op create_function id=fn.checkout return=OrderId
op infer_boundary target=fn.checkout
end
";
    let canonical = canonicalize_parsed(parse_changeset(source).expect("fixture must parse"));

    assert!(
        canonical.ops.iter().any(
            |op| matches!(op.payload, OpPayload::SetReturnByName { ref target, ref ty }
                    if target == "fn.checkout" && ty == "OrderId")
        ),
        "must emit SetReturnByName derived from create_function return"
    );
}

// TRIANGULATE: infer_boundary picks up effects from add_effect ops in same changeset.
#[test]
fn infer_boundary_derives_effects_from_add_effect_ops() {
    let source = "\
change e2e base=0
author tester
description e2e
op create_function id=fn.checkout return=OrderId
op add_effect target=fn.checkout effect=payment.charge
op infer_boundary target=fn.checkout
end
";
    let canonical = canonicalize_parsed(parse_changeset(source).expect("fixture must parse"));

    // The AddInferredFactByName must still be present as documentation.
    assert!(
        canonical
            .ops
            .iter()
            .any(|op| matches!(op.payload, OpPayload::AddInferredFactByName { .. })),
        "must retain AddInferredFactByName"
    );
    // An explicit AddEffectByName must be synthesized (from add_effect scanning).
    assert!(
        canonical.ops.iter().any(|op| matches!(
            op.payload,
            OpPayload::AddEffectByName { ref target, ref effect }
                if target == "fn.checkout" && effect == "payment.charge"
        )),
        "must emit AddEffectByName derived from sibling add_effect op"
    );
}

#[test]
fn canonicalize_parsed_materializes_semantic_graph_payloads() {
    let source = "\
change e2e base=0
author tester
description e2e
op infer_boundary target=fn.checkout
op bind_handler capability=payment.charge handler=handler.Stripe profile=prod
op expose target=fn.checkout as=api.checkout
op hide target=fn.internal
op derive_eq target=type.Address mode=structural
op generate_tests target=fn.checkout from=contracts
op assert_exists target=fn.checkout
op lock_behavior target=fn.checkout
op refactor_inline target=fn.old_helper
op migrate_api target=fn.checkout from=sig.v1 to=sig.v2
op approve_inferred_boundary target=fn.checkout version=sig_123
op reject_inferred_boundary target=fn.checkout version=sig_124
op verify target=fn.checkout
end
";
    let canonical = canonicalize_parsed(parse_changeset(source).expect("fixture must parse"));

    assert!(
        canonical
            .ops
            .iter()
            .any(|op| matches!(op.payload, OpPayload::AddInferredFactByName { .. }))
    );
    assert!(
        canonical
            .ops
            .iter()
            .any(|op| matches!(op.payload, OpPayload::AddBindingByName { .. }))
    );
    assert!(canonical.ops.iter().any(|op| matches!(
        op.payload,
        OpPayload::SetVisibilityByName {
            visibility: Visibility::Public,
            ..
        }
    )));
    assert!(canonical.ops.iter().any(|op| matches!(
        op.payload,
        OpPayload::SetVisibilityByName {
            visibility: Visibility::Private,
            ..
        }
    )));
    assert!(
        canonical
            .ops
            .iter()
            .any(|op| matches!(op.payload, OpPayload::AddDerivedImplByName { .. }))
    );
    assert!(
        canonical
            .ops
            .iter()
            .any(|op| matches!(op.payload, OpPayload::AddGeneratedArtifactByName { .. }))
    );
    assert!(
        canonical
            .ops
            .iter()
            .any(|op| matches!(op.payload, OpPayload::AddAssertionByName { .. }))
    );
    for state in [
        WorkflowState::Locked,
        WorkflowState::Refactoring,
        WorkflowState::Migrating,
        WorkflowState::Approved,
        WorkflowState::Rejected,
        WorkflowState::Verified,
    ] {
        assert!(canonical.ops.iter().any(|op| matches!(
            op.payload,
            OpPayload::SetWorkflowStateByName { state: actual, .. } if actual == state
        )));
    }
    assert!(
        canonical
            .ops
            .iter()
            .all(|op| !matches!(op.payload, OpPayload::Noop))
    );
}
