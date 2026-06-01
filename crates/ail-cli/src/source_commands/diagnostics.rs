use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceIgnoredExpressionStatement {
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
