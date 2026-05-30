// ── Export naming ─────────────────────────────────────────────────────────

pub fn export_name(binding_name: &str) -> String {
    let logical = binding_name
        .strip_prefix("fn.")
        .or_else(|| binding_name.strip_prefix("test."))
        .unwrap_or(binding_name);
    logical
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}
