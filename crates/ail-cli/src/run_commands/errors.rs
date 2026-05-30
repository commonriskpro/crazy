use super::*;

pub(super) fn format_run_preflight_error(err: &RuntimeError, module_name: &str) -> String {
    match err {
        RuntimeError::PreflightFailed(PreflightFailure::CapabilityDenied { denied }) => {
            format_missing_capability_grants(denied, module_name)
        }
        _ => err.to_string(),
    }
}

pub(super) fn format_missing_capability_grants(
    denied: &[CapabilityId],
    module_name: &str,
) -> String {
    let names: Vec<&str> = denied.iter().map(CapabilityId::as_str).collect();
    let capability_label = if names.len() == 1 {
        "capability"
    } else {
        "capabilities"
    };
    let pronoun = if names.len() == 1 { "it" } else { "they" };
    let verb = if names.len() == 1 { "was" } else { "were" };
    let formatted_names = names
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ");

    let mut message = format!(
        "function `{module_name}` requires {capability_label} {formatted_names}, \
         but {pronoun} {verb} not supplied via --grant"
    );

    if names.iter().all(|name| is_safe_cli_word(name)) {
        let grants = names
            .iter()
            .map(|name| format!("--grant {name}"))
            .collect::<Vec<_>>()
            .join(" ");
        message.push_str(&format!("; suggestion: add `{grants}`"));
    }

    message
}

pub(super) fn is_safe_cli_word(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-' | b'/'))
}
