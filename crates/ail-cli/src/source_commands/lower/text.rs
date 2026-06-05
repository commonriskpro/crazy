use super::*;

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
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::TextHelper,
            "text_eq requires `text_eq(left, right)`",
        ));
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
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::TextHelper,
            "text_trim requires `text_trim(value)`",
        ));
    }
    Ok(Some(format!(
        "text.trim({})",
        lower_source_expr(&args[0], line_num)?
    )))
}

pub(super) fn lower_source_length_expr(
    expr: &str,
    line_num: usize,
) -> Result<Option<String>, CliError> {
    let Some((func, args)) = parse_source_call(expr) else {
        return Ok(None);
    };
    if !matches!(
        func.as_str(),
        "text.len" | "text.length" | "text_length" | "list.length" | "list_length"
    ) {
        return Ok(None);
    }
    if args.len() != 1 {
        let diagnostic = if func.starts_with("list") {
            SourceLowerDiagnostic::CollectionArity
        } else {
            SourceLowerDiagnostic::TextHelper
        };
        return Err(source_lower_error(
            line_num,
            diagnostic,
            format!("{func} requires `{func}(value)`"),
        ));
    }
    let lowered_arg = lower_source_expr(&args[0], line_num)?;
    if format_aliases_preserved() || type_aliases_preserved() {
        // Distinct length aliases collapse to the same `len(..)` core; preserve the
        // `text.length`/`list.length` spelling so the formatter can round-trip them
        // and the type checker can enforce the matching collection shape.
        let preserved = match func.as_str() {
            "list.length" | "list_length" => "list.length",
            _ => "text.length",
        };
        return Ok(Some(format!("{preserved}({lowered_arg})")));
    }
    Ok(Some(format!("len({lowered_arg})")))
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
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::TextHelper,
            "text_contains requires `text_contains(haystack, needle)`",
        ));
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
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::TextHelper,
            "text_index_of requires `text_index_of(haystack, needle)`",
        ));
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
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::TextHelper,
            "text_parse_int_or requires `text_parse_int_or(value, fallback)`",
        ));
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
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::TextHelper,
            "text_byte_at_or requires `text_byte_at_or(value, index, fallback)`",
        ));
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
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::TextHelper,
            "text_slice requires `text_slice(value, start, length)`",
        ));
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
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::TextHelper,
            "text_replace_first requires `text_replace_first(value, needle, replacement)`",
        ));
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
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::TextHelper,
            format!("{func} requires `{expected}`"),
        ));
    }
    Ok(Some(format!(
        "{lowered_func}({}, {})",
        lower_source_expr(&args[0], line_num)?,
        lower_source_expr(&args[1], line_num)?
    )))
}
