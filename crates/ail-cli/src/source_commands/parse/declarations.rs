use super::*;

pub(super) fn parse_source_module(
    rest: &str,
    line_num: usize,
    base_column: usize,
) -> Result<String, CliError> {
    let module = rest.trim();
    validate_source_name_at(module, line_num, trimmed_fragment_column(base_column, rest))?;
    Ok(module.to_string())
}

pub(super) fn parse_source_import(
    rest: &str,
    line_num: usize,
    base_column: usize,
) -> Result<String, CliError> {
    let raw_import_column = trimmed_fragment_column(base_column, rest);
    let raw_import = rest.trim();
    let Some(import) = raw_import
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
    else {
        return Err(source_parse_error_for_fragment_at(
            line_num,
            raw_import_column,
            SourceParseDiagnostic::InvalidDeclaration,
            raw_import,
            "import declaration requires `use \"relative/path.ail\"`",
        ));
    };
    let import_column = raw_import_column + 1;
    if import.is_empty() || import.contains('\0') || Path::new(import).is_absolute() {
        return Err(source_parse_error_for_fragment_at(
            line_num,
            import_column,
            SourceParseDiagnostic::InvalidDeclaration,
            import,
            "import path must be a non-empty relative path",
        ));
    }
    if import.contains('\\') {
        return Err(source_parse_error_for_fragment_at(
            line_num,
            import_column,
            SourceParseDiagnostic::InvalidDeclaration,
            import,
            format!("import path `{import}` must use `/` separators"),
        ));
    }
    if import.contains(':') {
        return Err(source_parse_error_for_fragment_at(
            line_num,
            import_column,
            SourceParseDiagnostic::InvalidDeclaration,
            import,
            format!("import path `{import}` must not contain `:`"),
        ));
    }
    if import.contains("//") {
        return Err(source_parse_error_for_fragment_at(
            line_num,
            import_column,
            SourceParseDiagnostic::InvalidDeclaration,
            import,
            format!("import path `{import}` must not contain empty path segments"),
        ));
    }
    if import.chars().any(char::is_whitespace) {
        return Err(source_parse_error_for_fragment_at(
            line_num,
            import_column,
            SourceParseDiagnostic::InvalidDeclaration,
            import,
            format!("import path `{import}` must not contain whitespace"),
        ));
    }
    if Path::new(import)
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(source_parse_error_for_fragment_at(
            line_num,
            import_column,
            SourceParseDiagnostic::InvalidDeclaration,
            import,
            format!("import path `{import}` must not contain `..`"),
        ));
    }
    if !import.starts_with("./") {
        return Err(source_parse_error_for_fragment_at(
            line_num,
            import_column,
            SourceParseDiagnostic::InvalidDeclaration,
            import,
            format!("local import path `{import}` must start with `./`"),
        ));
    }
    if Path::new(import).extension().and_then(|ext| ext.to_str()) != Some("ail") {
        return Err(source_parse_error_for_fragment_at(
            line_num,
            import_column,
            SourceParseDiagnostic::InvalidDeclaration,
            import,
            format!("import path `{import}` must end with `.ail`"),
        ));
    }
    Ok(import.to_string())
}

pub(super) fn parse_source_capability(
    rest: &str,
    line_num: usize,
    base_column: usize,
) -> Result<String, CliError> {
    let capability = rest.trim();
    validate_source_name_at(
        capability,
        line_num,
        trimmed_fragment_column(base_column, rest),
    )?;
    Ok(capability.to_string())
}

pub(super) fn parse_source_const(
    rest: &str,
    line_num: usize,
    base_column: usize,
) -> Result<SourceConst, CliError> {
    let (head, body) = rest.split_once('=').ok_or_else(|| {
        source_parse_error_for_fragment(
            line_num,
            SourceParseDiagnostic::InvalidDeclaration,
            rest,
            "const declaration requires `= body`",
        )
    })?;
    let (name, return_type) = head.split_once(':').ok_or_else(|| {
        source_parse_error_for_fragment(
            line_num,
            SourceParseDiagnostic::InvalidDeclaration,
            head,
            "const declaration requires `name: Type`",
        )
    })?;
    let name_column = trimmed_fragment_column(base_column, name);
    let return_type_column = source_fragment_column(base_column, head, return_type);
    let name = name.trim();
    let return_type = return_type.trim();
    let body = body.trim();
    validate_source_name_at(name, line_num, name_column)?;
    validate_source_type_name_at(return_type, line_num, return_type_column)?;
    let return_type = normalize_source_type_name(return_type);
    if body.is_empty() {
        return Err(source_parse_error_for_fragment(
            line_num,
            SourceParseDiagnostic::InvalidDeclaration,
            rest,
            "const body must be non-empty",
        ));
    }
    Ok(SourceConst {
        name: normalize_function_name(name),
        return_type,
        body: lower_source_expr(body, line_num)?,
        source_body: Some(lower_source_expr_for_format(body, line_num)?),
        line_num,
        source_path: None,
    })
}

pub(super) fn parse_source_function(
    rest: &str,
    line_num: usize,
    base_column: usize,
) -> Result<SourceFunction, CliError> {
    let (name, params, return_and_body, return_and_body_column) =
        parse_source_function_signature(rest, line_num, base_column)?;
    let (return_type, body) = return_and_body.split_once('=').ok_or_else(|| {
        source_parse_error_for_fragment(
            line_num,
            SourceParseDiagnostic::InvalidDeclaration,
            &return_and_body,
            "function declaration requires `= body`",
        )
    })?;
    let return_type_column = trimmed_fragment_column(return_and_body_column, return_type);

    build_source_function(
        name,
        params,
        return_type.trim(),
        return_type_column,
        body.trim(),
        line_num,
    )
}

pub(super) fn parse_source_function_with_body(
    rest: &str,
    line_num: usize,
    base_column: usize,
    body: String,
) -> Result<SourceFunction, CliError> {
    let (name, params, return_type, return_type_column) =
        parse_source_function_signature(rest, line_num, base_column)?;
    build_source_function_lowered(
        name,
        params,
        return_type.trim(),
        trimmed_fragment_column(return_type_column, &return_type),
        body.trim().to_string(),
        None,
        line_num,
    )
}

pub(super) fn parse_source_function_signature(
    rest: &str,
    line_num: usize,
    base_column: usize,
) -> Result<(String, Vec<SourceParam>, String, usize), CliError> {
    let open_paren = rest.find('(').ok_or_else(|| {
        source_parse_error_for_fragment(
            line_num,
            SourceParseDiagnostic::MissingDelimiter,
            rest,
            "function declaration requires `()`",
        )
    })?;
    let raw_name_text = &rest[..open_paren];
    let raw_name = raw_name_text.trim();
    let raw_name_column = trimmed_fragment_column(base_column, raw_name_text);
    validate_source_name_at(raw_name, line_num, raw_name_column)?;

    let params_start = open_paren + 1;
    let close_paren = rest[params_start..].find(')').ok_or_else(|| {
        source_parse_error_for_fragment(
            line_num,
            SourceParseDiagnostic::MissingDelimiter,
            rest,
            "function declaration requires closing `)`",
        )
    })? + params_start;
    let params = parse_source_params(
        &rest[params_start..close_paren],
        line_num,
        base_column + rest[..params_start].chars().count(),
    )?;
    let after_params = &rest[close_paren + 1..];
    let after_params_column = base_column + rest[..close_paren + 1].chars().count();
    let arrow_leading = after_params
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .count();
    let after_arrow = after_params
        .trim_start()
        .strip_prefix("->")
        .ok_or_else(|| {
            source_parse_error_for_fragment(
                line_num,
                SourceParseDiagnostic::InvalidDeclaration,
                after_params,
                "function declaration requires `-> Type`",
            )
        })?;
    let after_arrow_column = after_params_column + arrow_leading + 2;
    Ok((
        normalize_function_name(raw_name),
        params,
        after_arrow.to_string(),
        after_arrow_column,
    ))
}

fn trimmed_fragment_column(base_column: usize, text: &str) -> usize {
    base_column + text.chars().take_while(|ch| ch.is_whitespace()).count()
}

pub(super) fn build_source_function(
    name: String,
    params: Vec<SourceParam>,
    return_type: &str,
    return_type_column: usize,
    body: &str,
    line_num: usize,
) -> Result<SourceFunction, CliError> {
    if return_type.is_empty() || body.is_empty() {
        return Err(source_parse_error_for_fragment(
            line_num,
            SourceParseDiagnostic::InvalidDeclaration,
            body,
            "function return type and body must be non-empty",
        ));
    }
    let return_expr = source_optional_return_expr(body);
    let lowered_body = lower_source_expr(return_expr, line_num)?;
    let source_body = Some(lower_source_expr_for_format(return_expr, line_num)?);
    build_source_function_lowered(
        name,
        params,
        return_type,
        return_type_column,
        lowered_body,
        source_body,
        line_num,
    )
}

/// Build a function whose body has already been lowered (e.g. block bodies produced
/// by `source_block_to_expr`). Re-lowering an already-lowered body is not idempotent
/// for forms like match patterns, so the lowered body is stored verbatim here.
///
/// `source_body` carries the alias-preserving rendering form for inline declarations;
/// block bodies pass `None` and fall back to `body` during formatting.
pub(super) fn build_source_function_lowered(
    name: String,
    params: Vec<SourceParam>,
    return_type: &str,
    return_type_column: usize,
    lowered_body: String,
    source_body: Option<String>,
    line_num: usize,
) -> Result<SourceFunction, CliError> {
    if return_type.is_empty() || lowered_body.is_empty() {
        return Err(source_parse_error_for_fragment(
            line_num,
            SourceParseDiagnostic::InvalidDeclaration,
            &lowered_body,
            "function return type and body must be non-empty",
        ));
    }
    validate_source_type_name_at(return_type, line_num, return_type_column)?;
    let return_type = normalize_source_type_name(return_type);

    Ok(SourceFunction {
        name,
        params,
        return_type,
        body: lowered_body,
        source_body,
        line_num,
        source_path: None,
    })
}

pub(super) fn parse_source_params(
    params: &str,
    line_num: usize,
    base_column: usize,
) -> Result<Vec<SourceParam>, CliError> {
    let params_column = trimmed_fragment_column(base_column, params);
    let params = params.trim();
    if params.is_empty() {
        return Ok(vec![]);
    }

    let mut seen = BTreeSet::new();
    split_source_param_list(params)
        .into_iter()
        .map(|raw| {
            let param = raw.trim();
            let param_column = source_fragment_column(params_column, params, param);
            let (name, ty) = param.split_once(':').ok_or_else(|| {
                source_parse_error_for_fragment_at(
                    line_num,
                    param_column,
                    SourceParseDiagnostic::InvalidDeclaration,
                    param,
                    "function parameters must use `name: Type`",
                )
            })?;
            let name_column = trimmed_fragment_column(param_column, name);
            let ty_column = trimmed_fragment_column(param_column + name.chars().count() + 1, ty);
            let name = name.trim();
            let ty = ty.trim();
            validate_source_user_local_name_at(name, line_num, name_column)?;
            if !seen.insert(name.to_string()) {
                return Err(source_parse_error_for_fragment_at(
                    line_num,
                    name_column,
                    SourceParseDiagnostic::InvalidDeclaration,
                    name,
                    format!("duplicate parameter `{name}`"),
                ));
            }
            if ty.is_empty() {
                return Err(source_parse_error_for_fragment_at(
                    line_num,
                    ty_column,
                    SourceParseDiagnostic::InvalidType,
                    ty,
                    format!("parameter `{name}` requires a type"),
                ));
            }
            validate_source_type_name_at(ty, line_num, ty_column)?;
            let ty = normalize_source_type_name(ty);
            Ok(SourceParam {
                name: name.to_string(),
                ty,
            })
        })
        .collect()
}

fn source_fragment_column(base_column: usize, source: &str, fragment: &str) -> usize {
    let source_start = source.as_ptr() as usize;
    let source_end = source_start + source.len();
    let fragment_start = fragment.as_ptr() as usize;
    if fragment_start < source_start || fragment_start > source_end {
        return base_column;
    }
    let byte_offset = fragment_start - source_start;
    source
        .get(..byte_offset)
        .map(|prefix| base_column + prefix.chars().count())
        .unwrap_or(base_column)
}

pub(super) fn collect_braced_body(
    statements: &[(usize, usize, String)],
    mut idx: usize,
    opener_line: usize,
) -> Result<(Vec<(usize, usize, String)>, usize), CliError> {
    let mut body = Vec::new();
    let mut pending_nested: Option<(usize, usize, String, isize)> = None;
    while idx < statements.len() {
        let (line_num, column, statement) = &statements[idx];
        if let Some((start_line, start_column, combined, depth)) = pending_nested.as_mut() {
            append_source_block_fragment(combined, statement);
            *depth += source_brace_delta(statement);
            if *depth <= 0 {
                body.push((*start_line, *start_column, combined.clone()));
                pending_nested = None;
            }
            idx += 1;
            continue;
        }

        if statement == "}" {
            return Ok((body, idx + 1));
        }
        let brace_delta = source_brace_delta(statement);
        if brace_delta > 0 {
            pending_nested = Some((*line_num, *column, statement.clone(), brace_delta));
            idx += 1;
            continue;
        }
        body.push((*line_num, *column, statement.clone()));
        idx += 1;
    }

    if let Some((line_num, _column, statement, _depth)) = pending_nested {
        return Err(source_parse_error_for_fragment(
            line_num,
            SourceParseDiagnostic::MissingDelimiter,
            &statement,
            "nested source block requires closing `}`",
        ));
    }

    Err(source_parse_error(
        opener_line,
        SourceParseDiagnostic::MissingDelimiter,
        "function block requires closing `}`",
    ))
}

fn append_source_block_fragment(combined: &mut String, fragment: &str) {
    if !combined.is_empty() {
        combined.push('\n');
    }
    combined.push_str(fragment.trim());
}

/// Detect whether a `fn`/`test` declaration line uses an inline expression body
/// (`... = expression`) rather than a braced block body (`... { ... }`).
///
/// The distinction matters when an inline expression body (a `match`/`if`) opens a
/// brace that spans multiple source lines: those lines belong to the expression, not
/// to a statement block, and must be collected back into a single declaration.
pub(super) fn source_decl_has_inline_body(rest: &str) -> bool {
    source_top_level_assignment_index(rest).is_some()
}

fn source_top_level_assignment_index(input: &str) -> Option<usize> {
    let mut paren_depth = 0isize;
    let mut brace_depth = 0isize;
    let mut bracket_depth = 0isize;
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
            '=' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                let next = input[idx + ch.len_utf8()..].chars().next();
                if matches!(next, Some('=') | Some('>')) {
                    prev_was_escape = false;
                    continue;
                }
                let prev = input[..idx].chars().next_back();
                if matches!(prev, Some('<') | Some('>') | Some('!') | Some('=')) {
                    prev_was_escape = false;
                    continue;
                }
                return Some(idx);
            }
            _ => {}
        }
        prev_was_escape = false;
    }
    None
}

/// Collect the continuation lines of an inline declaration whose expression body
/// opens unbalanced braces (a multi-line `match`/`if`), joining them back into a
/// single logical declaration string the regular parser can consume.
pub(super) fn collect_source_inline_declaration(
    statements: &[(usize, usize, String)],
    head: &str,
    mut idx: usize,
    opener_line: usize,
) -> Result<(String, usize), CliError> {
    let mut combined = head.to_string();
    let mut depth = source_brace_delta(head);
    while depth > 0 && idx < statements.len() {
        let statement = &statements[idx].2;
        combined.push('\n');
        combined.push_str(statement);
        depth += source_brace_delta(statement);
        idx += 1;
    }
    if depth > 0 {
        return Err(source_parse_error(
            opener_line,
            SourceParseDiagnostic::MissingDelimiter,
            "inline declaration requires closing `}`",
        ));
    }
    Ok((combined, idx))
}

pub(super) fn source_declaration_brace_delta(statement: &str) -> isize {
    source_brace_delta(statement)
}

fn source_brace_delta(statement: &str) -> isize {
    let mut delta = 0isize;
    let mut in_string = false;
    let mut prev_was_escape = false;

    for ch in statement.chars() {
        if in_string {
            if ch == '"' && !prev_was_escape {
                in_string = false;
            }
            prev_was_escape = ch == '\\' && !prev_was_escape;
            continue;
        }
        match ch {
            '"' => {
                in_string = true;
                prev_was_escape = false;
            }
            '{' => delta += 1,
            '}' => delta -= 1,
            _ => {
                prev_was_escape = false;
            }
        }
    }

    delta
}

pub(crate) fn source_block_to_expr(lines: &[(usize, usize, String)]) -> Result<String, CliError> {
    let Some((last_line, _last_column, last_statement)) = lines.last() else {
        return Err(source_parse_error(
            1,
            SourceParseDiagnostic::InvalidDeclaration,
            "function block body cannot be empty",
        ));
    };
    let mut final_expr = last_statement.as_str();
    if let Some(rest) = final_expr.strip_prefix("return ") {
        final_expr = rest.trim();
    }
    if final_expr.starts_with("let ") {
        return Err(source_parse_error_for_fragment(
            *last_line,
            SourceParseDiagnostic::InvalidDeclaration,
            final_expr,
            "function block must end with an expression or `return expression`",
        ));
    }

    let mut body = lower_source_expr(final_expr, *last_line)?;
    for (line_num, column, statement) in lines[..lines.len().saturating_sub(1)].iter().rev() {
        let Some(rest) = statement.strip_prefix("let ") else {
            let lowered_statement = lower_source_expr(statement, *line_num)?;
            let name = source_statement_binding_name(*line_num);
            body = format!("let({name}, {lowered_statement}, {body})");
            continue;
        };
        let rest_column = *column + 4;
        let (binding, value) = rest.split_once('=').ok_or_else(|| {
            source_parse_error_for_fragment(
                *line_num,
                SourceParseDiagnostic::InvalidDeclaration,
                statement,
                "let statement requires `let name = expression`",
            )
        })?;
        let binding_column = trimmed_fragment_column(rest_column, binding);
        let binding = binding.trim();
        let value = value.trim();
        let (name, name_column, ty) = if let Some((name, ty)) = binding.split_once(':') {
            let name_column = trimmed_fragment_column(binding_column, name);
            let ty_column = trimmed_fragment_column(binding_column + name.chars().count() + 1, ty);
            let name = name.trim();
            let ty = ty.trim();
            if ty.is_empty() {
                return Err(source_parse_error_for_fragment_at(
                    *line_num,
                    ty_column,
                    SourceParseDiagnostic::InvalidType,
                    ty,
                    "typed let statement requires a type annotation",
                ));
            }
            validate_source_type_name_at(ty, *line_num, ty_column)?;
            (name, name_column, Some(normalize_source_type_name(ty)))
        } else {
            (binding, binding_column, None)
        };
        validate_source_user_local_name_at(name, *line_num, name_column)?;
        if value.is_empty() {
            return Err(source_parse_error_for_fragment(
                *line_num,
                SourceParseDiagnostic::InvalidDeclaration,
                statement,
                "let statement requires a value expression",
            ));
        }
        let lowered_value = lower_source_expr(value, *line_num)?;
        body = if let Some(ty) = ty.as_deref() {
            format!("let_typed({name}, {ty}, {line_num}, {lowered_value}, {body})")
        } else {
            format!("let({name}, {lowered_value}, {body})")
        };
    }

    Ok(body)
}

pub(super) fn parse_source_test(
    rest: &str,
    line_num: usize,
    base_column: usize,
) -> Result<SourceTest, CliError> {
    let (head, body) = rest.split_once('=').ok_or_else(|| {
        source_parse_error_for_fragment(
            line_num,
            SourceParseDiagnostic::InvalidDeclaration,
            rest,
            "test declaration requires `= body`",
        )
    })?;
    build_source_test(head, body.trim(), line_num, base_column, rest)
}

pub(super) fn parse_source_test_with_body(
    rest: &str,
    line_num: usize,
    base_column: usize,
    body: String,
) -> Result<SourceTest, CliError> {
    build_source_test_with_options(rest, body.trim(), line_num, base_column, rest, true)
}

fn build_source_test(
    head: &str,
    body: &str,
    line_num: usize,
    base_column: usize,
    diagnostic_fragment: &str,
) -> Result<SourceTest, CliError> {
    build_source_test_with_options(head, body, line_num, base_column, diagnostic_fragment, false)
}

fn build_source_test_with_options(
    head: &str,
    body: &str,
    line_num: usize,
    base_column: usize,
    diagnostic_fragment: &str,
    body_already_lowered: bool,
) -> Result<SourceTest, CliError> {
    let (raw_name_text, raw_name, return_type, return_type_column) =
        if let Some((name, ty)) = head.split_once("->") {
            (
                name,
                name.trim(),
                ty.trim(),
                source_fragment_column(base_column, head, ty),
            )
        } else {
            (head, head.trim(), "Bool", base_column)
        };
    validate_source_name_at(
        raw_name,
        line_num,
        trimmed_fragment_column(base_column, raw_name_text),
    )?;

    let body = body.trim();
    if return_type.is_empty() || body.is_empty() {
        return Err(source_parse_error_for_fragment(
            line_num,
            SourceParseDiagnostic::InvalidDeclaration,
            diagnostic_fragment,
            "test return type and body must be non-empty",
        ));
    }
    validate_source_type_name_at(return_type, line_num, return_type_column)?;
    let return_type = normalize_source_type_name(return_type);

    let (lowered_body, source_body) = if body_already_lowered {
        (body.to_string(), None)
    } else {
        let return_expr = source_optional_return_expr(body);
        (
            lower_source_expr(return_expr, line_num)?,
            Some(lower_source_expr_for_format(return_expr, line_num)?),
        )
    };

    Ok(SourceTest {
        name: normalize_test_name(raw_name),
        return_type,
        body: lowered_body,
        source_body,
        line_num,
        source_path: None,
    })
}

fn source_optional_return_expr(expr: &str) -> &str {
    expr.trim()
        .strip_prefix("return ")
        .map(str::trim)
        .unwrap_or_else(|| expr.trim())
}

pub(super) fn parse_source_grant(
    rest: &str,
    line_num: usize,
    base_column: usize,
) -> Result<SourceGrant, CliError> {
    let parts = rest.split_whitespace().collect::<Vec<_>>();
    if parts.len() != 2 {
        return Err(source_parse_error_for_fragment(
            line_num,
            SourceParseDiagnostic::InvalidDeclaration,
            rest,
            "grant declaration requires `grant target capability`",
        ));
    }
    let target = normalize_grant_target(parts[0]);
    let capability = parts[1].to_string();
    let target_column = trimmed_fragment_column(base_column, rest);
    validate_source_name_at(&target, line_num, target_column)?;
    let capability_column = rest
        .find(parts[0])
        .and_then(|target_idx| {
            let after_target_idx = target_idx + parts[0].len();
            rest[after_target_idx..]
                .find(parts[1])
                .map(|capability_idx| after_target_idx + capability_idx)
        })
        .map(|capability_idx| base_column + rest[..capability_idx].chars().count())
        .unwrap_or(base_column);
    validate_source_name_at(&capability, line_num, capability_column)?;

    Ok(SourceGrant { target, capability })
}
