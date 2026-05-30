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
            validate_source_local_expr_name(binding)?;
        }
        return Ok(());
    }
    Err(CliError::ParseError(format!(
        "unsupported source match pattern `{pattern}`"
    )))
}

pub(super) fn validate_source_constructor_tag(tag: &str) -> Result<(), CliError> {
    if matches!(tag, "Some" | "None" | "Ok" | "Err") {
        return Ok(());
    }
    Err(CliError::ParseError(format!(
        "unsupported source match constructor `{tag}`"
    )))
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
                CliError::ParseError(format!(
                    "type mismatch in match pattern `{pattern}`: expected Option<Unknown>, got {scrutinee_ty}"
                ))
            }),
        "Ok" => source_result_types(scrutinee_ty)
            .map(|(ok_ty, _)| Some((binding, ok_ty.to_string())))
            .ok_or_else(|| {
                CliError::ParseError(format!(
                    "type mismatch in match pattern `{pattern}`: expected Result<Unknown,Unknown>, got {scrutinee_ty}"
                ))
            }),
        "Err" => source_result_types(scrutinee_ty)
            .map(|(_, err_ty)| Some((binding, err_ty.to_string())))
            .ok_or_else(|| {
                CliError::ParseError(format!(
                    "type mismatch in match pattern `{pattern}`: expected Result<Unknown,Unknown>, got {scrutinee_ty}"
                ))
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
        "Some" | "None" => Err(CliError::ParseError(format!(
            "type mismatch in match pattern `{pattern}`: expected Option<Unknown>, got {scrutinee_ty}"
        ))),
        "Ok" | "Err" if source_result_types(scrutinee_ty).is_some() => Ok(()),
        "Ok" | "Err" => Err(CliError::ParseError(format!(
            "type mismatch in match pattern `{pattern}`: expected Result<Unknown,Unknown>, got {scrutinee_ty}"
        ))),
        _ => Ok(()),
    }
}

pub(super) fn validate_source_match_reachable(arms: &[String]) -> Result<(), CliError> {
    let mut seen = BTreeSet::new();
    let mut saw_wildcard = false;

    for pattern in arms.iter().step_by(2).map(String::as_str) {
        let normalized = source_match_pattern_key(pattern);
        if saw_wildcard {
            return Err(CliError::ParseError(format!(
                "unreachable match arm `{pattern}` after wildcard `_`"
            )));
        }
        if normalized == "_" {
            saw_wildcard = true;
            continue;
        }
        if !seen.insert(normalized.clone()) {
            return Err(CliError::ParseError(format!(
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
        return Err(CliError::ParseError(format!(
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
        return Err(CliError::ParseError(format!(
            "non-exhaustive match for {scrutinee_ty}: expected Ok and Err arms or `_`"
        )));
    }

    if scrutinee_ty == "Bool" {
        let has_true = patterns.clone().any(|pattern| pattern.trim() == "true");
        let has_false = patterns.clone().any(|pattern| pattern.trim() == "false");
        if has_true && has_false {
            return Ok(());
        }
        return Err(CliError::ParseError(
            "non-exhaustive match for Bool: expected true and false arms or `_`".to_string(),
        ));
    }

    Ok(())
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
        .map_err(|err| source_error_at_line(err, line_num))?;
    Ok(normalized)
}
