use super::*;

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
