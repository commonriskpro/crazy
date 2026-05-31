use super::*;

pub(super) const RUN_CAPABILITY_GRANT_DENIED_CODE: &str = "AIL_RUN_CAPABILITY_GRANT_DENIED";
pub(super) const RUN_CAPABILITY_GRANT_DENIED_KEY: &str = "run.capability_grant_denied";

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
    let redacted_count = names
        .iter()
        .filter(|name| capability_display_kind(name).is_redacted())
        .count();
    let capability_label = if names.len() == 1 {
        "capability"
    } else {
        "capabilities"
    };
    let pronoun = if names.len() == 1 { "it" } else { "they" };
    let verb = if names.len() == 1 { "was" } else { "were" };
    let formatted_names = names
        .iter()
        .map(|name| capability_display_label(name))
        .collect::<Vec<_>>()
        .join(", ");

    let mut message = format!(
        "{RUN_CAPABILITY_GRANT_DENIED_CODE}: {RUN_CAPABILITY_GRANT_DENIED_KEY}: \
         function `{module_name}` requires {capability_label} {formatted_names}, \
         but {pronoun} {verb} not supplied via --grant; denied_count={}",
        names.len()
    );

    if redacted_count > 0 {
        message.push_str(&format!("; redacted_capabilities={redacted_count}"));
    }

    if names
        .iter()
        .all(|name| matches!(capability_display_kind(name), CapabilityDisplayKind::Plain))
    {
        let grants = names
            .iter()
            .map(|name| format!("--grant {name}"))
            .collect::<Vec<_>>()
            .join(" ");
        message.push_str(&format!("; suggestion: add `{grants}`"));
    }

    message
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CapabilityDisplayKind {
    Plain,
    Secret,
    UnsafeCliWord,
}

impl CapabilityDisplayKind {
    fn is_redacted(self) -> bool {
        !matches!(self, CapabilityDisplayKind::Plain)
    }
}

fn capability_display_kind(value: &str) -> CapabilityDisplayKind {
    if is_secret_capability(value) {
        CapabilityDisplayKind::Secret
    } else if is_safe_cli_word(value) {
        CapabilityDisplayKind::Plain
    } else {
        CapabilityDisplayKind::UnsafeCliWord
    }
}

fn capability_display_label(value: &str) -> String {
    match capability_display_kind(value) {
        CapabilityDisplayKind::Plain => format!("`{value}`"),
        CapabilityDisplayKind::Secret => "`<redacted:secret-capability>`".to_string(),
        CapabilityDisplayKind::UnsafeCliWord => "`<redacted:unsafe-capability-id>`".to_string(),
    }
}

fn is_secret_capability(value: &str) -> bool {
    matches!(
        value.split_once(':').map_or(value, |(prefix, _)| prefix),
        "secret.read" | "secret.write" | "secret.delete" | "secret"
    )
}

pub(super) fn is_safe_cli_word(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-' | b'/'))
}
