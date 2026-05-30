use super::*;

pub(super) fn strip_wrapping_source_parens(expr: &str) -> Option<&str> {
    if !expr.starts_with('(') || !expr.ends_with(')') {
        return None;
    }
    (matching_paren(expr, 0)? == expr.len() - 1).then(|| expr[1..expr.len() - 1].trim())
}

pub(super) fn split_top_level_source_binary_str<'a>(
    expr: &'a str,
    op: &str,
) -> Option<(&'a str, &'a str)> {
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;
    let mut split_at = None;

    for (idx, ch) in expr.char_indices() {
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
            _ if paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && idx > 0
                && expr[idx..].starts_with(op) =>
            {
                split_at = Some(idx);
            }
            _ => {}
        }
        prev_was_escape = false;
    }

    let idx = split_at?;
    let left = expr[..idx].trim();
    let right = expr[idx + op.len()..].trim();
    (!left.is_empty() && !right.is_empty()).then_some((left, right))
}

pub(super) fn split_top_level_source_binary_any<'a>(
    expr: &'a str,
    ops: &[char],
) -> Option<(&'a str, char, &'a str)> {
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;
    let mut split_at = None;

    for (idx, ch) in expr.char_indices() {
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
            _ if ops.contains(&ch)
                && paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && source_binary_char_has_left_operand(expr, idx) =>
            {
                split_at = Some((idx, ch));
            }
            _ => {}
        }
        prev_was_escape = false;
    }

    let (idx, op) = split_at?;
    let left = expr[..idx].trim();
    let right = expr[idx + op.len_utf8()..].trim();
    (!left.is_empty() && !right.is_empty()).then_some((left, op, right))
}

pub(super) fn source_binary_char_has_left_operand(expr: &str, idx: usize) -> bool {
    expr[..idx]
        .chars()
        .rev()
        .find(|ch| !ch.is_whitespace())
        .is_some_and(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | ')' | '}' | ']' | '"'))
}

pub(super) fn split_top_level_source_binary(expr: &str, op: char) -> Option<(&str, &str)> {
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;
    let mut split_at = None;

    for (idx, ch) in expr.char_indices() {
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
            _ if ch == op
                && paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && idx > 0 =>
            {
                split_at = Some(idx);
            }
            _ => {}
        }
        prev_was_escape = false;
    }

    let idx = split_at?;
    let left = expr[..idx].trim();
    let right = expr[idx + op.len_utf8()..].trim();
    (!left.is_empty() && !right.is_empty()).then_some((left, right))
}

pub(super) fn matching_brace(s: &str, open_idx: usize) -> Option<usize> {
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
            '{' => depth += 1,
            '}' => {
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
