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
