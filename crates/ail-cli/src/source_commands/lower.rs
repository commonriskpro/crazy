use super::model::*;
use super::parse::*;
use super::syntax::*;
use super::*;

mod collections;
mod control;
mod int;
mod match_expr;
mod option_result;
mod syntax_helpers;
mod text;

pub(super) use collections::*;
pub(super) use control::*;
pub(super) use int::*;
pub(super) use match_expr::*;
pub(super) use option_result::*;
pub(super) use syntax_helpers::*;
pub(super) use text::*;

#[derive(Clone, Copy)]
pub(super) enum SourceLowerDiagnostic {
    CollectionArity,
    FieldAccess,
    IndexExpression,
    ListLiteral,
    RecordLiteral,
    UnaryOperator,
}

impl SourceLowerDiagnostic {
    fn code(self) -> &'static str {
        match self {
            SourceLowerDiagnostic::CollectionArity => "AIL_SOURCE_LOWER_COLLECTION_ARITY",
            SourceLowerDiagnostic::FieldAccess => "AIL_SOURCE_LOWER_FIELD_ACCESS",
            SourceLowerDiagnostic::IndexExpression => "AIL_SOURCE_LOWER_INDEX_EXPRESSION",
            SourceLowerDiagnostic::ListLiteral => "AIL_SOURCE_LOWER_LIST_LITERAL",
            SourceLowerDiagnostic::RecordLiteral => "AIL_SOURCE_LOWER_RECORD_LITERAL",
            SourceLowerDiagnostic::UnaryOperator => "AIL_SOURCE_LOWER_UNARY_OPERATOR",
        }
    }

    fn category(self) -> &'static str {
        match self {
            SourceLowerDiagnostic::CollectionArity
            | SourceLowerDiagnostic::FieldAccess
            | SourceLowerDiagnostic::IndexExpression
            | SourceLowerDiagnostic::ListLiteral
            | SourceLowerDiagnostic::RecordLiteral => "source.lower.collection",
            SourceLowerDiagnostic::UnaryOperator => "source.lower.operator",
        }
    }
}

pub(super) fn source_lower_error(
    line_num: usize,
    diagnostic: SourceLowerDiagnostic,
    message: impl AsRef<str>,
) -> CliError {
    CliError::ParseError(format!(
        "line {line_num}: [{}] category={}: {}",
        diagnostic.code(),
        diagnostic.category(),
        message.as_ref()
    ))
}

pub(super) fn source_program_to_graph(
    program: &SourceProgram,
    change_name: impl Into<String>,
) -> Result<SemanticGraph, CliError> {
    let acl = source_program_to_acl(program, change_name.into());
    let parsed = parse_changeset(&acl)
        .map_err(|e| CliError::ParseError(format!("failed to lower AIL source to ACL: {e}")))?;
    let canonical = canonicalize_parsed(parsed);
    let mut graph = SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };
    let bridge = SimpleSnapshotBridge(SnapshotId(0));

    match ail_change::apply::apply(canonical, &mut graph, &bridge) {
        ChangeSetOutcome::Applied => Ok(graph),
        ChangeSetOutcome::RebaseRequired {
            current_snapshot_id,
        } => Err(CliError::RebaseRequired {
            current_snapshot_id: current_snapshot_id.0,
        }),
        ChangeSetOutcome::Failed { reason } => Err(CliError::Domain(format!(
            "AIL source graph materialization failed: {reason}"
        ))),
        ChangeSetOutcome::ConflictIrresolvable { reason } => Err(CliError::Domain(format!(
            "AIL source graph conflict: {reason:?}"
        ))),
    }
}

pub(super) fn source_program_to_acl(program: &SourceProgram, change_name: String) -> String {
    let constants = source_constant_names(program);
    let mut acl = format!(
        "change {change_name}\n\
author ail-source\n\
description AIL source file\n\
base 0\n"
    );

    for capability in &program.capabilities {
        acl.push_str(&format!("op create_capability id={capability}\n"));
    }
    for constant in &program.constants {
        acl.push_str(&format!(
            "op create_function id={} return={} body={}\n",
            constant.name,
            constant.return_type,
            source_expr_to_acl_body(&constant.body, &constants)
        ));
    }
    for function in &program.functions {
        acl.push_str(&format!(
            "op create_function id={} return={} body={}\n",
            function.name,
            function.return_type,
            source_expr_to_acl_body(&function.body, &constants)
        ));
        for param in &function.params {
            acl.push_str(&format!(
                "op add_param target={} name={} type={}\n",
                function.name, param.name, param.ty
            ));
        }
    }
    for test in &program.tests {
        acl.push_str(&format!(
            "op create_test id={} return={} body={}\n",
            test.name,
            test.return_type,
            source_expr_to_acl_body(&test.body, &constants)
        ));
    }
    for grant in &program.grants {
        acl.push_str(&format!(
            "op grant target={} capability={}\n",
            grant.target, grant.capability
        ));
    }
    acl.push_str("end\n");
    acl
}

pub(super) fn source_expr_to_acl_body(expr: &str, constants: &BTreeMap<String, String>) -> String {
    let expr = expr.trim();
    let Some((func, args)) = parse_source_call(expr) else {
        return source_const_reference_target(expr, constants)
            .map(|constant| format!("{constant}()"))
            .unwrap_or_else(|| expr.to_string());
    };
    if func == "let_typed" && args.len() == 5 && is_source_local_ident(&args[0]) {
        return format!(
            "let({}, {}, {})",
            args[0],
            source_expr_to_acl_body(&args[3], constants),
            source_expr_to_acl_body(&args[4], constants)
        );
    }
    if func == "record" && args.len().is_multiple_of(2) {
        let fields = args
            .chunks_exact(2)
            .map(|pair| {
                format!(
                    "{}, {}",
                    pair[0].trim(),
                    source_expr_to_acl_body(&pair[1], constants)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        return format!("record({fields})");
    }
    if func == "field" && args.len() == 2 {
        return format!(
            "field({}, {})",
            source_expr_to_acl_body(&args[0], constants),
            args[1].trim()
        );
    }
    if func == "update" && args.len() == 3 {
        return format!(
            "update({}, {}, {})",
            source_expr_to_acl_body(&args[0], constants),
            args[1].trim(),
            source_expr_to_acl_body(&args[2], constants)
        );
    }
    format!(
        "{}({})",
        func,
        args.iter()
            .map(|arg| source_expr_to_acl_body(arg, constants))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

pub(super) fn lower_source_expr(expr: &str, line_num: usize) -> Result<String, CliError> {
    let expr = expr.trim();
    if let Some(rest) = expr.strip_prefix("if ") {
        return lower_if_expr(rest, line_num);
    }
    if let Some(rest) = expr.strip_prefix("match ") {
        return lower_match_expr(rest, line_num);
    }
    if let Some(inner) = strip_wrapping_source_parens(expr) {
        return lower_source_expr(inner, line_num);
    }
    if let Some(lowered) = lower_source_constructor_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_unwrap_or_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_option_predicate_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_result_predicate_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_is_empty_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_int_bounds_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_text_eq_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_text_trim_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_text_contains_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_text_index_of_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_text_parse_int_or_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_text_byte_at_or_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_text_slice_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_text_replace_first_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_text_boundary_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_first_or_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_last_or_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(lowered) = lower_source_get_or_expr(expr, line_num)? {
        return Ok(lowered);
    }
    if let Some(literal) = parse_source_record_literal(expr, line_num)? {
        if let Some(base) = literal.base {
            let mut lowered = lower_source_expr(&base, line_num)?;
            for (field, value) in literal.fields {
                lowered = format!(
                    "update({lowered}, {field}, {})",
                    lower_source_expr(&value, line_num)?
                );
            }
            return Ok(lowered);
        }
        return Ok(format!(
            "record({})",
            literal
                .fields
                .iter()
                .map(|(field, value)| {
                    lower_source_expr(value, line_num)
                        .map(|lowered_value| format!("{field}, {lowered_value}"))
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ")
        ));
    }
    if let Some(items) = parse_source_list_literal(expr, line_num)? {
        return Ok(format!(
            "list({})",
            items
                .iter()
                .map(|item| lower_source_expr(item, line_num))
                .collect::<Result<Vec<_>, _>>()?
                .join(", ")
        ));
    }
    if let Some((collection, index)) = parse_source_index_expr(expr, line_num)? {
        return Ok(format!(
            "index({}, {})",
            lower_source_expr(collection, line_num)?,
            lower_source_expr(index, line_num)?
        ));
    }
    if let Some((record, field)) = parse_source_dot_field_expr(expr, line_num)? {
        return Ok(format!(
            "field({}, {field})",
            lower_source_expr(record, line_num)?
        ));
    }
    if let Some((left, right)) = split_top_level_source_binary_str(expr, "||") {
        return Ok(format!(
            "or({}, {})",
            lower_source_expr(left, line_num)?,
            lower_source_expr(right, line_num)?
        ));
    }
    if let Some((left, right)) = split_top_level_source_binary_str(expr, "&&") {
        return Ok(format!(
            "and({}, {})",
            lower_source_expr(left, line_num)?,
            lower_source_expr(right, line_num)?
        ));
    }
    if let Some((left, right)) = split_top_level_source_binary_str(expr, "==") {
        return Ok(format!(
            "eq({}, {})",
            lower_source_expr(left, line_num)?,
            lower_source_expr(right, line_num)?
        ));
    }
    if let Some((left, right)) = split_top_level_source_binary_str(expr, "!=") {
        return Ok(format!(
            "ne({}, {})",
            lower_source_expr(left, line_num)?,
            lower_source_expr(right, line_num)?
        ));
    }
    if let Some((left, right)) = split_top_level_source_binary_str(expr, ">=") {
        return Ok(format!(
            "ge({}, {})",
            lower_source_expr(left, line_num)?,
            lower_source_expr(right, line_num)?
        ));
    }
    if let Some((left, right)) = split_top_level_source_binary_str(expr, "<=") {
        return Ok(format!(
            "le({}, {})",
            lower_source_expr(left, line_num)?,
            lower_source_expr(right, line_num)?
        ));
    }
    if let Some((left, right)) = split_top_level_source_binary(expr, '>') {
        return Ok(format!(
            "gt({}, {})",
            lower_source_expr(left, line_num)?,
            lower_source_expr(right, line_num)?
        ));
    }
    if let Some((left, right)) = split_top_level_source_binary(expr, '<') {
        return Ok(format!(
            "lt({}, {})",
            lower_source_expr(left, line_num)?,
            lower_source_expr(right, line_num)?
        ));
    }
    if let Some((left, right)) = split_top_level_source_binary_str(expr, "++") {
        return Ok(format!(
            "concat({}, {})",
            lower_source_expr(left, line_num)?,
            lower_source_expr(right, line_num)?
        ));
    }
    if let Some((left, op, right)) = split_top_level_source_binary_any(expr, &['+', '-']) {
        let func = match op {
            '+' => "add",
            '-' => "sub",
            _ => unreachable!("unsupported additive operator"),
        };
        return Ok(format!(
            "{}({}, {})",
            func,
            lower_source_expr(left, line_num)?,
            lower_source_expr(right, line_num)?
        ));
    }
    if let Some((left, op, right)) = split_top_level_source_binary_any(expr, &['*', '/', '%']) {
        let func = match op {
            '*' => "mul",
            '/' => "div",
            '%' => "mod",
            _ => unreachable!("unsupported multiplicative operator"),
        };
        return Ok(format!(
            "{}({}, {})",
            func,
            lower_source_expr(left, line_num)?,
            lower_source_expr(right, line_num)?
        ));
    }
    if let Some(inner) = expr.strip_prefix('-') {
        if expr.parse::<i64>().is_ok() || is_source_float_literal(expr) {
            return Ok(expr.to_string());
        }
        let inner = inner.trim();
        if inner.is_empty() {
            return Err(source_lower_error(
                line_num,
                SourceLowerDiagnostic::UnaryOperator,
                "unary `-` requires an expression",
            ));
        }
        return Ok(format!("sub(0, {})", lower_source_expr(inner, line_num)?));
    }
    if let Some(inner) = expr.strip_prefix('!') {
        let inner = inner.trim();
        if inner.is_empty() || inner.starts_with('=') {
            return Err(source_lower_error(
                line_num,
                SourceLowerDiagnostic::UnaryOperator,
                "unary `!` requires an expression",
            ));
        }
        return Ok(format!("not({})", lower_source_expr(inner, line_num)?));
    }
    Ok(expr.to_string())
}
