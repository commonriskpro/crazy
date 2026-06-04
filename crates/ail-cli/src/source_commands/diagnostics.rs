use super::syntax::*;
use super::types::*;
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceIgnoredExpressionStatement {
    pub(crate) line_num: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceUnusedBinding {
    pub(crate) name: String,
    pub(crate) line_num: usize,
}

pub(crate) fn source_ignored_expression_statement_diagnostics(
    src: &str,
) -> Vec<SourceIgnoredExpressionStatement> {
    let Ok(program) = parse_ail_source(src) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for function in &program.functions {
        collect_ignored_expression_statements(&function.body, &mut out);
    }
    for test in &program.tests {
        collect_ignored_expression_statements(&test.body, &mut out);
    }
    out.sort_by_key(|diagnostic| diagnostic.line_num);
    out.dedup_by_key(|diagnostic| diagnostic.line_num);
    out
}

pub(crate) fn source_unused_binding_diagnostics(src: &str) -> Vec<SourceUnusedBinding> {
    let Ok(program) = parse_ail_source(src) else {
        return Vec::new();
    };
    let mut line_map = source_let_binding_line_map(src);
    let mut out = Vec::new();
    for function in &program.functions {
        collect_unused_bindings(&function.body, &mut line_map, &mut out);
    }
    for test in &program.tests {
        collect_unused_bindings(&test.body, &mut line_map, &mut out);
    }
    out.sort_by_key(|diagnostic| (diagnostic.line_num, diagnostic.name.clone()));
    out.dedup_by(|left, right| left.line_num == right.line_num && left.name == right.name);
    out
}

fn collect_ignored_expression_statements(
    expr: &str,
    out: &mut Vec<SourceIgnoredExpressionStatement>,
) {
    let Some((func, args)) = parse_source_call(expr) else {
        return;
    };

    match (func.as_str(), args.as_slice()) {
        ("let", [name, value, next]) if is_source_statement_binding_name(name) => {
            if !source_expr_has_direct_effect(value)
                && let Some(line_num) = source_statement_binding_line(name)
            {
                out.push(SourceIgnoredExpressionStatement { line_num });
            }
            collect_ignored_expression_statements(value, out);
            collect_ignored_expression_statements(next, out);
        }
        ("let", [_name, value, next]) => {
            collect_ignored_expression_statements(value, out);
            collect_ignored_expression_statements(next, out);
        }
        ("let_typed", [_name, _ty, _line, value, next]) => {
            collect_ignored_expression_statements(value, out);
            collect_ignored_expression_statements(next, out);
        }
        ("if", [condition, then_expr, else_expr]) => {
            collect_ignored_expression_statements(condition, out);
            collect_ignored_expression_statements(then_expr, out);
            collect_ignored_expression_statements(else_expr, out);
        }
        ("match", match_args) if match_args.len() >= 3 && match_args.len() % 2 == 1 => {
            collect_ignored_expression_statements(&match_args[0], out);
            for pair in match_args[1..].chunks_exact(2) {
                collect_ignored_expression_statements(&pair[1], out);
            }
        }
        _ => {
            for arg in args {
                collect_ignored_expression_statements(&arg, out);
            }
        }
    }
}

fn collect_unused_bindings(
    expr: &str,
    line_map: &mut std::collections::BTreeMap<String, std::collections::VecDeque<usize>>,
    out: &mut Vec<SourceUnusedBinding>,
) {
    let Some((func, args)) = parse_source_call(expr) else {
        return;
    };

    match (func.as_str(), args.as_slice()) {
        ("let", [name, value, next]) if is_source_local_ident(name) => {
            let line_num = source_let_binding_line(name, line_map);
            if should_report_unused_binding(name)
                && !source_expr_references_binding(name, next)
                && let Some(line_num) = line_num
            {
                out.push(SourceUnusedBinding {
                    name: name.clone(),
                    line_num,
                });
            }
            collect_unused_bindings(value, line_map, out);
            collect_unused_bindings(next, line_map, out);
        }
        ("let_typed", [name, _ty, line, value, next]) if is_source_local_ident(name) => {
            if should_report_unused_binding(name)
                && !source_expr_references_binding(name, next)
                && let Ok(line_num) = parse_source_let_line_marker(line)
            {
                out.push(SourceUnusedBinding {
                    name: name.clone(),
                    line_num,
                });
            }
            collect_unused_bindings(value, line_map, out);
            collect_unused_bindings(next, line_map, out);
        }
        _ => {
            for arg in args {
                collect_unused_bindings(&arg, line_map, out);
            }
        }
    }
}

fn source_expr_references_binding(name: &str, expr: &str) -> bool {
    if expr.trim() == name {
        return true;
    }
    let Some((func, args)) = parse_source_call(expr) else {
        return false;
    };
    match (func.as_str(), args.as_slice()) {
        ("let", [binding, value, next]) | ("let_typed", [binding, _, _, value, next])
            if binding == name =>
        {
            source_expr_references_binding(name, value)
        }
        _ => args
            .iter()
            .any(|arg| source_expr_references_binding(name, arg)),
    }
}

fn should_report_unused_binding(name: &str) -> bool {
    !is_source_statement_binding_name(name) && !name.starts_with('_')
}

fn source_let_binding_line(
    name: &str,
    line_map: &mut std::collections::BTreeMap<String, std::collections::VecDeque<usize>>,
) -> Option<usize> {
    line_map.get_mut(name)?.pop_front()
}

fn source_let_binding_line_map(
    src: &str,
) -> std::collections::BTreeMap<String, std::collections::VecDeque<usize>> {
    let mut out = std::collections::BTreeMap::new();
    for (idx, line) in src.lines().enumerate() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("let ") else {
            continue;
        };
        let Some((binding, _value)) = rest.split_once('=') else {
            continue;
        };
        let name = binding
            .split_once(':')
            .map(|(name, _ty)| name)
            .unwrap_or(binding)
            .trim();
        if is_source_local_ident(name) {
            out.entry(name.to_string())
                .or_insert_with(std::collections::VecDeque::new)
                .push_back(idx + 1);
        }
    }
    out
}

fn source_statement_binding_line(name: &str) -> Option<usize> {
    name.strip_prefix(SOURCE_STATEMENT_BINDING_PREFIX)?
        .parse()
        .ok()
}

fn source_expr_has_direct_effect(expr: &str) -> bool {
    let Some((func, args)) = parse_source_call(expr) else {
        return false;
    };
    matches!(
        func.as_str(),
        "print" | "log.write" | "log_write" | "effect_call"
    ) || args.iter().any(|arg| source_expr_has_direct_effect(arg))
}
