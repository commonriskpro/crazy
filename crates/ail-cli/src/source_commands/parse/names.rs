use super::*;

pub(super) fn split_source_top_level_commas(input: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut angle_depth = 0usize;

    for (idx, ch) in input.char_indices() {
        match ch {
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            ',' if angle_depth == 0 => {
                out.push(input[start..idx].trim());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    out.push(input[start..].trim());
    out.into_iter().filter(|part| !part.is_empty()).collect()
}

pub(super) fn normalize_grant_target(target: &str) -> String {
    target.to_string()
}

pub(super) fn validate_source_local_name(name: &str, line_num: usize) -> Result<(), CliError> {
    validate_source_local_name_at(name, line_num, 1)
}

pub(super) fn validate_source_local_name_at(
    name: &str,
    line_num: usize,
    column: usize,
) -> Result<(), CliError> {
    validate_source_name_at(name, line_num, column)?;
    if name.contains('.') {
        return Err(source_parse_error_for_fragment_at(
            line_num,
            column,
            SourceParseDiagnostic::InvalidName,
            name,
            format!("local binding name `{name}` must not contain `.`"),
        ));
    }
    Ok(())
}

pub(super) fn validate_source_user_local_name_at(
    name: &str,
    line_num: usize,
    column: usize,
) -> Result<(), CliError> {
    validate_source_local_name_at(name, line_num, column)?;
    if is_source_statement_binding_name(name) {
        return Err(source_parse_error_for_fragment_at(
            line_num,
            column,
            SourceParseDiagnostic::InvalidName,
            name,
            format!(
                "local binding name `{name}` uses reserved compiler-generated prefix `{SOURCE_STATEMENT_BINDING_PREFIX}`"
            ),
        ));
    }
    Ok(())
}

pub(super) fn validate_source_user_local_expr_name(name: &str) -> Result<(), CliError> {
    validate_source_local_expr_name(name)?;
    if is_source_statement_binding_name(name) {
        return Err(source_parse_error_unlocated(
            SourceParseDiagnostic::InvalidName,
            name,
            format!(
                "local binding name `{name}` uses reserved compiler-generated prefix `{SOURCE_STATEMENT_BINDING_PREFIX}`"
            ),
        ));
    }
    Ok(())
}

pub(super) fn validate_source_name(name: &str, line_num: usize) -> Result<(), CliError> {
    validate_source_name_at(name, line_num, 1)
}

pub(super) fn validate_source_name_at(
    name: &str,
    line_num: usize,
    column: usize,
) -> Result<(), CliError> {
    if name.is_empty() {
        return Err(source_parse_error_for_fragment_at(
            line_num,
            column,
            SourceParseDiagnostic::InvalidName,
            name,
            "declaration name cannot be empty",
        ));
    }
    if !is_valid_source_name_chars(name) {
        return Err(source_parse_error_for_fragment_at(
            line_num,
            column,
            SourceParseDiagnostic::InvalidName,
            name,
            format!("declaration name `{name}` contains unsupported characters"),
        ));
    }
    if name.split('.').any(str::is_empty) {
        return Err(source_parse_error_for_fragment_at(
            line_num,
            column,
            SourceParseDiagnostic::InvalidName,
            name,
            format!("declaration name `{name}` contains an empty path segment"),
        ));
    }
    if let Some(segment) = first_invalid_source_name_segment(name) {
        return Err(source_parse_error_for_fragment_at(
            line_num,
            column,
            SourceParseDiagnostic::InvalidName,
            name,
            format!(
                "declaration name `{name}` segment `{segment}` must start with a letter or `_`"
            ),
        ));
    }
    Ok(())
}

pub(super) fn is_valid_source_name_chars(name: &str) -> bool {
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

pub(super) fn source_name_segments_are_valid(name: &str) -> bool {
    !name.split('.').any(str::is_empty) && first_invalid_source_name_segment(name).is_none()
}

pub(super) fn first_invalid_source_name_segment(name: &str) -> Option<&str> {
    name.split('.').find(|segment| {
        !segment
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
    })
}

pub(super) fn normalize_function_name(name: &str) -> String {
    if name.starts_with("fn.") {
        name.to_string()
    } else {
        format!("fn.{name}")
    }
}

pub(super) fn normalize_test_name(name: &str) -> String {
    if name.starts_with("test.") {
        name.to_string()
    } else {
        format!("test.{name}")
    }
}

pub(super) fn source_default_entry(program: &SourceProgram) -> String {
    program
        .module
        .as_deref()
        .map(|module| format!("fn.{module}.main"))
        .unwrap_or_else(|| "fn.main".to_string())
}

pub(super) fn source_change_name(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("source");
    let sanitized = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("source_{sanitized}")
}
