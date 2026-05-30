use super::model::*;
use super::parse::*;
use super::syntax::*;
use super::*;

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
            return Err(CliError::ParseError(format!(
                "line {line_num}: unary `-` requires an expression"
            )));
        }
        return Ok(format!("sub(0, {})", lower_source_expr(inner, line_num)?));
    }
    if let Some(inner) = expr.strip_prefix('!') {
        let inner = inner.trim();
        if inner.is_empty() || inner.starts_with('=') {
            return Err(CliError::ParseError(format!(
                "line {line_num}: unary `!` requires an expression"
            )));
        }
        return Ok(format!("not({})", lower_source_expr(inner, line_num)?));
    }
    Ok(expr.to_string())
}

pub(super) fn lower_source_constructor_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    if expr == "None" || expr == "None()" {
        return Ok(Some("none()".to_string()));
    }
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func == "None" {
        if args.is_empty() {
            return Ok(Some("none()".to_string()));
        }
        return Err(CliError::ParseError(format!(
            "line {line_num}: source constructor `None` requires no values"
        )));
    }
    let lowered_func = match func.as_str() {
        "Some" => "some",
        "Ok" => "ok",
        "Err" => "err",
        _ => return Ok(None),
    };
    if args.len() != 1 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: source constructor `{func}` requires exactly one value"
        )));
    }
    Ok(Some(format!(
        "{lowered_func}({})",
        lower_source_expr(&args[0], line_num)?
    )))
}

pub(super) fn lower_source_option_predicate_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    let (some_body, none_body) = match func.as_str() {
        "is_some" => ("true", "false"),
        "is_none" => ("false", "true"),
        _ => return Ok(None),
    };
    if args.len() != 1 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: {func} requires `{func}(option)`"
        )));
    }
    Ok(Some(format!(
        "match({}, Some(_), {some_body}, None, {none_body})",
        lower_source_expr(&args[0], line_num)?
    )))
}

pub(super) fn lower_source_result_predicate_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    let (ok_body, err_body) = match func.as_str() {
        "is_ok" => ("true", "false"),
        "is_err" => ("false", "true"),
        _ => return Ok(None),
    };
    if args.len() != 1 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: {func} requires `{func}(result)`"
        )));
    }
    Ok(Some(format!(
        "match({}, Ok(_), {ok_body}, Err(_), {err_body})",
        lower_source_expr(&args[0], line_num)?
    )))
}

pub(super) fn lower_source_is_empty_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "is_empty" {
        return Ok(None);
    }
    if args.len() != 1 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: is_empty requires `is_empty(value)`"
        )));
    }
    Ok(Some(format!(
        "eq(len({}), 0)",
        lower_source_expr(&args[0], line_num)?
    )))
}

pub(super) fn lower_source_int_bounds_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    let (lowered_func, expected) = match func.as_str() {
        "int_min" => ("int.min", "int_min(left, right)"),
        "int_max" => ("int.max", "int_max(left, right)"),
        "int_clamp" => ("int.clamp", "int_clamp(value, low, high)"),
        "int_add_or" => ("int.add_or", "int_add_or(left, right, fallback)"),
        "int_sub_or" => ("int.sub_or", "int_sub_or(left, right, fallback)"),
        "int_mul_or" => ("int.mul_or", "int_mul_or(left, right, fallback)"),
        "int_saturating_add" => ("int.saturating_add", "int_saturating_add(left, right)"),
        "int_saturating_sub" => ("int.saturating_sub", "int_saturating_sub(left, right)"),
        "int_saturating_mul" => ("int.saturating_mul", "int_saturating_mul(left, right)"),
        "int_wrapping_add" => ("int.wrapping_add", "int_wrapping_add(left, right)"),
        "int_wrapping_sub" => ("int.wrapping_sub", "int_wrapping_sub(left, right)"),
        "int_wrapping_mul" => ("int.wrapping_mul", "int_wrapping_mul(left, right)"),
        "int_wrapping_neg" => ("int.wrapping_neg", "int_wrapping_neg(value)"),
        "int_bit_and" => ("int.bit_and", "int_bit_and(left, right)"),
        "int_bit_or" => ("int.bit_or", "int_bit_or(left, right)"),
        "int_bit_xor" => ("int.bit_xor", "int_bit_xor(left, right)"),
        "int_bit_not" => ("int.bit_not", "int_bit_not(value)"),
        "int_shift_left" => ("int.shift_left", "int_shift_left(value, amount)"),
        "int_shift_right" => ("int.shift_right", "int_shift_right(value, amount)"),
        "int_shift_right_unsigned" => (
            "int.shift_right_unsigned",
            "int_shift_right_unsigned(value, amount)",
        ),
        "int_saturating_neg" => ("int.saturating_neg", "int_saturating_neg(value)"),
        "int_abs_or" => ("int.abs_or", "int_abs_or(value, fallback)"),
        "int_neg_or" => ("int.neg_or", "int_neg_or(value, fallback)"),
        "int_div_or" => ("int.div_or", "int_div_or(value, divisor, fallback)"),
        "int_rem_or" => ("int.rem_or", "int_rem_or(value, divisor, fallback)"),
        _ => return Ok(None),
    };
    let expected_len = if matches!(
        func.as_str(),
        "int_saturating_neg" | "int_wrapping_neg" | "int_bit_not"
    ) {
        1
    } else if matches!(
        func.as_str(),
        "int_clamp" | "int_add_or" | "int_sub_or" | "int_mul_or" | "int_div_or" | "int_rem_or"
    ) {
        3
    } else {
        2
    };
    if args.len() != expected_len {
        return Err(CliError::ParseError(format!(
            "line {line_num}: {func} requires `{expected}`"
        )));
    }
    Ok(Some(format!(
        "{lowered_func}({})",
        args.iter()
            .map(|arg| lower_source_expr(arg, line_num))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ")
    )))
}

pub(super) fn lower_source_text_eq_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "text_eq" {
        return Ok(None);
    }
    if args.len() != 2 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: text_eq requires `text_eq(left, right)`"
        )));
    }
    Ok(Some(format!(
        "text.eq({}, {})",
        lower_source_expr(&args[0], line_num)?,
        lower_source_expr(&args[1], line_num)?
    )))
}

pub(super) fn lower_source_text_trim_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "text_trim" {
        return Ok(None);
    }
    if args.len() != 1 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: text_trim requires `text_trim(value)`"
        )));
    }
    Ok(Some(format!(
        "text.trim({})",
        lower_source_expr(&args[0], line_num)?
    )))
}

pub(super) fn lower_source_text_contains_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "text_contains" {
        return Ok(None);
    }
    if args.len() != 2 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: text_contains requires `text_contains(haystack, needle)`"
        )));
    }
    Ok(Some(format!(
        "text.contains({}, {})",
        lower_source_expr(&args[0], line_num)?,
        lower_source_expr(&args[1], line_num)?
    )))
}

pub(super) fn lower_source_text_index_of_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "text_index_of" {
        return Ok(None);
    }
    if args.len() != 2 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: text_index_of requires `text_index_of(haystack, needle)`"
        )));
    }
    Ok(Some(format!(
        "text.index_of({}, {})",
        lower_source_expr(&args[0], line_num)?,
        lower_source_expr(&args[1], line_num)?
    )))
}

pub(super) fn lower_source_text_parse_int_or_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "text_parse_int_or" {
        return Ok(None);
    }
    if args.len() != 2 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: text_parse_int_or requires `text_parse_int_or(value, fallback)`"
        )));
    }
    Ok(Some(format!(
        "text.parse_int_or({}, {})",
        lower_source_expr(&args[0], line_num)?,
        lower_source_expr(&args[1], line_num)?
    )))
}

pub(super) fn lower_source_text_byte_at_or_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "text_byte_at_or" {
        return Ok(None);
    }
    if args.len() != 3 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: text_byte_at_or requires `text_byte_at_or(value, index, fallback)`"
        )));
    }
    Ok(Some(format!(
        "text.byte_at_or({}, {}, {})",
        lower_source_expr(&args[0], line_num)?,
        lower_source_expr(&args[1], line_num)?,
        lower_source_expr(&args[2], line_num)?
    )))
}

pub(super) fn lower_source_text_slice_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "text_slice" {
        return Ok(None);
    }
    if args.len() != 3 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: text_slice requires `text_slice(value, start, length)`"
        )));
    }
    Ok(Some(format!(
        "text.slice({}, {}, {})",
        lower_source_expr(&args[0], line_num)?,
        lower_source_expr(&args[1], line_num)?,
        lower_source_expr(&args[2], line_num)?
    )))
}

pub(super) fn lower_source_text_replace_first_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "text_replace_first" {
        return Ok(None);
    }
    if args.len() != 3 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: text_replace_first requires `text_replace_first(value, needle, replacement)`"
        )));
    }
    Ok(Some(format!(
        "text.replace_first({}, {}, {})",
        lower_source_expr(&args[0], line_num)?,
        lower_source_expr(&args[1], line_num)?,
        lower_source_expr(&args[2], line_num)?
    )))
}

pub(super) fn lower_source_text_boundary_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    let (lowered_func, expected) = match func.as_str() {
        "text_starts_with" => ("text.starts_with", "text_starts_with(haystack, prefix)"),
        "text_ends_with" => ("text.ends_with", "text_ends_with(haystack, suffix)"),
        _ => return Ok(None),
    };
    if args.len() != 2 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: {func} requires `{expected}`"
        )));
    }
    Ok(Some(format!(
        "{lowered_func}({}, {})",
        lower_source_expr(&args[0], line_num)?,
        lower_source_expr(&args[1], line_num)?
    )))
}

pub(super) fn lower_source_first_or_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "first_or" {
        return Ok(None);
    }
    if args.len() != 2 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: first_or requires `first_or(list, fallback)`"
        )));
    }
    let list = lower_source_expr(&args[0], line_num)?;
    Ok(Some(format!(
        "if(gt(len({list}), 0), index({list}, 0), {})",
        lower_source_expr(&args[1], line_num)?
    )))
}

pub(super) fn lower_source_last_or_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "last_or" {
        return Ok(None);
    }
    if args.len() != 2 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: last_or requires `last_or(list, fallback)`"
        )));
    }
    let list = lower_source_expr(&args[0], line_num)?;
    Ok(Some(format!(
        "if(gt(len({list}), 0), index({list}, sub(len({list}), 1)), {})",
        lower_source_expr(&args[1], line_num)?
    )))
}

pub(super) fn lower_source_get_or_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "get_or" {
        return Ok(None);
    }
    if args.len() != 3 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: get_or requires `get_or(list, index, fallback)`"
        )));
    }
    let list = lower_source_expr(&args[0], line_num)?;
    let index = lower_source_expr(&args[1], line_num)?;
    Ok(Some(format!(
        "if(and(ge({index}, 0), lt({index}, len({list}))), index({list}, {index}), {})",
        lower_source_expr(&args[2], line_num)?
    )))
}

pub(super) fn lower_source_unwrap_or_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if func != "unwrap_or" {
        return Ok(None);
    }
    if args.len() != 2 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: unwrap_or requires `unwrap_or(value, fallback)`"
        )));
    }
    Ok(Some(format!(
        "match({}, Some(__ail_unwrap), __ail_unwrap, None, {})",
        lower_source_expr(&args[0], line_num)?,
        lower_source_expr(&args[1], line_num)?
    )))
}

pub(super) fn strip_wrapping_source_parens(expr: &str) -> Option<&str> {
    if !expr.starts_with('(') || !expr.ends_with(')') {
        return None;
    }
    (matching_paren(expr, 0)? == expr.len() - 1).then(|| expr[1..expr.len() - 1].trim())
}

pub(super) fn split_top_level_source_binary_str<'a>(
    expr: &'a str,
    op: &str,
) -> Option<(&'a str, &'a str)> {
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;
    let mut split_at = None;

    for (idx, ch) in expr.char_indices() {
        if in_string {
            if ch == '"' && !prev_was_escape {
                in_string = false;
            }
            prev_was_escape = ch == '\\' && !prev_was_escape;
            continue;
        }

        match ch {
            '"' => in_string = true,
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            _ if paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && idx > 0
                && expr[idx..].starts_with(op) =>
            {
                split_at = Some(idx);
            }
            _ => {}
        }
        prev_was_escape = false;
    }

    let idx = split_at?;
    let left = expr[..idx].trim();
    let right = expr[idx + op.len()..].trim();
    (!left.is_empty() && !right.is_empty()).then_some((left, right))
}

pub(super) fn split_top_level_source_binary_any<'a>(
    expr: &'a str,
    ops: &[char],
) -> Option<(&'a str, char, &'a str)> {
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;
    let mut split_at = None;

    for (idx, ch) in expr.char_indices() {
        if in_string {
            if ch == '"' && !prev_was_escape {
                in_string = false;
            }
            prev_was_escape = ch == '\\' && !prev_was_escape;
            continue;
        }

        match ch {
            '"' => in_string = true,
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            _ if ops.contains(&ch)
                && paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && source_binary_char_has_left_operand(expr, idx) =>
            {
                split_at = Some((idx, ch));
            }
            _ => {}
        }
        prev_was_escape = false;
    }

    let (idx, op) = split_at?;
    let left = expr[..idx].trim();
    let right = expr[idx + op.len_utf8()..].trim();
    (!left.is_empty() && !right.is_empty()).then_some((left, op, right))
}

pub(super) fn source_binary_char_has_left_operand(expr: &str, idx: usize) -> bool {
    expr[..idx]
        .chars()
        .rev()
        .find(|ch| !ch.is_whitespace())
        .is_some_and(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | ')' | '}' | ']' | '"'))
}

pub(super) fn split_top_level_source_binary(expr: &str, op: char) -> Option<(&str, &str)> {
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;
    let mut split_at = None;

    for (idx, ch) in expr.char_indices() {
        if in_string {
            if ch == '"' && !prev_was_escape {
                in_string = false;
            }
            prev_was_escape = ch == '\\' && !prev_was_escape;
            continue;
        }

        match ch {
            '"' => in_string = true,
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            _ if ch == op
                && paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && idx > 0 =>
            {
                split_at = Some(idx);
            }
            _ => {}
        }
        prev_was_escape = false;
    }

    let idx = split_at?;
    let left = expr[..idx].trim();
    let right = expr[idx + op.len_utf8()..].trim();
    (!left.is_empty() && !right.is_empty()).then_some((left, right))
}

pub(super) fn parse_source_list_literal(
    expr: &str,
    line_num: usize,
) -> Result<Option<Vec<String>>, CliError> {
    if !expr.starts_with('[') {
        return Ok(None);
    }
    if !expr.ends_with(']') {
        return Err(CliError::ParseError(format!(
            "line {line_num}: list literal has unclosed `[`"
        )));
    }
    if matching_bracket(expr, 0) != Some(expr.len() - 1) {
        return Ok(None);
    }
    let inner = expr[1..expr.len() - 1].trim();
    if inner.is_empty() {
        return Ok(Some(vec![]));
    }
    Ok(Some(split_source_args(inner)))
}

pub(super) struct SourceRecordLiteral {
    base: Option<String>,
    fields: Vec<(String, String)>,
}

pub(super) fn parse_source_record_literal(
    expr: &str,
    line_num: usize,
) -> Result<Option<SourceRecordLiteral>, CliError> {
    if !expr.starts_with('{') {
        return Ok(None);
    }
    let close = matching_brace(expr, 0);
    if close.is_none() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: record literal has unclosed `{{`"
        )));
    }
    if close != Some(expr.len() - 1) {
        return Ok(None);
    }
    let inner = expr[1..expr.len() - 1].trim();
    if inner.is_empty() {
        return Ok(Some(SourceRecordLiteral {
            base: None,
            fields: vec![],
        }));
    }

    let mut base = None;
    let mut fields = Vec::new();
    for (entry_idx, field_expr) in split_source_args(inner).into_iter().enumerate() {
        let entry = field_expr.trim();
        if let Some(spread) = entry.strip_prefix("...") {
            if entry_idx != 0 || base.is_some() {
                return Err(CliError::ParseError(format!(
                    "line {line_num}: record update spread must appear first"
                )));
            }
            let spread = spread.trim();
            if spread.is_empty() {
                return Err(CliError::ParseError(format!(
                    "line {line_num}: record update spread requires a base expression"
                )));
            }
            base = Some(spread.to_string());
            continue;
        }

        let colon = find_top_level_source_colon(entry).ok_or_else(|| {
            CliError::ParseError(format!(
                "line {line_num}: record literal field requires `name: expression`"
            ))
        })?;
        let field = entry[..colon].trim();
        let value = entry[colon + ':'.len_utf8()..].trim();
        if field.is_empty() || value.is_empty() {
            return Err(CliError::ParseError(format!(
                "line {line_num}: record literal field and value must be non-empty"
            )));
        }
        validate_source_local_name(field, line_num)?;
        fields.push((field.to_string(), value.to_string()));
    }

    Ok(Some(SourceRecordLiteral { base, fields }))
}

pub(super) fn find_top_level_source_colon(expr: &str) -> Option<usize> {
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut angle_depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;

    for (idx, ch) in expr.char_indices() {
        if in_string {
            if ch == '"' && !prev_was_escape {
                in_string = false;
            }
            prev_was_escape = ch == '\\' && !prev_was_escape;
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            ':' if paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && angle_depth == 0 =>
            {
                return Some(idx);
            }
            _ => {}
        }
        prev_was_escape = false;
    }

    None
}

pub(super) fn parse_source_index_expr<'a>(
    expr: &'a str,
    line_num: usize,
) -> Result<Option<(&'a str, &'a str)>, CliError> {
    if !expr.ends_with(']') || expr.starts_with('[') {
        return Ok(None);
    }
    let open = find_top_level_source_index_bracket(expr);
    let Some(open) = open else {
        return Ok(None);
    };
    if matching_bracket(expr, open) != Some(expr.len() - 1) {
        return Ok(None);
    }
    let collection = expr[..open].trim();
    let index = expr[open + 1..expr.len() - 1].trim();
    if collection.is_empty() || index.is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: index expression requires `collection[index]`"
        )));
    }
    Ok(Some((collection, index)))
}

pub(super) fn parse_source_dot_field_expr<'a>(
    expr: &'a str,
    line_num: usize,
) -> Result<Option<(&'a str, &'a str)>, CliError> {
    let Some(dot) = find_top_level_source_dot(expr) else {
        return Ok(None);
    };
    let record = expr[..dot].trim();
    let field = expr[dot + 1..].trim();
    if record.is_empty() || field.is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: field access requires `record.field`"
        )));
    }
    if !is_source_local_ident(field) {
        return Ok(None);
    }
    Ok(Some((record, field)))
}

pub(super) fn find_top_level_source_dot(expr: &str) -> Option<usize> {
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;
    let mut candidate = None;

    for (idx, ch) in expr.char_indices() {
        if in_string {
            if ch == '"' && !prev_was_escape {
                in_string = false;
            }
            prev_was_escape = ch == '\\' && !prev_was_escape;
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '.' if paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && source_dot_has_record_operand(expr, idx) =>
            {
                candidate = Some(idx);
            }
            _ => {}
        }
        prev_was_escape = false;
    }

    candidate
}

pub(super) fn source_dot_has_record_operand(expr: &str, idx: usize) -> bool {
    expr[..idx]
        .chars()
        .rev()
        .find(|ch| !ch.is_whitespace())
        .is_some_and(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | ')' | '}' | ']' | '"'))
}

pub(super) fn find_top_level_source_index_bracket(expr: &str) -> Option<usize> {
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;
    let mut candidate = None;

    for (idx, ch) in expr.char_indices() {
        if in_string {
            if ch == '"' && !prev_was_escape {
                in_string = false;
            }
            prev_was_escape = ch == '\\' && !prev_was_escape;
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                candidate = Some(idx);
                bracket_depth += 1;
            }
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            _ => {}
        }
        prev_was_escape = false;
    }

    candidate
}

pub(super) fn lower_match_expr(rest: &str, line_num: usize) -> Result<String, CliError> {
    let open = rest.find('{').ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: match expression requires `match value {{ Pattern => expr, ... }}`"
        ))
    })?;
    let scrutinee = rest[..open].trim();
    if scrutinee.is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: match expression requires a scrutinee"
        )));
    }
    let close = matching_brace(rest, open).ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: match expression has unclosed arm block"
        ))
    })?;
    if !rest[close + 1..].trim().is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: unexpected tokens after match expression"
        )));
    }
    let arms = rest[open + 1..close].trim();
    if arms.is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: match expression requires at least one arm"
        )));
    }

    let mut lowered = vec![lower_source_expr(scrutinee, line_num)?];
    for arm in split_source_match_arms(arms) {
        let (pattern, body) = split_source_match_arm(arm, line_num)?;
        let pattern = normalize_source_match_pattern(pattern, line_num)?;
        lowered.push(pattern);
        lowered.push(lower_source_expr(body, line_num)?);
    }
    Ok(format!("match({})", lowered.join(", ")))
}

pub(super) fn split_source_match_arms(arms: &str) -> Vec<String> {
    split_source_args(arms)
}

pub(super) fn split_source_match_arm<'a>(
    arm: &'a str,
    line_num: usize,
) -> Result<(&'a str, &'a str), CliError> {
    let Some(idx) = find_top_level_source_arrow(arm) else {
        return Err(CliError::ParseError(format!(
            "line {line_num}: match arm requires `Pattern => expression`"
        )));
    };
    let pattern = arm[..idx].trim();
    let body = arm[idx + 2..].trim();
    if pattern.is_empty() || body.is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: match arm pattern and body must be non-empty"
        )));
    }
    Ok((pattern, body))
}

pub(super) fn find_top_level_source_arrow(input: &str) -> Option<usize> {
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;

    for (idx, ch) in input.char_indices() {
        if in_string {
            if ch == '"' && !prev_was_escape {
                in_string = false;
            }
            prev_was_escape = ch == '\\' && !prev_was_escape;
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '=' if paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && input[idx..].starts_with("=>") =>
            {
                return Some(idx);
            }
            _ => {}
        }
        prev_was_escape = false;
    }
    None
}

pub(super) fn lower_if_expr(rest: &str, line_num: usize) -> Result<String, CliError> {
    let open_then = rest.find('{').ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: if expression requires `{{ then }} else {{ else }}`"
        ))
    })?;
    let cond = rest[..open_then].trim();
    if cond.is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: if expression requires a condition"
        )));
    }

    let then_close = matching_brace(rest, open_then).ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: if expression has unclosed then block"
        ))
    })?;
    let then_expr = rest[open_then + 1..then_close].trim();
    let after_then = rest[then_close + 1..].trim_start();
    let after_else = after_then.strip_prefix("else").ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: if expression requires an else branch"
        ))
    })?;
    let after_else = after_else.trim_start();
    if !after_else.starts_with('{') {
        return Err(CliError::ParseError(format!(
            "line {line_num}: else branch requires `{{ expression }}`"
        )));
    }
    let else_close = matching_brace(after_else, 0).ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: if expression has unclosed else block"
        ))
    })?;
    if !after_else[else_close + 1..].trim().is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: unexpected tokens after if expression"
        )));
    }
    let else_expr = after_else[1..else_close].trim();

    if then_expr.is_empty() || else_expr.is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: if branches must be non-empty expressions"
        )));
    }

    Ok(format!(
        "if({}, {}, {})",
        lower_source_expr(cond, line_num)?,
        lower_source_expr(then_expr, line_num)?,
        lower_source_expr(else_expr, line_num)?
    ))
}

pub(super) fn matching_brace(s: &str, open_idx: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;

    for (idx, ch) in s.char_indices().skip_while(|(idx, _)| *idx < open_idx) {
        if in_string {
            if ch == '"' && !prev_was_escape {
                in_string = false;
            }
            prev_was_escape = ch == '\\' && !prev_was_escape;
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
        prev_was_escape = false;
    }
    None
}
