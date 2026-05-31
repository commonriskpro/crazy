use super::*;

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
        "is_some" | "option.is_some" | "option_is_some" => ("true", "false"),
        "is_none" | "option.is_none" | "option_is_none" => ("false", "true"),
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
        "is_ok" | "result.is_ok" | "result_is_ok" => ("true", "false"),
        "is_err" | "result.is_err" | "result_is_err" => ("false", "true"),
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

pub(super) fn lower_source_option_ok_or_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if !matches!(func.as_str(), "ok_or" | "option.ok_or" | "option_ok_or") {
        return Ok(None);
    }
    if args.len() != 2 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: {func} requires `{func}(option, error)`"
        )));
    }
    Ok(Some(format!(
        "match({}, Some(__ail_ok), ok(__ail_ok), None, err({}))",
        lower_source_expr(&args[0], line_num)?,
        lower_source_expr(&args[1], line_num)?
    )))
}

pub(super) fn lower_source_result_unwrap_or_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if !matches!(func.as_str(), "result.unwrap_or" | "result_unwrap_or") {
        return Ok(None);
    }
    if args.len() != 2 {
        return Err(CliError::ParseError(format!(
            "line {line_num}: {func} requires `{func}(result, fallback)`"
        )));
    }
    Ok(Some(format!(
        "match({}, Ok(__ail_unwrap), __ail_unwrap, Err(_), {})",
        lower_source_expr(&args[0], line_num)?,
        lower_source_expr(&args[1], line_num)?
    )))
}

pub(super) fn lower_source_is_empty_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if !matches!(
        func.as_str(),
        "is_empty" | "text.is_empty" | "text_is_empty" | "list.is_empty" | "list_is_empty"
    ) {
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
