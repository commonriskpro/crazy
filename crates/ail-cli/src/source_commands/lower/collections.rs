use super::*;

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
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::CollectionArity,
            "first_or requires `first_or(list, fallback)`",
        ));
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
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::CollectionArity,
            "last_or requires `last_or(list, fallback)`",
        ));
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
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::CollectionArity,
            "get_or requires `get_or(list, index, fallback)`",
        ));
    }
    let list = lower_source_expr(&args[0], line_num)?;
    let index = lower_source_expr(&args[1], line_num)?;
    Ok(Some(format!(
        "if(and(ge({index}, 0), lt({index}, len({list}))), index({list}, {index}), {})",
        lower_source_expr(&args[2], line_num)?
    )))
}

pub(super) fn lower_source_list_get_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if !matches!(func.as_str(), "list.get" | "list_get") {
        return Ok(None);
    }
    if args.len() != 2 {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::CollectionArity,
            "list_get requires `list_get(list, index)`",
        ));
    }
    let list = lower_source_expr(&args[0], line_num)?;
    let index = lower_source_expr(&args[1], line_num)?;
    Ok(Some(format!(
        "if(and(ge({index}, 0), lt({index}, len({list}))), some(index({list}, {index})), none())"
    )))
}

pub(super) fn lower_source_unwrap_or_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if !matches!(
        func.as_str(),
        "unwrap_or" | "option.unwrap_or" | "option_unwrap_or"
    ) {
        return Ok(None);
    }
    if args.len() != 2 {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::CollectionArity,
            "unwrap_or requires `unwrap_or(option, fallback)`",
        ));
    }
    Ok(Some(format!(
        "match({}, Some(__ail_unwrap), __ail_unwrap, None, {})",
        lower_source_expr(&args[0], line_num)?,
        lower_source_expr(&args[1], line_num)?
    )))
}

pub(super) fn lower_source_list_helper_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    let Some((lowered_func, arity, usage)) = (match func.as_str() {
        "list.push" | "list_push" => Some(("list.push", 2, "list_push(list, value)")),
        "list.concat" | "list_concat" => Some(("list.concat", 2, "list_concat(left, right)")),
        _ => None,
    }) else {
        return Ok(None);
    };
    if args.len() != arity {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::CollectionArity,
            format!("{func} requires `{usage}`"),
        ));
    }
    Ok(Some(format!(
        "{lowered_func}({})",
        args.iter()
            .map(|arg| lower_source_expr(arg, line_num))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ")
    )))
}

pub(super) fn lower_source_queue_helper_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    let Some((lowered_func, arity, usage)) = (match func.as_str() {
        "queue.push_back" | "queue_push_back" => {
            Some(("queue.push_back", 2, "queue_push_back(queue, value)"))
        }
        "queue.pop_front" | "queue_pop_front" => {
            Some(("queue.pop_front", 1, "queue_pop_front(queue)"))
        }
        "queue.peek_front" | "queue_peek_front" => {
            Some(("queue.peek_front", 1, "queue_peek_front(queue)"))
        }
        "queue.length" | "queue_length" => Some(("queue.length", 1, "queue_length(queue)")),
        "queue.is_empty" | "queue_is_empty" => Some(("queue.is_empty", 1, "queue_is_empty(queue)")),
        _ => None,
    }) else {
        return Ok(None);
    };
    if args.len() != arity {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::CollectionArity,
            format!("{func} requires `{usage}`"),
        ));
    }
    Ok(Some(format!(
        "{lowered_func}({})",
        args.iter()
            .map(|arg| lower_source_expr(arg, line_num))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ")
    )))
}

pub(super) fn lower_source_set_helper_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    let Some((lowered_func, arity, usage)) = (match func.as_str() {
        "set.contains" | "set_contains" => Some(("set.contains", 2, "set_contains(set, value)")),
        "set.length" | "set_length" => Some(("set.length", 1, "set_length(set)")),
        "set.insert" | "set_insert" => Some(("set.insert", 2, "set_insert(set, value)")),
        _ => None,
    }) else {
        return Ok(None);
    };
    if args.len() != arity {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::CollectionArity,
            format!("{func} requires `{usage}`"),
        ));
    }
    Ok(Some(format!(
        "{lowered_func}({})",
        args.iter()
            .map(|arg| lower_source_expr(arg, line_num))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ")
    )))
}

/// Lower the bare `set`, `map`, and `tuple` collection constructors.
///
/// The constructor name is preserved while each argument is lowered
/// recursively, so nested expressions such as `set(1, 2 + 3)` become
/// `set(1, add(2, 3))`. Re-lowering already-lowered output is a no-op because
/// the lowered argument forms (literals, `add(..)`, etc.) pass through
/// unchanged.
pub(super) fn lower_source_collection_constructor_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if !matches!(func.as_str(), "set" | "map" | "tuple") {
        return Ok(None);
    }
    Ok(Some(format!(
        "{func}({})",
        args.iter()
            .map(|arg| lower_source_expr(arg, line_num))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ")
    )))
}

pub(super) fn lower_source_map_helper_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    let Some((lowered_func, arity, usage)) = (match func.as_str() {
        "map.get" | "map_get" => Some(("map.get", 2, "map_get(map, key)")),
        "map.contains_key" | "map_contains_key" => {
            Some(("map.contains_key", 2, "map_contains_key(map, key)"))
        }
        "map.length" | "map_length" => Some(("map.length", 1, "map_length(map)")),
        "map.insert" | "map_insert" => Some(("map.insert", 3, "map_insert(map, key, value)")),
        _ => None,
    }) else {
        return Ok(None);
    };
    if args.len() != arity {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::CollectionArity,
            format!("{func} requires `{usage}`"),
        ));
    }
    Ok(Some(format!(
        "{lowered_func}({})",
        args.iter()
            .map(|arg| lower_source_expr(arg, line_num))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ")
    )))
}

pub(super) fn parse_source_list_literal(
    expr: &str,
    line_num: usize,
) -> Result<Option<Vec<String>>, CliError> {
    if !expr.starts_with('[') {
        return Ok(None);
    }
    if !expr.ends_with(']') {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::ListLiteral,
            "list literal has unclosed `[`",
        ));
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
    pub(super) base: Option<String>,
    pub(super) fields: Vec<(String, String)>,
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
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::RecordLiteral,
            "record literal has unclosed `{`",
        ));
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
    let mut seen_fields = BTreeSet::new();
    for (entry_idx, field_expr) in split_source_args(inner).into_iter().enumerate() {
        let entry = field_expr.trim();
        if let Some(spread) = entry.strip_prefix("...") {
            if entry_idx != 0 || base.is_some() {
                return Err(source_lower_error(
                    line_num,
                    SourceLowerDiagnostic::RecordLiteral,
                    "record update spread must appear first",
                ));
            }
            let spread = spread.trim();
            if spread.is_empty() {
                return Err(source_lower_error(
                    line_num,
                    SourceLowerDiagnostic::RecordLiteral,
                    "record update spread requires a base expression",
                ));
            }
            base = Some(spread.to_string());
            continue;
        }

        let colon = find_top_level_source_colon(entry).ok_or_else(|| {
            source_lower_error(
                line_num,
                SourceLowerDiagnostic::RecordLiteral,
                "record literal field requires `name: expression`",
            )
        })?;
        let field = entry[..colon].trim();
        let value = entry[colon + ':'.len_utf8()..].trim();
        if field.is_empty() || value.is_empty() {
            return Err(source_lower_error(
                line_num,
                SourceLowerDiagnostic::RecordLiteral,
                "record literal field and value must be non-empty",
            ));
        }
        validate_source_local_name(field, line_num)?;
        if !seen_fields.insert(field.to_string()) {
            return Err(source_lower_expr_error(
                line_num,
                SourceLowerDiagnostic::BindingShape,
                expr,
                "record_literal",
                format!("duplicate record field `{field}` would overwrite an earlier binding"),
            ));
        }
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
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::IndexExpression,
            "index expression requires `collection[index]`",
        ));
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
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::FieldAccess,
            "field access requires `record.field`",
        ));
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
