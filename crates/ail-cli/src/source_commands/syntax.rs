use super::*;

pub(super) fn parse_source_call(expr: &str) -> Option<(String, Vec<String>)> {
    let expr = expr.trim();
    let open = expr.find('(')?;
    if !expr.ends_with(')') {
        return None;
    }
    if matching_paren(expr, open)? != expr.len() - 1 {
        return None;
    }
    let func = expr[..open].trim();
    if !is_source_ident(func) {
        return None;
    }
    let inner = &expr[open + 1..expr.len() - 1];
    Some((func.to_string(), split_source_args(inner)))
}

pub(super) fn split_source_args(args: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut angle_depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;

    for (idx, ch) in args.char_indices() {
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
            ',' if paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && angle_depth == 0 =>
            {
                out.push(args[start..idx].trim().to_string());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
        prev_was_escape = false;
    }

    let tail = args[start..].trim();
    if !tail.is_empty() {
        out.push(tail.to_string());
    }
    out
}

pub(super) fn matching_paren(s: &str, open_idx: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;

    for (idx, ch) in s.char_indices().skip_while(|(idx, _)| *idx < open_idx) {
        if in_string {
            if ch == '"' && !prev_was_escape {
                in_string = false;
            }
            prev_was_escape = ch == '\\' && !prev_was_escape;
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
        prev_was_escape = false;
    }
    None
}

pub(super) fn matching_bracket(s: &str, open_idx: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;

    for (idx, ch) in s.char_indices().skip_while(|(idx, _)| *idx < open_idx) {
        if in_string {
            if ch == '"' && !prev_was_escape {
                in_string = false;
            }
            prev_was_escape = ch == '\\' && !prev_was_escape;
            continue;
        }
        match ch {
            '"' => in_string = true,
            '[' => depth += 1,
            ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
        prev_was_escape = false;
    }
    None
}

pub(super) fn is_source_ident(name: &str) -> bool {
    !name.is_empty() && is_valid_source_name_chars(name) && source_name_segments_are_valid(name)
}

pub(super) fn is_source_local_ident(name: &str) -> bool {
    is_source_ident(name) && !name.contains('.')
}

pub(super) fn validate_source_local_expr_name(name: &str) -> Result<(), CliError> {
    if !is_source_ident(name) {
        return Err(CliError::ParseError(format!(
            "local binding name `{name}` is not a valid identifier"
        )));
    }
    if name.contains('.') {
        return Err(CliError::ParseError(format!(
            "local binding name `{name}` must not contain `.`"
        )));
    }
    Ok(())
}

pub(super) fn normalize_source_line(raw_line: &str) -> Option<String> {
    let trimmed = raw_line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
        return None;
    }
    let without_comment = strip_source_comment(trimmed);
    let statement = without_comment.trim().trim_end_matches(';').trim();
    if statement.is_empty() {
        None
    } else {
        Some(statement.to_string())
    }
}

pub(super) fn strip_source_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut prev_was_escape = false;
    let mut chars = line.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
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
            '/' if chars.peek().is_some_and(|(_, next)| *next == '/') => return &line[..idx],
            _ => {
                prev_was_escape = false;
            }
        }
    }

    line
}
