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

pub(super) fn parse_source_import(rest: &str, line_num: usize) -> Result<String, CliError> {
    let import = rest.trim();
    let Some(import) = import.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return Err(source_parse_error_for_fragment(
            line_num,
            SourceParseDiagnostic::InvalidDeclaration,
            rest,
            "import declaration requires `use \"relative/path.ail\"`",
        ));
    };
    if import.is_empty() || import.contains('\0') || Path::new(import).is_absolute() {
        return Err(source_parse_error_for_fragment(
            line_num,
            SourceParseDiagnostic::InvalidDeclaration,
            import,
            "import path must be a non-empty relative path",
        ));
    }
    if import.contains('\\') {
        return Err(source_parse_error_for_fragment(
            line_num,
            SourceParseDiagnostic::InvalidDeclaration,
            import,
            format!("import path `{import}` must use `/` separators"),
        ));
    }
    if import.contains(':') {
        return Err(source_parse_error_for_fragment(
            line_num,
            SourceParseDiagnostic::InvalidDeclaration,
            import,
            format!("import path `{import}` must not contain `:`"),
        ));
    }
    if import.contains("//") {
        return Err(source_parse_error_for_fragment(
            line_num,
            SourceParseDiagnostic::InvalidDeclaration,
            import,
            format!("import path `{import}` must not contain empty path segments"),
        ));
    }
    if import.chars().any(char::is_whitespace) {
        return Err(source_parse_error_for_fragment(
            line_num,
            SourceParseDiagnostic::InvalidDeclaration,
            import,
            format!("import path `{import}` must not contain whitespace"),
        ));
    }
    if Path::new(import)
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(source_parse_error_for_fragment(
            line_num,
            SourceParseDiagnostic::InvalidDeclaration,
            import,
            format!("import path `{import}` must not contain `..`"),
        ));
    }
    if !import.starts_with("./") {
        return Err(source_parse_error_for_fragment(
            line_num,
            SourceParseDiagnostic::InvalidDeclaration,
            import,
            format!("local import path `{import}` must start with `./`"),
        ));
    }
    if Path::new(import).extension().and_then(|ext| ext.to_str()) != Some("ail") {
        return Err(source_parse_error_for_fragment(
            line_num,
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
    let name = name.trim();
    let return_type = return_type.trim();
    let body = body.trim();
    validate_source_name_at(name, line_num, name_column)?;
    validate_source_type_name(return_type, line_num)?;
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
        line_num,
        source_path: None,
    })
}

pub(super) fn parse_source_function(
    rest: &str,
    line_num: usize,
    base_column: usize,
) -> Result<SourceFunction, CliError> {
    let (name, params, return_and_body) =
        parse_source_function_signature(rest, line_num, base_column)?;
    let (return_type, body) = return_and_body.split_once('=').ok_or_else(|| {
        source_parse_error_for_fragment(
            line_num,
            SourceParseDiagnostic::InvalidDeclaration,
            return_and_body,
            "function declaration requires `= body`",
        )
    })?;

    build_source_function(name, params, return_type.trim(), body.trim(), line_num)
}

pub(super) fn parse_source_function_with_body(
    rest: &str,
    line_num: usize,
    base_column: usize,
    body: String,
) -> Result<SourceFunction, CliError> {
    let (name, params, return_type) = parse_source_function_signature(rest, line_num, base_column)?;
    build_source_function(name, params, return_type.trim(), body.trim(), line_num)
}

pub(super) fn parse_source_function_signature(
    rest: &str,
    line_num: usize,
    base_column: usize,
) -> Result<(String, Vec<SourceParam>, String), CliError> {
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
    Ok((
        normalize_function_name(raw_name),
        params,
        after_arrow.trim().to_string(),
    ))
}

fn trimmed_fragment_column(base_column: usize, text: &str) -> usize {
    base_column + text.chars().take_while(|ch| ch.is_whitespace()).count()
}

pub(super) fn build_source_function(
    name: String,
    params: Vec<SourceParam>,
    return_type: &str,
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
    validate_source_type_name(return_type, line_num)?;
    let return_type = normalize_source_type_name(return_type);

    Ok(SourceFunction {
        name,
        params,
        return_type,
        body: lower_source_expr(body, line_num)?,
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
            let name = name.trim();
            let ty = ty.trim();
            validate_source_local_name_at(name, line_num, name_column)?;
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
                    param_column,
                    SourceParseDiagnostic::InvalidType,
                    param,
                    format!("parameter `{name}` requires a type"),
                ));
            }
            validate_source_type_name(ty, line_num)?;
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
    while idx < statements.len() {
        let (line_num, column, statement) = &statements[idx];
        if statement == "}" {
            return Ok((body, idx + 1));
        }
        if statement.ends_with('{') {
            return Err(source_parse_error_for_fragment(
                *line_num,
                SourceParseDiagnostic::InvalidDeclaration,
                statement,
                "nested source blocks are not supported yet",
            ));
        }
        body.push((*line_num, *column, statement.clone()));
        idx += 1;
    }

    Err(source_parse_error(
        opener_line,
        SourceParseDiagnostic::MissingDelimiter,
        "function block requires closing `}`",
    ))
}

pub(super) fn source_block_to_expr(lines: &[(usize, usize, String)]) -> Result<String, CliError> {
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
            return Err(source_parse_error_for_fragment(
                *line_num,
                SourceParseDiagnostic::InvalidDeclaration,
                statement,
                "only `let name = expression` statements may precede the final expression",
            ));
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
        let (name, ty) = if let Some((name, ty)) = binding.split_once(':') {
            let name_column = trimmed_fragment_column(binding_column, name);
            let name = name.trim();
            let ty = ty.trim();
            if ty.is_empty() {
                return Err(source_parse_error_for_fragment(
                    *line_num,
                    SourceParseDiagnostic::InvalidType,
                    statement,
                    "typed let statement requires a type annotation",
                ));
            }
            validate_source_type_name(ty, *line_num)?;
            (name, name_column, Some(normalize_source_type_name(ty)))
        } else {
            (binding, binding_column, None)
        };
        validate_source_local_name_at(name, *line_num, name_column)?;
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
    let (raw_name_text, raw_name, return_type) = if let Some((name, ty)) = head.split_once("->") {
        (name, name.trim(), ty.trim())
    } else {
        (head, head.trim(), "Bool")
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
            rest,
            "test return type and body must be non-empty",
        ));
    }
    validate_source_type_name(return_type, line_num)?;
    let return_type = normalize_source_type_name(return_type);

    Ok(SourceTest {
        name: normalize_test_name(raw_name),
        return_type,
        body: lower_source_expr(body, line_num)?,
        line_num,
        source_path: None,
    })
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
