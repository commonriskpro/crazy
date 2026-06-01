use super::*;

#[derive(Clone, Copy)]
pub(super) enum SourceParseDiagnostic {
    InvalidDeclaration,
    InvalidName,
    InvalidPattern,
    InvalidType,
    MissingDelimiter,
    UnexpectedToken,
}

pub(super) struct SourceParseDiagnosticDescriptor {
    pub(super) code: &'static str,
    pub(super) category: &'static str,
}

impl SourceParseDiagnostic {
    pub(super) fn descriptor(self) -> SourceParseDiagnosticDescriptor {
        match self {
            SourceParseDiagnostic::InvalidDeclaration => SourceParseDiagnosticDescriptor {
                code: "AIL_SOURCE_PARSE_INVALID_DECLARATION",
                category: "source.parse.declaration",
            },
            SourceParseDiagnostic::InvalidName => SourceParseDiagnosticDescriptor {
                code: "AIL_SOURCE_PARSE_INVALID_NAME",
                category: "source.parse.name",
            },
            SourceParseDiagnostic::InvalidPattern => SourceParseDiagnosticDescriptor {
                code: "AIL_SOURCE_PARSE_INVALID_PATTERN",
                category: "source.parse.pattern",
            },
            SourceParseDiagnostic::InvalidType => SourceParseDiagnosticDescriptor {
                code: "AIL_SOURCE_PARSE_INVALID_TYPE",
                category: "source.parse.type",
            },
            SourceParseDiagnostic::MissingDelimiter => SourceParseDiagnosticDescriptor {
                code: "AIL_SOURCE_PARSE_MISSING_DELIMITER",
                category: "source.parse.delimiter",
            },
            SourceParseDiagnostic::UnexpectedToken => SourceParseDiagnosticDescriptor {
                code: "AIL_SOURCE_PARSE_UNEXPECTED_TOKEN",
                category: "source.parse.token",
            },
        }
    }
}

pub(super) fn source_parse_error(
    line_num: usize,
    diagnostic: SourceParseDiagnostic,
    message: impl AsRef<str>,
) -> CliError {
    source_parse_error_at(line_num, 1, 1, diagnostic, "", message)
}

pub(super) fn source_parse_error_for_fragment(
    line_num: usize,
    diagnostic: SourceParseDiagnostic,
    fragment: &str,
    message: impl AsRef<str>,
) -> CliError {
    source_parse_error_for_fragment_at(line_num, 1, diagnostic, fragment, message)
}

pub(super) fn source_parse_error_for_fragment_at(
    line_num: usize,
    column: usize,
    diagnostic: SourceParseDiagnostic,
    fragment: &str,
    message: impl AsRef<str>,
) -> CliError {
    let width = fragment.chars().count().max(1);
    source_parse_error_at(
        line_num,
        column.max(1),
        width,
        diagnostic,
        fragment,
        message,
    )
}

pub(super) fn source_parse_error_at(
    line_num: usize,
    column: usize,
    width: usize,
    diagnostic: SourceParseDiagnostic,
    snippet: &str,
    message: impl AsRef<str>,
) -> CliError {
    let descriptor = diagnostic.descriptor();
    let end_column = column.saturating_add(width.max(1));
    CliError::ParseError(format!(
        "line {line_num}: {} [{}] category={} span={line_num}:{column}..{line_num}:{end_column} snippet={:?}",
        message.as_ref(),
        descriptor.code,
        descriptor.category,
        redact_source_parse_snippet(snippet)
    ))
}

pub(super) fn source_parse_error_unlocated(
    diagnostic: SourceParseDiagnostic,
    snippet: &str,
    message: impl AsRef<str>,
) -> CliError {
    let descriptor = diagnostic.descriptor();
    CliError::ParseError(format!(
        "{} [{}] category={} span=<unknown> snippet={:?}",
        message.as_ref(),
        descriptor.code,
        descriptor.category,
        redact_source_parse_snippet(snippet)
    ))
}

fn redact_source_parse_snippet(snippet: &str) -> String {
    const MAX_CHARS: usize = 96;
    let mut out = String::new();
    let mut in_string = false;
    let mut string_redacted = false;
    let mut prev_was_escape = false;

    for ch in snippet.chars() {
        if out.chars().count() >= MAX_CHARS {
            out.push('…');
            break;
        }
        if in_string {
            if !string_redacted {
                out.push_str("<redacted>");
                string_redacted = true;
            }
            if ch == '"' && !prev_was_escape {
                out.push('"');
                in_string = false;
                string_redacted = false;
            }
            prev_was_escape = ch == '\\' && !prev_was_escape;
            continue;
        }
        match ch {
            '"' => {
                out.push('"');
                in_string = true;
                prev_was_escape = false;
            }
            ch if ch.is_control() => out.push(' '),
            ch => out.push(ch),
        }
    }

    out.trim().to_string()
}

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
        return Err(source_parse_error_unlocated(
            SourceParseDiagnostic::InvalidName,
            name,
            format!("local binding name `{name}` is not a valid identifier"),
        ));
    }
    if name.contains('.') {
        return Err(source_parse_error_unlocated(
            SourceParseDiagnostic::InvalidName,
            name,
            format!("local binding name `{name}` must not contain `.`"),
        ));
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
