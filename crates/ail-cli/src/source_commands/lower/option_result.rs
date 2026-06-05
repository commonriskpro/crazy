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
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::OptionResultHelper,
            "source constructor `None` requires no values",
        ));
    }
    let lowered_func = match func.as_str() {
        "Some" => "some",
        "Ok" => "ok",
        "Err" => "err",
        _ => return Ok(None),
    };
    if args.len() != 1 {
        // A unary constructor with the wrong argument count is reported both as a
        // malformed constructor and as a call-arity violation so callers that key on
        // either phrasing (compile vs. lsp diagnostics) see a consistent diagnostic.
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::OptionResultHelper,
            format!(
                "source constructor `{func}` requires exactly one value; function call `{func}` expects 1 argument(s), got {}",
                args.len()
            ),
        ));
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
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::OptionResultHelper,
            format!("{func} requires `{func}(option)`"),
        ));
    }
    let lowered_arg = lower_source_expr(&args[0], line_num)?;
    if type_aliases_preserved() {
        // The type checker needs the predicate identity to report shape diagnostics
        // against `is_some`/`is_none` instead of the collapsed `match` form.
        return Ok(Some(format!("{func}({lowered_arg})")));
    }
    if format_aliases_preserved() {
        // Namespaced predicates keep their spelling for the formatter; the bare
        // `is_some`/`is_none` aliases still collapse to the canonical `match` form.
        let preserved = match func.as_str() {
            "option.is_some" | "option_is_some" => Some("option.is_some"),
            "option.is_none" | "option_is_none" => Some("option.is_none"),
            _ => None,
        };
        if let Some(name) = preserved {
            return Ok(Some(format!("{name}({lowered_arg})")));
        }
    }
    Ok(Some(format!(
        "match({lowered_arg}, Some(_), {some_body}, None, {none_body})"
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
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::OptionResultHelper,
            format!("{func} requires `{func}(result)`"),
        ));
    }
    let lowered_arg = lower_source_expr(&args[0], line_num)?;
    if type_aliases_preserved() {
        // The type checker needs the predicate identity to report shape diagnostics
        // against `is_ok`/`is_err` instead of the collapsed `match` form.
        return Ok(Some(format!("{func}({lowered_arg})")));
    }
    if format_aliases_preserved() {
        // Namespaced predicates keep their spelling for the formatter; the bare
        // `is_ok`/`is_err` aliases still collapse to the canonical `match` form.
        let preserved = match func.as_str() {
            "result.is_ok" | "result_is_ok" => Some("result.is_ok"),
            "result.is_err" | "result_is_err" => Some("result.is_err"),
            _ => None,
        };
        if let Some(name) = preserved {
            return Ok(Some(format!("{name}({lowered_arg})")));
        }
    }
    Ok(Some(format!(
        "match({lowered_arg}, Ok(_), {ok_body}, Err(_), {err_body})"
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
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::OptionResultHelper,
            format!("{func} requires `{func}(option, error)`"),
        ));
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
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::OptionResultHelper,
            format!("{func} requires `{func}(result, fallback)`"),
        ));
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
        let diagnostic = if func.starts_with("text") {
            SourceLowerDiagnostic::TextHelper
        } else {
            SourceLowerDiagnostic::CollectionArity
        };
        return Err(source_lower_error(
            line_num,
            diagnostic,
            "is_empty requires `is_empty(value)`",
        ));
    }
    let lowered_arg = lower_source_expr(&args[0], line_num)?;
    if type_aliases_preserved() {
        // The type checker needs the original helper identity to report shape
        // diagnostics against `is_empty`/`list.is_empty`/`text.is_empty`.
        let preserved = match func.as_str() {
            "text.is_empty" | "text_is_empty" => "text.is_empty",
            "list.is_empty" | "list_is_empty" => "list.is_empty",
            _ => "is_empty",
        };
        return Ok(Some(format!("{preserved}({lowered_arg})")));
    }
    if format_aliases_preserved() {
        // Namespaced emptiness checks keep their spelling for the formatter; the bare
        // `is_empty` alias still collapses to the canonical `eq(len(..), 0)` form.
        let preserved = match func.as_str() {
            "text.is_empty" | "text_is_empty" => Some("text.is_empty"),
            "list.is_empty" | "list_is_empty" => Some("list.is_empty"),
            _ => None,
        };
        if let Some(name) = preserved {
            return Ok(Some(format!("{name}({lowered_arg})")));
        }
    }
    Ok(Some(format!("eq(len({lowered_arg}), 0)")))
}
