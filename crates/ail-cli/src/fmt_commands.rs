// ── ail-cli::fmt_commands ─────────────────────────────────────────────────
//
// Validation-stage formatter for `ail fmt`.
//
// It formats both current AI Change Language (ACL) ChangeSet files and the
// validation-stage `.ail` source surface. ACL formatting is semantic through
// ail-change canonicalization; `.ail` formatting parses the source frontend and
// emits stable source text for the supported syntax.

use std::path::{Path, PathBuf};

use ail_change::canonical::{Precondition, canonicalize_parsed};
use ail_change::model::BlockHash;
use ail_change::parser::parse_changeset;
use serde_json::{Map, Value, json};

use crate::error::CliError;
use crate::output::{OutputMode, print_error_response, print_response};
use crate::source_commands::format_ail_source;

// ── FmtOutcome ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FmtOutcome {
    pub formatted: String,
    pub changed: bool,
    pub op_count: usize,
    pub item_count: usize,
    pub language: &'static str,
}

// ── formatter diagnostics ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FmtLanguage {
    Acl,
    AilSource,
    Unknown,
}

impl FmtLanguage {
    fn as_str(self) -> &'static str {
        match self {
            FmtLanguage::Acl => "acl",
            FmtLanguage::AilSource => "ail-source",
            FmtLanguage::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FmtDiagnosticKind {
    ParseFailure,
    UnsupportedSyntax,
    NonIdempotent,
    WriteCheckModeMismatch,
    NotCanonical,
}

impl FmtDiagnosticKind {
    fn code(self) -> &'static str {
        match self {
            FmtDiagnosticKind::ParseFailure => "FMT_PARSE_FAILED",
            FmtDiagnosticKind::UnsupportedSyntax => "FMT_UNSUPPORTED_SYNTAX",
            FmtDiagnosticKind::NonIdempotent => "FMT_NON_IDEMPOTENT",
            FmtDiagnosticKind::WriteCheckModeMismatch => "FMT_WRITE_CHECK_MODE_MISMATCH",
            FmtDiagnosticKind::NotCanonical => "FMT_NOT_CANONICAL",
        }
    }

    fn category(self) -> &'static str {
        match self {
            FmtDiagnosticKind::ParseFailure => "parse",
            FmtDiagnosticKind::UnsupportedSyntax => "unsupported",
            FmtDiagnosticKind::NonIdempotent => "stability",
            FmtDiagnosticKind::WriteCheckModeMismatch => "usage",
            FmtDiagnosticKind::NotCanonical => "check",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FmtDiagnosticDescriptor {
    input: &'static str,
    extension: &'static str,
    language: &'static str,
    mode: &'static str,
    changed: Option<bool>,
}

impl FmtDiagnosticDescriptor {
    fn to_json(&self) -> Value {
        let mut obj = Map::new();
        obj.insert("input".to_string(), json!(self.input));
        obj.insert("extension".to_string(), json!(self.extension));
        obj.insert("language".to_string(), json!(self.language));
        obj.insert("mode".to_string(), json!(self.mode));
        if let Some(changed) = self.changed {
            obj.insert("changed".to_string(), json!(changed));
        }
        Value::Object(obj)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FmtDiagnostic {
    kind: FmtDiagnosticKind,
    message: String,
    descriptor: FmtDiagnosticDescriptor,
    details: Value,
}

impl FmtDiagnostic {
    fn new(
        kind: FmtDiagnosticKind,
        message: impl Into<String>,
        descriptor: FmtDiagnosticDescriptor,
    ) -> Self {
        Self {
            kind,
            message: message.into(),
            descriptor,
            details: json!({}),
        }
    }

    fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }

    fn to_json(&self) -> Value {
        let descriptor = self.descriptor.to_json();
        let diagnostic = json!({
            "code": self.kind.code(),
            "category": self.kind.category(),
            "message": self.message,
            "descriptor": descriptor,
        });

        let mut obj = Map::new();
        obj.insert("code".to_string(), json!(self.kind.code()));
        obj.insert("category".to_string(), json!(self.kind.category()));
        obj.insert("message".to_string(), json!(self.message));
        obj.insert("descriptor".to_string(), self.descriptor.to_json());
        obj.insert("diagnostic".to_string(), diagnostic);
        if let Value::Object(details) = &self.details {
            for (key, value) in details {
                obj.insert(key.clone(), value.clone());
            }
        }
        Value::Object(obj)
    }

    fn into_cli_error(self) -> CliError {
        match self.kind {
            FmtDiagnosticKind::ParseFailure | FmtDiagnosticKind::UnsupportedSyntax => {
                CliError::ParseError(self.message)
            }
            FmtDiagnosticKind::NonIdempotent
            | FmtDiagnosticKind::WriteCheckModeMismatch
            | FmtDiagnosticKind::NotCanonical => CliError::Domain(self.message),
        }
    }
}

fn print_fmt_diagnostic(mode: OutputMode, diagnostic: &FmtDiagnostic) {
    if mode == OutputMode::Json {
        print_error_response(diagnostic.to_json());
    }
}

fn redacted_fmt_descriptor(
    path: Option<&Path>,
    language: FmtLanguage,
    check: bool,
    write: bool,
    changed: Option<bool>,
) -> FmtDiagnosticDescriptor {
    FmtDiagnosticDescriptor {
        input: if path.is_some() { "file" } else { "stdin" },
        extension: redacted_extension(path),
        language: language.as_str(),
        mode: fmt_mode(check, write),
        changed,
    }
}

fn redacted_extension(path: Option<&Path>) -> &'static str {
    match path
        .and_then(|path| path.extension())
        .and_then(|ext| ext.to_str())
    {
        Some("ail") => "ail",
        Some("acl") => "acl",
        Some(_) => "other",
        None => "none",
    }
}

fn fmt_mode(check: bool, write: bool) -> &'static str {
    match (check, write) {
        (true, true) => "check-write",
        (true, false) => "check",
        (false, true) => "write",
        (false, false) => "print",
    }
}

fn mode_mismatch_diagnostic(file: Option<&Path>) -> FmtDiagnostic {
    FmtDiagnostic::new(
        FmtDiagnosticKind::WriteCheckModeMismatch,
        "ail fmt cannot combine --check and --write; run --check to validate or --write to rewrite",
        redacted_fmt_descriptor(file, FmtLanguage::Unknown, true, true, None),
    )
}

fn parse_diagnostic(
    err: CliError,
    path: Option<&Path>,
    language: FmtLanguage,
    check: bool,
    write: bool,
) -> FmtDiagnostic {
    let message = match err {
        CliError::ParseError(message) | CliError::Domain(message) => message,
        other => other.to_string(),
    };
    let kind = if is_unsupported_syntax_message(&message) {
        FmtDiagnosticKind::UnsupportedSyntax
    } else {
        FmtDiagnosticKind::ParseFailure
    };
    FmtDiagnostic::new(
        kind,
        message,
        redacted_fmt_descriptor(path, language, check, write, None),
    )
}

fn is_unsupported_syntax_message(message: &str) -> bool {
    message.to_ascii_lowercase().contains("unsupported")
}

fn non_idempotent_diagnostic(
    path: Option<&Path>,
    language: FmtLanguage,
    check: bool,
    write: bool,
    changed: bool,
    reason: &'static str,
) -> FmtDiagnostic {
    FmtDiagnostic::new(
        FmtDiagnosticKind::NonIdempotent,
        "fmt produced output that is not stable on a second formatting pass",
        redacted_fmt_descriptor(path, language, check, write, Some(changed)),
    )
    .with_details(json!({ "reason": reason }))
}

fn not_canonical_diagnostic(file: Option<&Path>, outcome: &FmtOutcome) -> FmtDiagnostic {
    let message = match file {
        Some(path) => format!("fmt check failed: {} is not canonical", path.display()),
        None => "fmt check failed: stdin is not canonical".to_string(),
    };
    FmtDiagnostic::new(
        FmtDiagnosticKind::NotCanonical,
        message,
        redacted_fmt_descriptor(
            file,
            FmtLanguage::from_language_name(outcome.language),
            true,
            false,
            Some(true),
        ),
    )
    .with_details(json!({
        "changed": true,
        "op_count": outcome.op_count,
        "item_count": outcome.item_count,
        "language": outcome.language,
    }))
}

impl FmtLanguage {
    fn from_language_name(language: &str) -> Self {
        match language {
            "acl" => FmtLanguage::Acl,
            "ail-source" => FmtLanguage::AilSource,
            _ => FmtLanguage::Unknown,
        }
    }
}

// ── format_acl_source ──────────────────────────────────────────────────────

/// Format one ACL ChangeSet into deterministic canonical ACL text.
///
/// The formatter is deliberately semantic rather than whitespace-only:
/// it runs through `canonicalize_parsed`, so op ordering, ID normalization,
/// and materialized defaults match the canonical ChangeSet path used by
/// verification/apply.
pub(crate) fn format_acl_source(src: &str) -> Result<FmtOutcome, CliError> {
    let parsed = parse_changeset(src).map_err(CliError::ParseError)?;
    let canonical = canonicalize_parsed(parsed);

    let mut out = String::new();
    out.push_str(&format!(
        "change formatted acl={} base={}\n",
        canonical.acl_version, canonical.base_snapshot_id.0
    ));
    out.push_str(&format!(
        "author {}\n",
        format_acl_value(&canonical.meta.author)
    ));
    if canonical.meta.description != "<no description>" {
        out.push_str(&format!(
            "description {}\n",
            format_acl_value(&canonical.meta.description)
        ));
    }

    render_schema_versions(&mut out, &canonical);
    render_composition(&mut out, &canonical.composition);
    render_preconditions(&mut out, &canonical.preconditions);

    if !canonical.ops.is_empty() {
        out.push_str("ops\n");
        for op in &canonical.ops {
            if op.verb.is_empty() {
                continue;
            }
            out.push_str("  op ");
            out.push_str(&op.verb);
            for (key, value) in &op.args {
                out.push(' ');
                out.push_str(key);
                out.push('=');
                out.push_str(&format_acl_value(value));
            }
            out.push('\n');
        }
        out.push_str("end\n");
    }

    if let Some(expect) = canonical.expect {
        if !expect.0.is_empty() {
            out.push_str("expect\n");
            for claim in expect.0 {
                out.push_str("  ");
                out.push_str(&claim);
                out.push('\n');
            }
            out.push_str("end\n");
        }
    }

    if let Some(approval) = canonical.approval {
        if !approval.0.is_empty() {
            out.push_str("approval\n");
            for req in approval.0 {
                out.push_str("  ");
                out.push_str(&req);
                out.push('\n');
            }
            out.push_str("end\n");
        }
    }

    for block in canonical.blocks {
        out.push_str("block ");
        out.push_str(&block.kind);
        out.push(' ');
        out.push_str(&block.block_ref);
        if let Some(hash) = block.hash {
            out.push_str(" hash=");
            out.push_str(&format_acl_value(&hash));
        }
        out.push('\n');
        if !block.content.is_empty() {
            out.push_str(&block.content);
            if !block.content.ends_with('\n') {
                out.push('\n');
            }
        }
        out.push_str("end\n");
    }

    if !canonical.verify.is_empty() {
        out.push_str("verify\n");
        for line in canonical.verify {
            out.push_str("  ");
            out.push_str(&line);
            out.push('\n');
        }
        out.push_str("end\n");
    }

    out.push_str("end\n");

    let changed = normalize_trailing_newline(src) != out;
    let op_count = out
        .lines()
        .filter(|line| line.trim_start().starts_with("op "))
        .count();
    Ok(FmtOutcome {
        formatted: out,
        changed,
        op_count,
        item_count: op_count,
        language: "acl",
    })
}

fn format_for_path(src: &str, path: Option<&Path>) -> Result<FmtOutcome, CliError> {
    format_for_path_with_diagnostics(src, path).map_err(FmtDiagnostic::into_cli_error)
}

fn format_for_path_with_diagnostics(
    src: &str,
    path: Option<&Path>,
) -> Result<FmtOutcome, FmtDiagnostic> {
    format_for_path_with_mode_diagnostics(src, path, false, false)
}

fn format_for_path_with_mode_diagnostics(
    src: &str,
    path: Option<&Path>,
    check: bool,
    write: bool,
) -> Result<FmtOutcome, FmtDiagnostic> {
    let language = detect_format_language(src, path);
    let outcome = format_for_language(src, language)
        .map_err(|err| parse_diagnostic(err, path, language, check, write))?;
    ensure_format_idempotent(&outcome, path, language, check, write)?;
    Ok(outcome)
}

fn format_for_language(src: &str, language: FmtLanguage) -> Result<FmtOutcome, CliError> {
    match language {
        FmtLanguage::AilSource => {
            let (formatted, item_count) = format_ail_source(src)?;
            let changed = normalize_trailing_newline(src) != formatted;
            Ok(FmtOutcome {
                formatted,
                changed,
                op_count: 0,
                item_count,
                language: language.as_str(),
            })
        }
        FmtLanguage::Acl | FmtLanguage::Unknown => format_acl_source(src),
    }
}

fn ensure_format_idempotent(
    outcome: &FmtOutcome,
    path: Option<&Path>,
    language: FmtLanguage,
    check: bool,
    write: bool,
) -> Result<(), FmtDiagnostic> {
    let second = format_for_language(&outcome.formatted, language).map_err(|_| {
        non_idempotent_diagnostic(
            path,
            language,
            check,
            write,
            outcome.changed,
            "second_pass_parse_failed",
        )
    })?;
    if second.formatted != outcome.formatted {
        return Err(non_idempotent_diagnostic(
            path,
            language,
            check,
            write,
            outcome.changed,
            "second_pass_changed_output",
        ));
    }
    Ok(())
}

fn detect_format_language(src: &str, path: Option<&Path>) -> FmtLanguage {
    if path.is_some_and(|path| path.extension().and_then(|ext| ext.to_str()) == Some("ail"))
        || (path.is_none() && looks_like_ail_source(src))
    {
        FmtLanguage::AilSource
    } else {
        FmtLanguage::Acl
    }
}

fn looks_like_ail_source(src: &str) -> bool {
    let Some(line) = src
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with("//"))
    else {
        return false;
    };

    line.starts_with("module ")
        || line.starts_with("use ")
        || line.starts_with("capability ")
        || line.starts_with("fn ")
        || line.starts_with("test ")
        || line.starts_with("grant ")
}

// ── cmd_fmt ───────────────────────────────────────────────────────────────

pub(crate) fn cmd_fmt(
    mode: OutputMode,
    file: Option<PathBuf>,
    check: bool,
    write: bool,
) -> Result<(), CliError> {
    if check && write {
        let diagnostic = mode_mismatch_diagnostic(file.as_deref());
        print_fmt_diagnostic(mode, &diagnostic);
        return Err(diagnostic.into_cli_error());
    }

    if write && file.is_none() {
        return Err(CliError::Domain(
            "ail fmt --write requires --file <path>; stdin formatting is stdout-only".to_string(),
        ));
    }

    let input = if let Some(path) = &file {
        std::fs::read_to_string(path)?
    } else {
        let mut buf = String::new();
        use std::io::Read;
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    };

    let outcome = match format_for_path_with_mode_diagnostics(&input, file.as_deref(), check, write)
    {
        Ok(outcome) => outcome,
        Err(diagnostic) => {
            print_fmt_diagnostic(mode, &diagnostic);
            return Err(diagnostic.into_cli_error());
        }
    };

    if check && outcome.changed {
        let diagnostic = not_canonical_diagnostic(file.as_deref(), &outcome);
        print_fmt_diagnostic(mode, &diagnostic);
        return Err(diagnostic.into_cli_error());
    }

    if write {
        let path = file.as_ref().expect("write requires file path");
        if outcome.changed {
            std::fs::write(path, &outcome.formatted)?;
        }
    }

    let human_msg = if write {
        let count_label = if outcome.language == "ail-source" {
            "items"
        } else {
            "ops"
        };
        if outcome.changed {
            format!(
                "formatted: {}\n{count_label}: {}",
                file.as_ref().expect("write requires file path").display(),
                outcome.item_count
            )
        } else {
            format!(
                "already formatted: {}\n{count_label}: {}",
                file.as_ref().expect("write requires file path").display(),
                outcome.item_count
            )
        }
    } else if check {
        "fmt check passed".to_string()
    } else {
        outcome.formatted.clone()
    };

    print_response(
        mode,
        &human_msg,
        json!({
            "formatted": outcome.formatted,
            "changed": outcome.changed,
            "written": write && outcome.changed,
            "checked": check,
            "op_count": outcome.op_count,
            "item_count": outcome.item_count,
            "language": outcome.language,
        }),
    );
    Ok(())
}

// ── render helpers ────────────────────────────────────────────────────────

fn render_schema_versions(out: &mut String, canonical: &ail_change::canonical::CanonicalChangeSet) {
    if let Some(v) = &canonical.op_schema_version {
        out.push_str(&format!("op_schema {}\n", format_acl_value(v)));
    }
    if let Some(v) = &canonical.graph_schema_version {
        out.push_str(&format!("graph_schema {}\n", format_acl_value(v)));
    }
    if let Some(v) = &canonical.core_ir_schema_version {
        out.push_str(&format!("core_ir_schema {}\n", format_acl_value(v)));
    }
    if let Some(v) = &canonical.diagnostics_schema_version {
        out.push_str(&format!("diagnostics_schema {}\n", format_acl_value(v)));
    }
    if let Some(v) = &canonical.verification_schema_version {
        out.push_str(&format!("verification_schema {}\n", format_acl_value(v)));
    }
}

fn render_composition(out: &mut String, composition: &ail_change::parser::ChangeComposition) {
    for value in &composition.depends_on {
        out.push_str(&format!("depends_on {}\n", format_acl_value(value)));
    }
    for value in &composition.supersedes {
        out.push_str(&format!("supersedes {}\n", format_acl_value(value)));
    }
    for value in &composition.conflicts_with {
        out.push_str(&format!("conflicts_with {}\n", format_acl_value(value)));
    }
    for value in &composition.part_of {
        out.push_str(&format!("part_of {}\n", format_acl_value(value)));
    }
    for value in &composition.blocks {
        out.push_str(&format!("blocks {}\n", format_acl_value(value)));
    }
}

fn render_preconditions(out: &mut String, preconditions: &[Precondition]) {
    if preconditions.is_empty() {
        return;
    }
    out.push_str("requires\n");
    for precondition in preconditions {
        match precondition {
            Precondition::AssertExists(a) => {
                out.push_str(&format!("  assert_exists {}\n", a.node_id.0));
            }
            Precondition::AssertHash(a) => {
                out.push_str(&format!(
                    "  assert_hash {} sig={}\n",
                    a.node_id.0,
                    hex_block_hash(&a.expected_hash)
                ));
            }
            Precondition::AssertExistsByName(name) => {
                out.push_str(&format!("  assert_exists {}\n", format_acl_value(name)));
            }
            Precondition::AssertHashByName {
                name,
                expected_hash,
            } => {
                out.push_str(&format!(
                    "  assert_hash {} sig={}\n",
                    format_acl_value(name),
                    hex_block_hash(expected_hash)
                ));
            }
            Precondition::AssertContext {
                target_name,
                context_hash,
            } => {
                out.push_str("  assert_context ");
                out.push_str(&format_acl_value(target_name));
                if let Some(hash) = context_hash {
                    out.push_str(" hash=");
                    out.push_str(&format_acl_value(hash));
                }
                out.push('\n');
            }
        }
    }
    out.push_str("end\n");
}

fn format_acl_value(value: &str) -> String {
    let needs_quotes = value.is_empty()
        || value.chars().any(char::is_whitespace)
        || value.contains('#')
        || value.contains('"');
    if needs_quotes {
        format!("\"{}\"", value.replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

fn hex_block_hash(hash: &BlockHash) -> String {
    hash.0.iter().map(|b| format!("{b:02x}")).collect()
}

fn normalize_trailing_newline(src: &str) -> String {
    let mut normalized = src.to_string();
    if !normalized.ends_with('\n') {
        normalized.push('\n');
    }
    normalized
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_acl_orders_ops_and_materializes_defaults() {
        let src = "change x\nbase 0\nauthor Ana\nop verify\nop create_function id=Fn.CartTotal return=I64\nend\n";
        let out = format_acl_source(src).expect("fmt must parse");
        assert!(out.changed, "input should be reformatted");
        assert!(
            out.formatted
                .contains("op create_function id=fn.cart_total return=I64 visibility=private")
        );
        assert!(
            out.formatted.find("op create_function").unwrap()
                < out.formatted.find("op verify").unwrap(),
            "create op must be phase-ordered before verify"
        );
    }

    #[test]
    fn fmt_acl_preserves_requires_and_json_ready_count() {
        let src = "change x\nauthor Ana\nbase 0\nrequires\nassert_exists fn.CartTotal\nend\nop verify\nend\n";
        let out = format_acl_source(src).expect("fmt must parse");
        assert!(out.formatted.contains("requires\n"));
        assert!(out.formatted.contains("assert_exists fn.CartTotal"));
        assert_eq!(out.op_count, 1);
    }

    #[test]
    fn fmt_ail_source_formats_supported_source_surface() {
        let src = "fn add_pair(x:Int,y:Int)->Int=add(x,y)\n";
        let out = format_for_path(src, Some(std::path::Path::new("main.ail")))
            .expect("source fmt must parse");

        assert_eq!(out.language, "ail-source");
        assert_eq!(out.item_count, 1);
        assert_eq!(out.op_count, 0);
        assert_eq!(
            out.formatted,
            "fn add_pair(x: Int, y: Int) -> Int = x + y\n"
        );
    }

    #[test]
    fn fmt_stdin_detects_ail_source_surface() {
        let src = "// source file\nfn add_pair(x:Int,y:Int)->Int=add(x,y)\n";
        let out = format_for_path(src, None).expect("stdin source fmt must parse");

        assert_eq!(out.language, "ail-source");
        assert_eq!(out.item_count, 1);
        assert_eq!(
            out.formatted,
            "fn add_pair(x: Int, y: Int) -> Int = x + y\n"
        );
    }

    #[test]
    fn fmt_parse_failure_has_stable_diagnostic_code() {
        let err = CliError::ParseError("expected change header".to_string());
        let diagnostic = parse_diagnostic(
            err,
            Some(std::path::Path::new("/private/customer/change.acl")),
            FmtLanguage::Acl,
            false,
            false,
        );

        assert_eq!(diagnostic.kind.code(), "FMT_PARSE_FAILED");
        assert_eq!(diagnostic.kind.category(), "parse");
        assert_eq!(diagnostic.descriptor.input, "file");
        assert_eq!(diagnostic.descriptor.extension, "acl");
        assert_eq!(diagnostic.descriptor.language, "acl");
    }

    #[test]
    fn fmt_unsupported_source_syntax_has_stable_diagnostic_code() {
        let diagnostic = format_for_path_with_diagnostics(
            "export fn helper() -> Int = 1\n",
            Some(std::path::Path::new("main.ail")),
        )
        .expect_err("unsupported source syntax must fail with a diagnostic");

        assert_eq!(diagnostic.kind.code(), "FMT_UNSUPPORTED_SYNTAX");
        assert_eq!(diagnostic.kind.category(), "unsupported");
        assert_eq!(diagnostic.descriptor.language, "ail-source");
    }

    #[test]
    fn fmt_non_idempotent_diagnostic_has_stable_code() {
        let diagnostic = non_idempotent_diagnostic(
            Some(std::path::Path::new("main.ail")),
            FmtLanguage::AilSource,
            false,
            false,
            true,
            "second_pass_changed_output",
        );

        assert_eq!(diagnostic.kind.code(), "FMT_NON_IDEMPOTENT");
        assert_eq!(diagnostic.kind.category(), "stability");
        assert_eq!(diagnostic.descriptor.changed, Some(true));
        assert_eq!(diagnostic.to_json()["reason"], "second_pass_changed_output");
    }

    #[test]
    fn fmt_check_write_mode_mismatch_has_stable_diagnostic_code() {
        let diagnostic = mode_mismatch_diagnostic(Some(std::path::Path::new("main.ail")));

        assert_eq!(diagnostic.kind.code(), "FMT_WRITE_CHECK_MODE_MISMATCH");
        assert_eq!(diagnostic.kind.category(), "usage");
        assert_eq!(diagnostic.descriptor.mode, "check-write");
    }

    #[test]
    fn fmt_redacted_descriptor_excludes_path_segments_and_source_text() {
        let descriptor = redacted_fmt_descriptor(
            Some(std::path::Path::new(
                "/private/customer/secrets/not-canonical.ail",
            )),
            FmtLanguage::AilSource,
            true,
            false,
            Some(true),
        )
        .to_json();
        let rendered = descriptor.to_string();

        assert_eq!(descriptor["input"], "file");
        assert_eq!(descriptor["extension"], "ail");
        assert_eq!(descriptor["language"], "ail-source");
        assert_eq!(descriptor["mode"], "check");
        assert_eq!(descriptor["changed"], true);
        assert!(!rendered.contains("private"));
        assert!(!rendered.contains("customer"));
        assert!(!rendered.contains("secrets"));
        assert!(!rendered.contains("not-canonical"));
    }
}
