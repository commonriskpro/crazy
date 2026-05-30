use super::*;

/// Derive runtime `CapabilityId`s from graph declarations and emitted effects.
///
/// Graph `capability_reqs` preserve explicit ACL grants. ANF `EffectCall`
/// collection follows calls reachable from the selected target, hardening the
/// runtime manifest when a function body emits an effect that the graph forgot
/// to declare while keeping missing grants in preflight instead of deferring
/// them to invocation time.
///
/// Returns an empty `Vec` only when neither graph declarations nor target ANF
/// effects require external capabilities.
pub(crate) fn derive_runtime_capability_ids(
    graph: &SemanticGraph,
    anf: &AnfIr,
    target: &str,
) -> Vec<CapabilityId> {
    use std::collections::{BTreeMap, BTreeSet};

    let mut unique = BTreeSet::new();
    if let Some(node) = graph.nodes.iter().find(|node| node.name == target) {
        if let Some(reqs) = &node.capability_reqs {
            unique.extend(reqs.caps.iter().cloned());
        }
    } else {
        unique.extend(
            graph
                .nodes
                .iter()
                .filter_map(|n| n.capability_reqs.as_ref())
                .flat_map(|reqs| reqs.caps.iter().cloned()),
        );
    }

    let binding_exprs: BTreeMap<&str, &AnfExpr> = anf
        .bindings
        .iter()
        .map(|binding| (binding.name.as_str(), &binding.expr))
        .collect();
    let target_binding_exists = anf
        .bindings
        .iter()
        .any(|binding| binding_matches_target(&binding.name, target));
    let mut visited = BTreeSet::new();
    for binding in &anf.bindings {
        if !target_binding_exists || binding_matches_target(&binding.name, target) {
            collect_binding_capability_ids(
                graph,
                &binding_exprs,
                &binding.name,
                &mut visited,
                &mut unique,
            );
        }
    }

    unique.into_iter().map(CapabilityId::new).collect()
}

fn binding_matches_target(binding_name: &str, target: &str) -> bool {
    binding_name == target
        || binding_name.rsplit('.').next() == Some(target)
        || target.rsplit('.').next() == Some(binding_name)
}

fn collect_binding_capability_ids<'a>(
    graph: &SemanticGraph,
    binding_exprs: &std::collections::BTreeMap<&'a str, &'a AnfExpr>,
    binding_name: &str,
    visited: &mut std::collections::BTreeSet<String>,
    unique: &mut std::collections::BTreeSet<String>,
) {
    if !visited.insert(binding_name.to_string()) {
        return;
    }

    if let Some(reqs) = graph
        .nodes
        .iter()
        .find(|node| binding_matches_target(&node.name, binding_name))
        .and_then(|node| node.capability_reqs.as_ref())
    {
        unique.extend(reqs.caps.iter().cloned());
    }

    if let Some(expr) = binding_exprs.get(binding_name).copied() {
        collect_effect_capability_ids(graph, expr, binding_exprs, visited, unique);
    }
}

fn collect_effect_capability_ids<'a>(
    graph: &SemanticGraph,
    expr: &AnfExpr,
    binding_exprs: &std::collections::BTreeMap<&'a str, &'a AnfExpr>,
    visited: &mut std::collections::BTreeSet<String>,
    unique: &mut std::collections::BTreeSet<String>,
) {
    match expr {
        AnfExpr::EffectCall { capability, .. } => {
            unique.insert(capability.clone());
        }
        AnfExpr::Let { value, body, .. } => {
            collect_effect_capability_ids(graph, value, binding_exprs, visited, unique);
            collect_effect_capability_ids(graph, body, binding_exprs, visited, unique);
        }
        AnfExpr::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_effect_capability_ids(graph, then_branch, binding_exprs, visited, unique);
            collect_effect_capability_ids(graph, else_branch, binding_exprs, visited, unique);
        }
        AnfExpr::Return(value) | AnfExpr::Loop { body: value } | AnfExpr::Break { value } => {
            collect_effect_capability_ids(graph, value, binding_exprs, visited, unique);
        }
        AnfExpr::Seq(exprs) | AnfExpr::TupleNew(exprs) | AnfExpr::ListNew(exprs) => {
            for expr in exprs {
                collect_effect_capability_ids(graph, expr, binding_exprs, visited, unique);
            }
        }
        AnfExpr::Match { arms, .. } => {
            for arm in arms {
                collect_effect_capability_ids(graph, &arm.body, binding_exprs, visited, unique);
            }
        }
        AnfExpr::Lambda { body, .. }
        | AnfExpr::WhileLoop { body, .. }
        | AnfExpr::ShortCircuitAnd { right: body, .. }
        | AnfExpr::ShortCircuitOr { right: body, .. }
        | AnfExpr::TaskGroup { body }
        | AnfExpr::Timeout { body, .. }
        | AnfExpr::ForEach { body, .. } => {
            collect_effect_capability_ids(graph, body, binding_exprs, visited, unique);
        }
        AnfExpr::RecordNew { fields } => {
            for (_, expr) in fields {
                collect_effect_capability_ids(graph, expr, binding_exprs, visited, unique);
            }
        }
        AnfExpr::FieldUpdate { value, .. } => {
            collect_effect_capability_ids(graph, value, binding_exprs, visited, unique);
        }
        AnfExpr::VariantNew { payload, .. } => {
            if let Some(payload) = payload {
                collect_effect_capability_ids(graph, payload, binding_exprs, visited, unique);
            }
        }
        AnfExpr::Select { branches } => {
            for branch in branches {
                collect_effect_capability_ids(graph, &branch.body, binding_exprs, visited, unique);
            }
        }
        AnfExpr::Call { func, .. } => {
            for binding_name in resolve_called_binding_names(binding_exprs, func) {
                collect_binding_capability_ids(graph, binding_exprs, binding_name, visited, unique);
            }
        }
        AnfExpr::Literal(_)
        | AnfExpr::Var(_)
        | AnfExpr::FieldGet { .. }
        | AnfExpr::Dispatch { .. }
        | AnfExpr::TaskSpawn { .. }
        | AnfExpr::ChannelSend { .. }
        | AnfExpr::ChannelReceive { .. }
        | AnfExpr::RuntimeCheck { .. }
        | AnfExpr::ResourceAcquire { .. }
        | AnfExpr::ResourceRelease { .. }
        | AnfExpr::TaskAwait { .. }
        | AnfExpr::TaskCancel { .. }
        | AnfExpr::ChannelNew { .. }
        | AnfExpr::CellNew { .. }
        | AnfExpr::CellGet { .. }
        | AnfExpr::CellSet { .. }
        | AnfExpr::Assume { .. }
        | AnfExpr::Abort { .. }
        | AnfExpr::IndexGet { .. }
        | AnfExpr::MapNew { .. }
        | AnfExpr::SetNew { .. }
        | AnfExpr::Fold { .. }
        | AnfExpr::Continue
        | AnfExpr::Placeholder => {}
    }
}

fn resolve_called_binding_names<'a>(
    binding_exprs: &std::collections::BTreeMap<&'a str, &'a AnfExpr>,
    func: &str,
) -> Vec<&'a str> {
    if let Some((name, _)) = binding_exprs.get_key_value(func) {
        return vec![*name];
    }

    binding_exprs
        .keys()
        .copied()
        .filter(|binding_name| binding_matches_target(binding_name, func))
        .collect()
}
