use super::*;

// ── Parsing helpers ───────────────────────────────────────────────────────

pub(super) fn parse_package_spec(spec: &str) -> (&str, &str) {
    spec.split_once('@').unwrap_or((spec, "latest"))
}

pub(super) fn parse_advisory_severity(
    raw: &str,
) -> Result<ail_package::AdvisorySeverity, CliError> {
    match raw.to_ascii_lowercase().as_str() {
        "low" => Ok(ail_package::AdvisorySeverity::Low),
        "medium" => Ok(ail_package::AdvisorySeverity::Medium),
        "high" => Ok(ail_package::AdvisorySeverity::High),
        "critical" => Ok(ail_package::AdvisorySeverity::Critical),
        other => Err(CliError::ParseError(format!(
            "unsupported advisory severity: {other}; expected low, medium, high, or critical"
        ))),
    }
}

pub(super) fn validate_required_package_metadata_field(
    field: &str,
    value: String,
) -> Result<String, CliError> {
    if value.trim().is_empty() {
        return Err(CliError::ParseError(format!(
            "package metadata {field} must not be empty"
        )));
    }
    Ok(value)
}
