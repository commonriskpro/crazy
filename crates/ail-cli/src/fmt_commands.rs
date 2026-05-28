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
use serde_json::json;

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
    if path.is_some_and(|path| path.extension().and_then(|ext| ext.to_str()) == Some("ail"))
        || (path.is_none() && looks_like_ail_source(src))
    {
        let (formatted, item_count) = format_ail_source(src)?;
        let changed = normalize_trailing_newline(src) != formatted;
        return Ok(FmtOutcome {
            formatted,
            changed,
            op_count: 0,
            item_count,
            language: "ail-source",
        });
    }

    format_acl_source(src)
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

    let outcome = format_for_path(&input, file.as_deref())?;

    if check && outcome.changed {
        let message = match &file {
            Some(path) => format!("fmt check failed: {} is not canonical", path.display()),
            None => "fmt check failed: stdin is not canonical".to_string(),
        };
        if mode == OutputMode::Json {
            print_error_response(json!({
                "code": "FMT_NOT_CANONICAL",
                "message": message,
                "changed": true,
                "op_count": outcome.op_count,
                "item_count": outcome.item_count,
                "language": outcome.language,
            }));
        }
        return Err(CliError::Domain(message));
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
            "fn add_pair(x: Int, y: Int) -> Int = add(x, y)\n"
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
            "fn add_pair(x: Int, y: Int) -> Int = add(x, y)\n"
        );
    }
}
