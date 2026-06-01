use std::path::PathBuf;

pub(super) fn file_path_from_uri(uri: &str) -> Option<PathBuf> {
    uri.strip_prefix("file://").map(PathBuf::from)
}

pub(super) fn is_ail_source_uri(uri: &str) -> bool {
    uri.trim_end().ends_with(".ail")
}

pub(super) fn language_for_uri(uri: &str) -> &'static str {
    if is_ail_source_uri(uri) {
        "ail-source"
    } else {
        "acl"
    }
}

pub(super) fn source_module_from_text(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix("module ")
            .map(str::trim)
            .filter(|module| !module.is_empty())
            .map(ToString::to_string)
    })
}

pub(super) fn source_imports_from_text(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let rest = trimmed.strip_prefix("use ")?;
            rest.trim()
                .strip_prefix('"')
                .and_then(|value| value.split_once('"').map(|(import, _)| import.to_string()))
        })
        .collect()
}

pub(super) fn source_test_name_end(rest: &str) -> Option<usize> {
    [rest.find("->"), rest.find('='), rest.find('{')]
        .into_iter()
        .flatten()
        .min()
}

pub(super) fn resolve_lsp_source_import(source_path: &std::path::Path, import: &str) -> PathBuf {
    let import_path = std::path::Path::new(import);
    if import_path.is_absolute() {
        import_path.to_path_buf()
    } else {
        source_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(import_path)
    }
}

pub(super) fn is_acl_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.')
}
