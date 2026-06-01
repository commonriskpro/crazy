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

pub(super) fn validate_optional_reproducible_evidence(
    source_digest: Option<String>,
    toolchain_id: Option<String>,
    recipe_hash: Option<String>,
) -> Result<Option<ReproducibleBuildEvidence>, CliError> {
    let provided = [
        source_digest.is_some(),
        toolchain_id.is_some(),
        recipe_hash.is_some(),
    ]
    .iter()
    .filter(|provided| **provided)
    .count();
    if provided == 0 {
        return Ok(None);
    }
    if provided != 3 {
        return Err(CliError::ParseError(
            "reproducible evidence requires --source-digest, --toolchain-id, and --recipe-hash"
                .to_string(),
        ));
    }

    let source_digest =
        validate_package_blake3_hex_field("source_digest", source_digest.expect("checked"))?;
    let toolchain_id =
        validate_required_package_metadata_field("toolchain_id", toolchain_id.expect("checked"))?;
    let recipe_hash =
        validate_package_blake3_hex_field("recipe_hash", recipe_hash.expect("checked"))?;
    Ok(Some(ReproducibleBuildEvidence::new(
        source_digest,
        toolchain_id,
        recipe_hash,
    )))
}

fn validate_package_blake3_hex_field(field: &str, value: String) -> Result<String, CliError> {
    let value = validate_required_package_metadata_field(field, value)?;
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(CliError::ParseError(format!(
            "package metadata {field} must be a 64-character lowercase BLAKE3 hex digest"
        )));
    }
    Ok(value)
}
