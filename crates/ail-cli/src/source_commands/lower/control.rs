use super::*;

pub(super) fn lower_if_expr(rest: &str, line_num: usize) -> Result<String, CliError> {
    let open_then = rest.find('{').ok_or_else(|| {
        source_lower_error(
            line_num,
            SourceLowerDiagnostic::ControlExpression,
            "if expression requires `{ then } else { else }`",
        )
    })?;
    let cond = rest[..open_then].trim();
    if cond.is_empty() {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::ControlExpression,
            "if expression requires a condition",
        ));
    }

    let then_close = matching_brace(rest, open_then).ok_or_else(|| {
        source_lower_error(
            line_num,
            SourceLowerDiagnostic::ControlExpression,
            "if expression has unclosed then block",
        )
    })?;
    let then_expr = lower_source_statement_block_expr(
        &rest[open_then + 1..then_close],
        line_num,
        SourceLowerDiagnostic::ControlExpression,
        "if branches must be non-empty expressions",
    )?;
    let after_then = rest[then_close + 1..].trim_start();
    let after_else = after_then.strip_prefix("else").ok_or_else(|| {
        source_lower_error(
            line_num,
            SourceLowerDiagnostic::ControlExpression,
            "if expression requires an else branch",
        )
    })?;
    let after_else = after_else.trim_start();
    let else_expr = if let Some(nested_if) = after_else.strip_prefix("if ") {
        lower_if_expr(nested_if, line_num)?
    } else {
        if !after_else.starts_with('{') {
            return Err(source_lower_error(
                line_num,
                SourceLowerDiagnostic::ControlExpression,
                "else branch requires `{ expression }` or `if condition { expression } else { expression }`",
            ));
        }
        let else_close = matching_brace(after_else, 0).ok_or_else(|| {
            source_lower_error(
                line_num,
                SourceLowerDiagnostic::ControlExpression,
                "if expression has unclosed else block",
            )
        })?;
        if !after_else[else_close + 1..].trim().is_empty() {
            return Err(source_lower_error(
                line_num,
                SourceLowerDiagnostic::ControlExpression,
                "unexpected tokens after if expression",
            ));
        }
        lower_source_statement_block_expr(
            &after_else[1..else_close],
            line_num,
            SourceLowerDiagnostic::ControlExpression,
            "if branches must be non-empty expressions",
        )?
    };

    Ok(format!(
        "if({}, {}, {})",
        lower_source_expr(cond, line_num)?,
        then_expr,
        else_expr
    ))
}

pub(super) fn lower_source_statement_block_expr(
    block: &str,
    line_num: usize,
    diagnostic: SourceLowerDiagnostic,
    empty_message: &str,
) -> Result<String, CliError> {
    let lines = source_statement_block_lines(block, line_num);
    if lines.is_empty() {
        return Err(source_lower_error(line_num, diagnostic, empty_message));
    }
    source_block_to_expr(&lines)
}

fn source_statement_block_lines(block: &str, base_line: usize) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut pending: Option<(usize, usize, String, isize)> = None;
    for (offset, raw_line) in block.lines().enumerate() {
        let Some((column, statement)) = normalize_source_line(raw_line) else {
            continue;
        };
        let line_num = base_line + offset;
        if let Some((start_line, start_column, combined, depth)) = pending.as_mut() {
            append_source_statement_block_fragment(combined, &statement);
            *depth += source_statement_brace_delta(&statement);
            if *depth <= 0 {
                out.push((*start_line, *start_column, combined.clone()));
                pending = None;
            }
            continue;
        }

        let depth = source_statement_brace_delta(&statement);
        if depth > 0 {
            pending = Some((line_num, column, statement, depth));
        } else {
            out.push((line_num, column, statement));
        }
    }
    if let Some((line_num, column, combined, _depth)) = pending {
        out.push((line_num, column, combined));
    }
    out
}

fn append_source_statement_block_fragment(combined: &mut String, fragment: &str) {
    if !combined.is_empty() {
        combined.push('\n');
    }
    combined.push_str(fragment.trim());
}

fn source_statement_brace_delta(statement: &str) -> isize {
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
