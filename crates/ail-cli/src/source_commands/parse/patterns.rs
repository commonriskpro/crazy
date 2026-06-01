use super::*;

pub(super) fn validate_source_match_pattern(pattern: &str) -> Result<(), CliError> {
    let pattern = pattern.trim();
    if pattern == "_" || pattern == "None" || pattern == "true" || pattern == "false" {
        return Ok(());
    }
    if pattern.parse::<i64>().is_ok() {
        return Ok(());
    }
    if let Some((tag, binding)) = source_constructor_pattern(pattern) {
        validate_source_constructor_tag(tag)?;
        if let Some(binding) = binding
            && binding != "_"
        {
            validate_source_constructor_pattern_binding_shape(pattern, binding)?;
            validate_source_user_local_expr_name(binding)?;
        }
        return Ok(());
    }
    if let Some(message) = unsupported_source_constructor_pattern_message(pattern)? {
        return Err(source_parse_error_unlocated(
            SourceParseDiagnostic::InvalidPattern,
            pattern,
            message,
        ));
    }
    Err(source_parse_error_unlocated(
        SourceParseDiagnostic::InvalidPattern,
        pattern,
        format!("unsupported source match pattern `{pattern}`"),
    ))
}

fn validate_source_constructor_pattern_binding_shape(
    pattern: &str,
    binding: &str,
) -> Result<(), CliError> {
    if let Some(message) = unsupported_source_constructor_binding_message(pattern, binding) {
        return Err(source_parse_error_unlocated(
            SourceParseDiagnostic::InvalidPattern,
            pattern,
            message,
        ));
    }
    Ok(())
}

fn unsupported_source_constructor_binding_message(pattern: &str, binding: &str) -> Option<String> {
    if binding.is_empty() {
        return Some(format!(
            "unsupported empty source match pattern `{pattern}`: constructor arms require a single local binding or `_`"
        ));
    }
    if binding.starts_with('{') || binding.contains(':') {
        return Some(format!(
            "unsupported source record-field match pattern `{pattern}`: constructor arms currently support only a single local binding or `_`; bind the value and inspect fields in the arm body"
        ));
    }
    if binding.starts_with('[') {
        return Some(format!(
            "unsupported source list match pattern `{pattern}`: constructor arms currently support only a single local binding or `_`; bind the value and inspect elements in the arm body"
        ));
    }
    None
}

fn unsupported_source_constructor_pattern_message(
    pattern: &str,
) -> Result<Option<String>, CliError> {
    let Some((tag, inner)) = unsupported_source_constructor_pattern_parts(pattern)? else {
        return Ok(None);
    };
    validate_source_constructor_tag(tag)?;

    let fields = split_source_args(inner);
    let binding = fields.first().map(String::as_str).unwrap_or(inner).trim();
    if let Some(message) = unsupported_source_constructor_binding_message(pattern, binding) {
        return Ok(Some(message));
    }
    if fields.len() > 1 || inner.ends_with(',') {
        return Ok(Some(format!(
            "unsupported multi-binding source match pattern `{pattern}`: constructor arms currently support exactly one local binding or `_`"
        )));
    }

    if binding.contains('(') || binding.contains(')') {
        return Ok(Some(format!(
            "unsupported nested source match pattern `{pattern}`: constructor arms currently support only a single local binding or `_`; bind the value and match again inside the arm"
        )));
    }

    Ok(None)
}

fn unsupported_source_constructor_pattern_parts(
    pattern: &str,
) -> Result<Option<(&str, &str)>, CliError> {
    let trimmed = pattern.trim();
    let Some(open) = trimmed.find('(') else {
        return Ok(None);
    };
    let tag = trimmed[..open].trim();
    if !tag.chars().next().is_some_and(|ch| ch.is_ascii_uppercase()) {
        return Ok(None);
    }
    let Some(close) = matching_paren(trimmed, open) else {
        return Err(source_parse_error_unlocated(
            SourceParseDiagnostic::MissingDelimiter,
            pattern,
            format!(
                "unsupported source match pattern `{pattern}`: constructor pattern has unclosed `)`"
            ),
        ));
    };
    if close != trimmed.len() - 1 {
        return Err(source_parse_error_unlocated(
            SourceParseDiagnostic::UnexpectedToken,
            pattern,
            format!(
                "unsupported source match pattern `{pattern}`: unexpected tokens after constructor pattern"
            ),
        ));
    }
    Ok(Some((tag, trimmed[open + 1..close].trim())))
}

pub(super) fn validate_source_constructor_tag(tag: &str) -> Result<(), CliError> {
    if matches!(tag, "Some" | "None" | "Ok" | "Err") {
        return Ok(());
    }
    Err(source_parse_error_unlocated(
        SourceParseDiagnostic::InvalidPattern,
        tag,
        format!("unsupported source match constructor `{tag}`"),
    ))
}

pub(super) fn source_match_pattern_binding(pattern: &str) -> Option<&str> {
    let (_, binding) = source_constructor_pattern(pattern)?;
    let binding = binding?;
    (binding != "_").then_some(binding)
}

pub(super) fn source_match_pattern_binding_type<'a>(
    pattern: &'a str,
    scrutinee_ty: &str,
) -> Result<Option<(&'a str, String)>, CliError> {
    let Some(binding) = source_match_pattern_binding(pattern) else {
        return Ok(None);
    };
    let Some((tag, _)) = source_constructor_pattern(pattern) else {
        return Ok(None);
    };
    match tag {
        "Some" => source_option_element_type(scrutinee_ty)
            .map(|ty| Some((binding, ty.to_string())))
            .ok_or_else(|| {
                source_match_pattern_type_error(pattern, "Option<Unknown>", scrutinee_ty)
            }),
        "Ok" => source_result_types(scrutinee_ty)
            .map(|(ok_ty, _)| Some((binding, ok_ty.to_string())))
            .ok_or_else(|| {
                source_match_pattern_type_error(pattern, "Result<Unknown,Unknown>", scrutinee_ty)
            }),
        "Err" => source_result_types(scrutinee_ty)
            .map(|(_, err_ty)| Some((binding, err_ty.to_string())))
            .ok_or_else(|| {
                source_match_pattern_type_error(pattern, "Result<Unknown,Unknown>", scrutinee_ty)
            }),
        _ => Ok(None),
    }
}

pub(super) fn validate_source_match_pattern_type(
    pattern: &str,
    scrutinee_ty: &str,
) -> Result<(), CliError> {
    let Some((tag, _)) = source_constructor_pattern(pattern) else {
        return Ok(());
    };
    match tag {
        "Some" | "None" if source_option_element_type(scrutinee_ty).is_some() => Ok(()),
        "Some" | "None" => Err(source_match_pattern_type_error(
            pattern,
            "Option<Unknown>",
            scrutinee_ty,
        )),
        "Ok" | "Err" if source_result_types(scrutinee_ty).is_some() => Ok(()),
        "Ok" | "Err" => Err(source_match_pattern_type_error(
            pattern,
            "Result<Unknown,Unknown>",
            scrutinee_ty,
        )),
        _ => Ok(()),
    }
}

pub(super) fn validate_source_match_reachable(arms: &[String]) -> Result<(), CliError> {
    let mut seen = BTreeSet::new();
    let mut saw_wildcard = false;

    for pattern in arms.iter().step_by(2).map(String::as_str) {
        let normalized = source_match_pattern_key(pattern);
        if saw_wildcard {
            return Err(source_match_unreachable_error(format!(
                "unreachable match arm `{pattern}` after wildcard `_`"
            )));
        }
        if normalized == "_" {
            saw_wildcard = true;
            continue;
        }
        if !seen.insert(normalized.clone()) {
            return Err(source_match_unreachable_error(format!(
                "duplicate match arm pattern `{normalized}`"
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_source_match_exhaustive(
    arms: &[String],
    scrutinee_ty: &str,
) -> Result<(), CliError> {
    let patterns = arms.iter().step_by(2).map(String::as_str);
    if patterns.clone().any(|pattern| pattern.trim() == "_") {
        return Ok(());
    }

    if source_option_element_type(scrutinee_ty).is_some() {
        let has_some = patterns
            .clone()
            .any(|pattern| source_pattern_tag(pattern) == Some("Some"));
        let has_none = patterns
            .clone()
            .any(|pattern| source_pattern_tag(pattern) == Some("None"));
        if has_some && has_none {
            return Ok(());
        }
        return Err(source_match_non_exhaustive_error(format!(
            "non-exhaustive match for {scrutinee_ty}: expected Some and None arms or `_`"
        )));
    }

    if source_result_types(scrutinee_ty).is_some() {
        let has_ok = patterns
            .clone()
            .any(|pattern| source_pattern_tag(pattern) == Some("Ok"));
        let has_err = patterns
            .clone()
            .any(|pattern| source_pattern_tag(pattern) == Some("Err"));
        if has_ok && has_err {
            return Ok(());
        }
        return Err(source_match_non_exhaustive_error(format!(
            "non-exhaustive match for {scrutinee_ty}: expected Ok and Err arms or `_`"
        )));
    }

    if scrutinee_ty == "Bool" {
        let has_true = patterns.clone().any(|pattern| pattern.trim() == "true");
        let has_false = patterns.clone().any(|pattern| pattern.trim() == "false");
        if has_true && has_false {
            return Ok(());
        }
        return Err(source_match_non_exhaustive_error(
            "non-exhaustive match for Bool: expected true and false arms or `_`",
        ));
    }

    Ok(())
}

fn source_match_unreachable_error(message: impl AsRef<str>) -> CliError {
    source_match_error(
        "AIL_SOURCE_MATCH_UNREACHABLE",
        "source.match.reachability",
        message,
    )
}

fn source_match_non_exhaustive_error(message: impl AsRef<str>) -> CliError {
    source_match_error(
        "AIL_SOURCE_MATCH_NON_EXHAUSTIVE",
        "source.match.exhaustiveness",
        message,
    )
}

fn source_match_pattern_type_error(pattern: &str, expected: &str, actual: &str) -> CliError {
    source_match_error(
        "AIL_SOURCE_MATCH_PATTERN_TYPE",
        "source.match.pattern",
        format!("type mismatch in match pattern `{pattern}`: expected {expected}, got {actual}"),
    )
}

fn source_match_error(code: &str, category: &str, message: impl AsRef<str>) -> CliError {
    CliError::ParseError(format!("{} [{code}] category={category}", message.as_ref()))
}

pub(super) fn source_match_pattern_key(pattern: &str) -> String {
    source_pattern_tag(pattern)
        .map(ToString::to_string)
        .unwrap_or_else(|| pattern.trim().to_string())
}

pub(super) fn source_pattern_tag(pattern: &str) -> Option<&str> {
    if pattern.trim() == "None" {
        return Some("None");
    }
    source_constructor_pattern(pattern).map(|(tag, _)| tag)
}

pub(super) fn source_constructor_pattern(pattern: &str) -> Option<(&str, Option<&str>)> {
    let trimmed = pattern.trim();
    let first = trimmed.chars().next()?;
    if !first.is_ascii_uppercase() {
        return None;
    }
    if let Some(open) = trimmed.find('(') {
        let tag = trimmed[..open].trim();
        let binding = trimmed[open + 1..].trim().strip_suffix(')')?.trim();
        if binding.contains('(') || binding.contains(',') || binding.is_empty() {
            return None;
        }
        Some((tag, Some(binding)))
    } else {
        Some((trimmed, None))
    }
}

pub(super) fn normalize_source_match_pattern(
    pattern: &str,
    line_num: usize,
) -> Result<String, CliError> {
    let pattern = pattern.trim();
    let normalized = if pattern == "none" || pattern == "none()" {
        "None".to_string()
    } else if let Some(inner) = pattern
        .strip_prefix("some(")
        .and_then(|rest| rest.strip_suffix(')'))
    {
        format!("Some({})", inner.trim())
    } else if let Some(inner) = pattern
        .strip_prefix("ok(")
        .and_then(|rest| rest.strip_suffix(')'))
    {
        format!("Ok({})", inner.trim())
    } else if let Some(inner) = pattern
        .strip_prefix("err(")
        .and_then(|rest| rest.strip_suffix(')'))
    {
        format!("Err({})", inner.trim())
    } else {
        pattern.to_string()
    };
    validate_source_match_pattern(&normalized)
        .map_err(|err| source_pattern_error_at_line(err, line_num, &normalized))?;
    Ok(normalized)
}

fn source_pattern_error_at_line(err: CliError, line_num: usize, pattern: &str) -> CliError {
    let width = pattern.chars().count().max(1);
    match err {
        CliError::ParseError(message) if message.contains("span=<unknown>") => {
            let message = message.replace(
                "span=<unknown>",
                &format!("span={line_num}:1..{line_num}:{}", 1 + width),
            );
            CliError::ParseError(format!("line {line_num}: {message}"))
        }
        other => source_error_at_line(other, line_num),
    }
}
