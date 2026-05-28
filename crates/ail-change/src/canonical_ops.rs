// ── ail-change::canonical::canonical_ops ─────────────────────────────────
//
// Payload materialization helpers for `CanonicalOp` construction.
//
// Included inline from `canonical.rs` via `#[path = "canonical_ops.rs"]`.
// All items are private to this module; only `materialize_payload` is
// `pub(super)` so `canonical_parsed_inner` can call it.

use ail_core::semantic_graph::{
    Assertion, Binding, CapabilityReqs, EdgeKind, GeneratedArtifact, GraphNode, InferredFact,
    NodeKind, NodeRef, RuntimeCheckMeta, TypeFacts, Visibility, WorkflowState,
};

use crate::model::ChangeSetOp;
use crate::parser::OpArgs;

use super::OpPayload;

// ── materialize_payload ───────────────────────────────────────────────────

pub(super) fn materialize_payload(
    idx: usize,
    kind: &ChangeSetOp,
    verb: &str,
    args: &OpArgs,
) -> OpPayload {
    match (kind, verb) {
        (ChangeSetOp::Create, "create_module") => args
            .get("id")
            .map(|id| OpPayload::CreateNode(Box::new(module_node(idx, id))))
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Create, "create_type") => args
            .get("id")
            .map(|id| OpPayload::CreateNode(Box::new(type_node(idx, id))))
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Create, "create_function") => args
            .get("id")
            .map(|id| OpPayload::CreateNode(Box::new(function_node(idx, id, args))))
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Create, "create_test") => args
            .get("id")
            .map(|id| OpPayload::CreateNode(Box::new(test_node(idx, id, args))))
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Create, "create_capability") => args
            .get("id")
            .map(|id| OpPayload::CreateNode(Box::new(capability_node(idx, id))))
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Set, "set_return") => target_and(args, "type")
            .map(|(target, ty)| OpPayload::SetReturnByName { target, ty })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Set, "set_body") => target_and(args, "body")
            .map(|(target, body)| OpPayload::SetBodyByName { target, body })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Set, _) | (ChangeSetOp::Replace, _) => args
            .get("target")
            .and_then(|target| metadata_arg(args).map(|(key, value)| (target.clone(), key, value)))
            .map(|(target, key, value)| OpPayload::SetMetadataByName { target, key, value })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Add, "add_param") => {
            match (args.get("target"), args.get("name"), args.get("type")) {
                (Some(target), Some(name), Some(ty)) => OpPayload::AddParamByName {
                    target: target.clone(),
                    name: name.clone(),
                    ty: ty.clone(),
                },
                _ => OpPayload::Noop,
            }
        }
        (ChangeSetOp::Add, "add_effect") => target_and(args, "effect")
            .map(|(target, effect)| OpPayload::AddEffectByName { target, effect })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Add, "add_contract") => {
            match (args.get("target"), args.get("kind"), args.get("rule")) {
                (Some(target), Some(kind), Some(rule)) => OpPayload::AddContractByName {
                    target: target.clone(),
                    kind: kind.clone(),
                    rule: rule.clone(),
                },
                _ => OpPayload::Noop,
            }
        }
        (ChangeSetOp::Remove, "remove_effect") => target_and(args, "effect")
            .map(|(target, effect)| OpPayload::RemoveEffectByName { target, effect })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Remove, "remove_contract") => target_and(args, "rule")
            .map(|(target, rule)| OpPayload::RemoveContractByName { target, rule })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Remove, verb) if verb.starts_with("remove_") => args
            .get("target")
            .cloned()
            .map(OpPayload::RemoveNodeByName)
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Delete, _) => args
            .get("target")
            .cloned()
            .map(OpPayload::RemoveNodeByName)
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Rename, _) => match (args.get("target"), args.get("name")) {
            (Some(target), Some(name)) => OpPayload::RenameNodeByName {
                target: target.clone(),
                name: name.clone(),
            },
            _ => OpPayload::Noop,
        },
        (ChangeSetOp::Move, _) => target_and(args, "to")
            .map(|(target, value)| OpPayload::SetMetadataByName {
                target,
                key: "module".to_string(),
                value,
            })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Connect, _) => edge_payload(args, false),
        (ChangeSetOp::Disconnect, _) => edge_payload(args, true),
        (ChangeSetOp::Grant, _) => target_and(args, "capability")
            .map(|(target, capability)| OpPayload::AddCapabilityReqByName { target, capability })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Revoke, _) => target_and(args, "capability")
            .map(|(target, capability)| OpPayload::RemoveCapabilityReqByName { target, capability })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Expose, _) => args
            .get("target")
            .cloned()
            .map(|target| OpPayload::SetVisibilityByName {
                target,
                visibility: Visibility::Public,
            })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Hide, _) => args
            .get("target")
            .cloned()
            .map(|target| OpPayload::SetVisibilityByName {
                target,
                visibility: Visibility::Private,
            })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Bind, _) => match (args.get("capability"), args.get("handler")) {
            (Some(capability), Some(handler)) => OpPayload::AddBindingByName {
                target: capability.clone(),
                binding: Binding {
                    name: capability.clone(),
                    implementation: handler.clone(),
                    profile: args.get("profile").cloned(),
                },
            },
            _ => OpPayload::Noop,
        },
        (ChangeSetOp::Infer, _) => args
            .get("target")
            .cloned()
            .map(|target| OpPayload::AddInferredFactByName {
                target,
                fact: InferredFact {
                    kind: verb_suffix(verb, "infer"),
                    value: inferred_value(args),
                },
            })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Derive, _) => args
            .get("target")
            .cloned()
            .map(|target| OpPayload::AddDerivedImplByName {
                target,
                impl_name: verb_suffix(verb, "derive"),
            })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Generate, _) => args
            .get("target")
            .cloned()
            .map(|target| OpPayload::AddGeneratedArtifactByName {
                target,
                artifact: GeneratedArtifact {
                    kind: verb_suffix(verb, "generate"),
                    source: generated_source(args),
                },
            })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Assert, _) => args
            .get("target")
            .cloned()
            .map(|target| OpPayload::AddAssertionByName {
                target,
                assertion: Assertion {
                    kind: verb_suffix(verb, "assert"),
                    value: assertion_value(args),
                },
            })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Lock, _) => workflow_payload(args, WorkflowState::Locked),
        (ChangeSetOp::Refactor, _) => workflow_target(args).map_or(OpPayload::Noop, |target| {
            OpPayload::SetWorkflowStateByName {
                target,
                state: WorkflowState::Refactoring,
            }
        }),
        (ChangeSetOp::Migrate, _) => workflow_payload(args, WorkflowState::Migrating),
        (ChangeSetOp::Approve, _) => workflow_payload(args, WorkflowState::Approved),
        (ChangeSetOp::Reject, _) => workflow_payload(args, WorkflowState::Rejected),
        (ChangeSetOp::Verify, _) => workflow_payload(args, WorkflowState::Verified),
        (ChangeSetOp::Deprecate, _) => target_and(args, "replacement")
            .map(|(target, value)| OpPayload::SetMetadataByName {
                target,
                key: "deprecated_replacement".to_string(),
                value,
            })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Annotate, _) => {
            match (args.get("target"), args.get("key"), args.get("value")) {
                (Some(target), Some(key), Some(value)) => OpPayload::SetMetadataByName {
                    target: target.clone(),
                    key: key.clone(),
                    value: value.clone(),
                },
                _ => OpPayload::Noop,
            }
        }
        // Raw or malformed ops without enough graph identity remain no-ops.
        _ => OpPayload::Noop,
    }
}

// ── verb / value extractors ───────────────────────────────────────────────

fn verb_suffix(verb: &str, prefix: &str) -> String {
    verb.strip_prefix(prefix)
        .and_then(|rest| rest.strip_prefix('_'))
        .filter(|rest| !rest.is_empty())
        .unwrap_or(prefix)
        .to_string()
}

fn inferred_value(args: &OpArgs) -> String {
    args.get("type")
        .or_else(|| args.get("effect"))
        .or_else(|| args.get("value"))
        .cloned()
        .unwrap_or_else(|| "pending".to_string())
}

fn generated_source(args: &OpArgs) -> Option<String> {
    args.get("from")
        .or_else(|| args.get("language"))
        .or_else(|| args.get("audience"))
        .cloned()
}

fn assertion_value(args: &OpArgs) -> String {
    args.get("hash")
        .or_else(|| args.get("value"))
        .cloned()
        .unwrap_or_else(|| "true".to_string())
}

// ── workflow helpers ──────────────────────────────────────────────────────

fn workflow_payload(args: &OpArgs, state: WorkflowState) -> OpPayload {
    workflow_target(args).map_or(OpPayload::Noop, |target| {
        OpPayload::SetWorkflowStateByName { target, state }
    })
}

fn workflow_target(args: &OpArgs) -> Option<String> {
    args.get("target")
        .or_else(|| args.get("from"))
        .or_else(|| args.get("scope"))
        .cloned()
}

// ── generic arg helpers ───────────────────────────────────────────────────

fn target_and(args: &OpArgs, key: &str) -> Option<(String, String)> {
    Some((args.get("target")?.clone(), args.get(key)?.clone()))
}

fn metadata_arg(args: &OpArgs) -> Option<(String, String)> {
    args.iter()
        .find(|(key, _)| !matches!(key.as_str(), "target" | "source" | "id"))
        .map(|(key, value)| (key.clone(), value.clone()))
}

// ── edge helpers ──────────────────────────────────────────────────────────

fn edge_payload(args: &OpArgs, remove: bool) -> OpPayload {
    match (args.get("source"), args.get("target")) {
        (Some(source), Some(target)) => {
            let kind = args
                .get("relation")
                .map(|relation| edge_kind(relation))
                .unwrap_or(EdgeKind::DependsOn);
            if remove {
                OpPayload::RemoveEdgeByName {
                    source: source.clone(),
                    target: target.clone(),
                    kind,
                }
            } else {
                OpPayload::AddEdgeByName {
                    source: source.clone(),
                    target: target.clone(),
                    kind,
                }
            }
        }
        _ => OpPayload::Noop,
    }
}

fn edge_kind(relation: &str) -> EdgeKind {
    match relation {
        "calls" => EdgeKind::Calls,
        "reads" => EdgeKind::Reads,
        "writes" => EdgeKind::Writes,
        "emits" => EdgeKind::Emits,
        "proves" => EdgeKind::Proves,
        "breaks_if_changed" => EdgeKind::BreaksIfChanged,
        _ => EdgeKind::DependsOn,
    }
}

// ── node constructors ─────────────────────────────────────────────────────

fn module_node(idx: usize, id: &str) -> GraphNode {
    GraphNode::new(NodeRef(idx as u32), NodeKind::Module, id)
}

fn type_node(idx: usize, id: &str) -> GraphNode {
    let mut node = GraphNode::new(NodeRef(idx as u32), NodeKind::Type, id);
    node.type_facts = Some(TypeFacts {
        nominal: id.rsplit('.').next().unwrap_or(id).to_string(),
        generics: vec![],
    });
    node
}

fn function_node(idx: usize, id: &str, args: &OpArgs) -> GraphNode {
    let mut node = GraphNode::new(NodeRef(idx as u32), NodeKind::Function, id);
    node.return_type = args.get("return").cloned();
    node.body_expr = args.get("body").cloned();
    if let Some(value) = args.get("value") {
        let predicate = format!("literal:i64={value}");
        node.runtime_checks = Some(vec![RuntimeCheckMeta {
            hash: blake3::hash(predicate.as_bytes()).to_hex().to_string(),
            predicate,
        }]);
    }
    node
}

fn test_node(idx: usize, id: &str, args: &OpArgs) -> GraphNode {
    let mut node = GraphNode::new(NodeRef(idx as u32), NodeKind::Test, id);
    node.return_type = Some(
        args.get("return")
            .cloned()
            .unwrap_or_else(|| "Bool".to_string()),
    );
    node.body_expr = args.get("body").cloned();
    node
}

fn capability_node(idx: usize, id: &str) -> GraphNode {
    let mut node = GraphNode::new(NodeRef(idx as u32), NodeKind::Capability, id);
    node.capability_reqs = Some(CapabilityReqs {
        caps: vec![id.to_string()],
    });
    node
}
