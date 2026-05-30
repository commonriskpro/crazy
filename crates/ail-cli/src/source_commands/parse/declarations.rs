use super::*;

pub(super) fn parse_source_module(rest: &str, line_num: usize) -> Result<String, CliError> {
    let module = rest.trim();
    validate_source_name(module, line_num)?;
    Ok(module.to_string())
}

pub(super) fn parse_source_import(rest: &str, line_num: usize) -> Result<String, CliError> {
    let import = rest.trim();
    let Some(import) = import.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return Err(CliError::ParseError(format!(
            "line {line_num}: import declaration requires `use \"relative/path.ail\"`"
        )));
    };
    if import.is_empty() || import.contains('\0') || Path::new(import).is_absolute() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: import path must be a non-empty relative path"
        )));
    }
    if import.contains('\\') {
        return Err(CliError::ParseError(format!(
            "line {line_num}: import path `{import}` must use `/` separators"
        )));
    }
    if import.contains(':') {
        return Err(CliError::ParseError(format!(
            "line {line_num}: import path `{import}` must not contain `:`"
        )));
    }
    if import.contains("//") {
        return Err(CliError::ParseError(format!(
            "line {line_num}: import path `{import}` must not contain empty path segments"
        )));
    }
    if import.chars().any(char::is_whitespace) {
        return Err(CliError::ParseError(format!(
            "line {line_num}: import path `{import}` must not contain whitespace"
        )));
    }
    if Path::new(import)
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(CliError::ParseError(format!(
            "line {line_num}: import path `{import}` must not contain `..`"
        )));
    }
    if !import.starts_with("./") {
        return Err(CliError::ParseError(format!(
            "line {line_num}: local import path `{import}` must start with `./`"
        )));
    }
    if Path::new(import).extension().and_then(|ext| ext.to_str()) != Some("ail") {
        return Err(CliError::ParseError(format!(
            "line {line_num}: import path `{import}` must end with `.ail`"
        )));
    }
    Ok(import.to_string())
}

pub(super) fn parse_source_capability(rest: &str, line_num: usize) -> Result<String, CliError> {
    let capability = rest.trim();
    validate_source_name(capability, line_num)?;
    Ok(capability.to_string())
}

pub(super) fn parse_source_const(rest: &str, line_num: usize) -> Result<SourceConst, CliError> {
    let (head, body) = rest.split_once('=').ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: const declaration requires `= body`"
        ))
    })?;
    let (name, return_type) = head.split_once(':').ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: const declaration requires `name: Type`"
        ))
    })?;
    let name = name.trim();
    let return_type = return_type.trim();
    let body = body.trim();
    validate_source_name(name, line_num)?;
    validate_source_type_name(return_type, line_num)?;
    let return_type = normalize_source_type_name(return_type);
    if body.is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: const body must be non-empty"
        )));
    }
    Ok(SourceConst {
        name: normalize_function_name(name),
        return_type,
        body: lower_source_expr(body, line_num)?,
        line_num,
    })
}

pub(super) fn parse_source_function(
    rest: &str,
    line_num: usize,
) -> Result<SourceFunction, CliError> {
    let (name, params, return_and_body) = parse_source_function_signature(rest, line_num)?;
    let (return_type, body) = return_and_body.split_once('=').ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: function declaration requires `= body`"
        ))
    })?;

    build_source_function(name, params, return_type.trim(), body.trim(), line_num)
}

pub(super) fn parse_source_function_with_body(
    rest: &str,
    line_num: usize,
    body: String,
) -> Result<SourceFunction, CliError> {
    let (name, params, return_type) = parse_source_function_signature(rest, line_num)?;
    build_source_function(name, params, return_type.trim(), body.trim(), line_num)
}

pub(super) fn parse_source_function_signature(
    rest: &str,
    line_num: usize,
) -> Result<(String, Vec<SourceParam>, String), CliError> {
    let open_paren = rest.find('(').ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: function declaration requires `()`"
        ))
    })?;
    let raw_name = rest[..open_paren].trim();
    validate_source_name(raw_name, line_num)?;

    let params_start = open_paren + 1;
    let close_paren = rest[params_start..].find(')').ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: function declaration requires closing `)`"
        ))
    })? + params_start;
    let params = parse_source_params(&rest[params_start..close_paren], line_num)?;
    let after_params = &rest[close_paren + 1..];
    let after_arrow = after_params
        .trim_start()
        .strip_prefix("->")
        .ok_or_else(|| {
            CliError::ParseError(format!(
                "line {line_num}: function declaration requires `-> Type`"
            ))
        })?;
    Ok((
        normalize_function_name(raw_name),
        params,
        after_arrow.trim().to_string(),
    ))
}

pub(super) fn build_source_function(
    name: String,
    params: Vec<SourceParam>,
    return_type: &str,
    body: &str,
    line_num: usize,
) -> Result<SourceFunction, CliError> {
    if return_type.is_empty() || body.is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: function return type and body must be non-empty"
        )));
    }
    validate_source_type_name(return_type, line_num)?;
    let return_type = normalize_source_type_name(return_type);

    Ok(SourceFunction {
        name,
        params,
        return_type,
        body: lower_source_expr(body, line_num)?,
        line_num,
    })
}

pub(super) fn parse_source_params(
    params: &str,
    line_num: usize,
) -> Result<Vec<SourceParam>, CliError> {
    let params = params.trim();
    if params.is_empty() {
        return Ok(vec![]);
    }

    let mut seen = BTreeSet::new();
    split_source_param_list(params)
        .into_iter()
        .map(|raw| {
            let param = raw.trim();
            let (name, ty) = param.split_once(':').ok_or_else(|| {
                CliError::ParseError(format!(
                    "line {line_num}: function parameters must use `name: Type`"
                ))
            })?;
            let name = name.trim();
            let ty = ty.trim();
            validate_source_local_name(name, line_num)?;
            if !seen.insert(name.to_string()) {
                return Err(CliError::ParseError(format!(
                    "line {line_num}: duplicate parameter `{name}`"
                )));
            }
            if ty.is_empty() {
                return Err(CliError::ParseError(format!(
                    "line {line_num}: parameter `{name}` requires a type"
                )));
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

pub(super) fn collect_braced_body(
    statements: &[(usize, String)],
    mut idx: usize,
    opener_line: usize,
) -> Result<(Vec<(usize, String)>, usize), CliError> {
    let mut body = Vec::new();
    while idx < statements.len() {
        let (line_num, statement) = &statements[idx];
        if statement == "}" {
            return Ok((body, idx + 1));
        }
        if statement.ends_with('{') {
            return Err(CliError::ParseError(format!(
                "line {line_num}: nested source blocks are not supported yet"
            )));
        }
        body.push((*line_num, statement.clone()));
        idx += 1;
    }

    Err(CliError::ParseError(format!(
        "line {opener_line}: function block requires closing `}}`"
    )))
}

pub(super) fn source_block_to_expr(lines: &[(usize, String)]) -> Result<String, CliError> {
    let Some((last_line, last_statement)) = lines.last() else {
        return Err(CliError::ParseError(
            "function block body cannot be empty".to_string(),
        ));
    };
    let mut final_expr = last_statement.as_str();
    if let Some(rest) = final_expr.strip_prefix("return ") {
        final_expr = rest.trim();
    }
    if final_expr.starts_with("let ") {
        return Err(CliError::ParseError(format!(
            "line {last_line}: function block must end with an expression or `return expression`"
        )));
    }

    let mut body = lower_source_expr(final_expr, *last_line)?;
    for (line_num, statement) in lines[..lines.len().saturating_sub(1)].iter().rev() {
        let Some(rest) = statement.strip_prefix("let ") else {
            return Err(CliError::ParseError(format!(
                "line {line_num}: only `let name = expression` statements may precede the final expression"
            )));
        };
        let (binding, value) = rest.split_once('=').ok_or_else(|| {
            CliError::ParseError(format!(
                "line {line_num}: let statement requires `let name = expression`"
            ))
        })?;
        let binding = binding.trim();
        let value = value.trim();
        let (name, ty) = if let Some((name, ty)) = binding.split_once(':') {
            let name = name.trim();
            let ty = ty.trim();
            if ty.is_empty() {
                return Err(CliError::ParseError(format!(
                    "line {line_num}: typed let statement requires a type annotation"
                )));
            }
            validate_source_type_name(ty, *line_num)?;
            (name, Some(normalize_source_type_name(ty)))
        } else {
            (binding, None)
        };
        validate_source_local_name(name, *line_num)?;
        if value.is_empty() {
            return Err(CliError::ParseError(format!(
                "line {line_num}: let statement requires a value expression"
            )));
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

pub(super) fn parse_source_test(rest: &str, line_num: usize) -> Result<SourceTest, CliError> {
    let (head, body) = rest.split_once('=').ok_or_else(|| {
        CliError::ParseError(format!(
            "line {line_num}: test declaration requires `= body`"
        ))
    })?;
    let (raw_name, return_type) = if let Some((name, ty)) = head.split_once("->") {
        (name.trim(), ty.trim())
    } else {
        (head.trim(), "Bool")
    };
    validate_source_name(raw_name, line_num)?;

    let body = body.trim();
    if return_type.is_empty() || body.is_empty() {
        return Err(CliError::ParseError(format!(
            "line {line_num}: test return type and body must be non-empty"
        )));
    }
    validate_source_type_name(return_type, line_num)?;
    let return_type = normalize_source_type_name(return_type);

    Ok(SourceTest {
        name: normalize_test_name(raw_name),
        return_type,
        body: lower_source_expr(body, line_num)?,
        line_num,
    })
}

pub(super) fn parse_source_grant(rest: &str, line_num: usize) -> Result<SourceGrant, CliError> {
    let parts = rest.split_whitespace().collect::<Vec<_>>();
    if parts.len() != 2 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: grant declaration requires `grant target capability`"
        )));
    }
    let target = normalize_grant_target(parts[0]);
    let capability = parts[1].to_string();
    validate_source_name(&target, line_num)?;
    validate_source_name(&capability, line_num)?;

    Ok(SourceGrant { target, capability })
}
