use super::*;

pub(super) fn lower_match_expr(rest: &str, line_num: usize) -> Result<String, CliError> {
    let open = rest.find('{').ok_or_else(|| {
        source_lower_error(
            line_num,
            SourceLowerDiagnostic::MatchExpression,
            "match expression requires `match value { Pattern => expr, ... }`",
        )
    })?;
    let scrutinee = rest[..open].trim();
    if scrutinee.is_empty() {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::MatchExpression,
            "match expression requires a scrutinee",
        ));
    }
    let close = matching_brace(rest, open).ok_or_else(|| {
        source_lower_error(
            line_num,
            SourceLowerDiagnostic::MatchExpression,
            "match expression has unclosed arm block",
        )
    })?;
    if !rest[close + 1..].trim().is_empty() {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::MatchExpression,
            "unexpected tokens after match expression",
        ));
    }
    let raw_arms = &rest[open + 1..close];
    let arms_line_num = line_num
        + raw_arms[..raw_arms.len() - raw_arms.trim_start().len()]
            .matches('\n')
            .count();
    let arms = raw_arms.trim();
    if arms.is_empty() {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::MatchExpression,
            "match expression requires at least one arm",
        ));
    }

    let mut lowered = vec![lower_source_expr(scrutinee, line_num)?];
    let mut arm_search_start = 0usize;
    for arm in split_source_match_arms(arms) {
        let arm_start = arms[arm_search_start..]
            .find(&arm)
            .map(|idx| arm_search_start + idx)
            .unwrap_or(arm_search_start);
        let (pattern, body) = split_source_match_arm(&arm, line_num)?;
        let body_line_num = source_match_arm_body_line(arms, arm_start, &arm, arms_line_num);
        let pattern = normalize_source_match_pattern(pattern, line_num)?;
        lowered.push(pattern);
        lowered.push(lower_source_match_arm_body_expr(body, body_line_num)?);
        arm_search_start = arm_start + arm.len();
    }
    Ok(format!("match({})", lowered.join(", ")))
}

pub(super) fn split_source_match_arms(arms: &str) -> Vec<String> {
    let comma_split = split_source_args(arms);
    if comma_split.len() != 1 {
        return comma_split;
    }
    split_braced_source_match_arms(arms).unwrap_or(comma_split)
}

fn split_braced_source_match_arms(arms: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut start = 0usize;
    while start < arms.len() {
        let rest = arms[start..].trim_start();
        if rest.is_empty() {
            break;
        }
        start += arms[start..].len() - rest.len();
        let arrow = find_top_level_source_arrow(&arms[start..])? + start;
        let body_start = arms[arrow + 2..]
            .char_indices()
            .find(|(_, ch)| !ch.is_whitespace())
            .map(|(idx, _)| arrow + 2 + idx)
            .unwrap_or(arms.len());
        if !arms[body_start..].starts_with('{') {
            return None;
        }
        let body_end = matching_brace(arms, body_start)? + 1;
        out.push(
            arms[start..body_end]
                .trim()
                .trim_end_matches(',')
                .trim()
                .to_string(),
        );
        start = body_end;
        while let Some((idx, ch)) = arms[start..].char_indices().next() {
            if ch.is_whitespace() || ch == ',' {
                start += idx + ch.len_utf8();
            } else {
                break;
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

pub(super) fn split_source_match_arm<'a>(
    arm: &'a str,
    line_num: usize,
) -> Result<(&'a str, &'a str), CliError> {
    let Some(idx) = find_top_level_source_arrow(arm) else {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::MatchExpression,
            "match arm requires `Pattern => expression`",
        ));
    };
    let pattern = arm[..idx].trim();
    let body = arm[idx + 2..].trim();
    if pattern.is_empty() || body.is_empty() {
        return Err(source_lower_error(
            line_num,
            SourceLowerDiagnostic::MatchExpression,
            "match arm pattern and body must be non-empty",
        ));
    }
    Ok((pattern, body))
}

fn lower_source_match_arm_body_expr(body: &str, line_num: usize) -> Result<String, CliError> {
    let body = body.trim();
    if let Some(inner) = source_match_arm_braced_expr(body) {
        return lower_source_statement_block_expr(
            inner,
            line_num,
            SourceLowerDiagnostic::MatchExpression,
            "match arm body must be a non-empty expression",
        );
    }
    // A bare `return expression` arm is sugar for the expression itself; the match
    // arm is already a tail position, so the return marker carries no extra meaning.
    let body = body.strip_prefix("return ").map(str::trim).unwrap_or(body);
    lower_source_expr(body, line_num)
}

fn source_match_arm_body_line(arms: &str, arm_start: usize, arm: &str, line_num: usize) -> usize {
    let Some(arrow) = find_top_level_source_arrow(arm) else {
        return line_num;
    };
    let body_start = arm_start + arrow + 2;
    line_num + arms[..body_start].chars().filter(|ch| *ch == '\n').count()
}

fn source_match_arm_braced_expr(body: &str) -> Option<&str> {
    let body = body.trim();
    if !body.starts_with('{') || matching_brace(body, 0)? != body.len() - 1 {
        return None;
    }
    let inner = &body[1..body.len() - 1];
    let trimmed = inner.trim();
    if trimmed.starts_with("return ") || find_top_level_source_colon(trimmed).is_none() {
        Some(inner)
    } else {
        None
    }
}

pub(super) fn find_top_level_source_arrow(input: &str) -> Option<usize> {
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut in_string = false;
    let mut prev_was_escape = false;

    for (idx, ch) in input.char_indices() {
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
            '=' if paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && input[idx..].starts_with("=>") =>
            {
                return Some(idx);
            }
            _ => {}
        }
        prev_was_escape = false;
    }
    None
}
