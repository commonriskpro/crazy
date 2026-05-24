// ── ail-change::parser ────────────────────────────────────────────────────
//
// Line-oriented parser for the AI Change Language (ACL) DSL.
//
// # Grammar subset (this parser)
//
// ```text
// document        = "change" ws id attrs? nl change_body "end" nl?
// change_body     = (directive | op_line | section | comment | blank)*
// directive       = ("author" | "description" | "intent" | "base") ws value nl
// op_line         = "op" ws verb (ws kv)* nl
// section         = section_name nl section_body "end" nl
// section_name    = "metadata" | "requires" | "ops" | "expect" | "approval"
// section_body    = (directive | op_line | precondition | comment | blank)*
// precondition    = ("assert_exists" | "assert_hash") ws args nl
// kv              = key "=" value
// value           = quoted_string | bare_word
// ```
//
// # Verb → ChangeSetOp mapping
//
// | Prefix / exact match              | Variant    |
// |-----------------------------------|------------|
// | `create` / `create_*`             | Create     |
// | `set` / `set_*`                   | Set        |
// | `add` / `add_*`                   | Add        |
// | `remove` / `remove_*`             | Remove     |
// | `delete` / `delete_*`             | Delete     |
// | `disconnect` / `disconnect_*`     | Disconnect |
// | `rename` / `rename_*`             | Rename     |
// | `move` / `move_*`                 | Move       |
// | `replace` / `replace_*`           | Replace    |
// | `connect` / `connect_*`           | Connect    |
// | `bind` / `bind_*`                 | Bind       |
// | `expose` / `expose_*`             | Expose     |
// | `hide` / `hide_*`                 | Hide       |
// | `grant` / `grant_*`               | Grant      |
// | `revoke` / `revoke_*`             | Revoke     |
// | `infer` / `infer_*`               | Infer      |
// | `derive` / `derive_*`             | Derive     |
// | `generate` / `generate_*`         | Generate   |
// | `assert` / `assert_*`             | Assert     |
// | `lock` / `lock_*`                 | Lock       |
// | `refactor` / `refactor_*`         | Refactor   |
// | `migrate` / `migrate_*`           | Migrate    |
// | `approve` / `approve_*`           | Approve    |
// | `reject` / `reject_*`             | Reject     |
// | `deprecate` / `deprecate_*`       | Deprecate  |
// | `annotate` / `annotate_*`         | Annotate   |
// | `verify` / `verify_*`             | Verify     |
//
// # Pure function
//
// `parse_changeset` is a pure function: it takes `&str` and returns a `Result`.

use std::collections::BTreeMap;

use ail_core::semantic_graph::NodeRef;

// ── Re-exports from parser_types ─────────────────────────────────────────
//
// All public DTOs live in `parser_types`; they are re-exported here so that
// existing import paths (`use crate::parser::ParsedChangeSet` etc.) keep
// working without any change to callers.

pub use crate::parser_types::{
    ApprovalRequirements, ChangeComposition, ExpectClaims, OpArgs, ParsedBlock, ParsedChangeSet,
    ParsedOp,
};

use crate::{
    canonical::Precondition,
    model::{
        AssertExists, AssertHash, BlockHash, ChangeSet, ChangeSetMeta, ChangeSetOp, SnapshotId,
        Timestamp,
    },
};

// ── Section state ─────────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Eq)]
enum Section {
    TopLevel,
    Metadata,
    Requires,
    Ops,
    Expect,
    Approval,
    #[allow(dead_code)]
    Block,
    Verify,
}

// ── parse_changeset ───────────────────────────────────────────────────────

/// Parse an ACL document from a string into a `ParsedChangeSet`.
///
/// This is a **pure function**: no I/O, no side effects.
///
/// # Errors
///
/// Returns a human-readable `String` if:
/// - `author` is missing
/// - `base` is missing or not a valid `u64`
/// - An op verb is not recognised
/// - A `requires` assertion is malformed
/// - A section block is unclosed
/// - An unrecognised directive is encountered
pub fn parse_changeset(src: &str) -> Result<ParsedChangeSet, String> {
    let mut author: Option<String> = None;
    let mut description: Option<String> = None;
    let mut base: Option<SnapshotId> = None;
    let mut acl_version: String = "1.0".to_string();
    let mut op_schema_version: Option<String> = None;
    let mut graph_schema_version: Option<String> = None;
    let mut core_ir_schema_version: Option<String> = None;
    let mut diagnostics_schema_version: Option<String> = None;
    let mut verification_schema_version: Option<String> = None;
    let mut ops: Vec<ChangeSetOp> = Vec::new();
    let mut parsed_ops: Vec<ParsedOp> = Vec::new();
    let mut preconditions: Vec<Precondition> = Vec::new();
    let mut expect_claims: Vec<String> = Vec::new();
    let mut approval_reqs: Vec<String> = Vec::new();
    let mut composition = ChangeComposition::default();
    let mut blocks: Vec<ParsedBlock> = Vec::new();
    let mut verify_lines: Vec<String> = Vec::new();

    // In-progress block being collected.
    let mut current_block: Option<ParsedBlock> = None;
    // Whether we are inside a `verify ... end` block form.
    let mut in_verify_block = false;

    let mut section = Section::TopLevel;
    let mut line_num: usize = 0;

    // Tracks whether we are inside the outer `change ... end` block.
    // Lines before `change` and after the closing `end` are accepted if blank/comment.
    let mut in_change = false;

    for raw in src.lines() {
        line_num += 1;
        let line = raw.trim();

        // Always skip blanks and comments — except inside block content
        // (blocks may contain blank lines as semantic content; we skip them
        // here for simplicity since the spec treats content as opaque free text).
        if line.is_empty() || line.starts_with('#') {
            // Collect blank lines inside block content.
            if let Some(ref mut blk) = current_block {
                if !line.is_empty() {
                    // comment inside block — skip
                } else {
                    blk.content.push('\n');
                }
            }
            continue;
        }

        // Handle `change <id> ...` opener.
        if line.starts_with("change ") || line == "change" {
            if in_change {
                return Err(format!(
                    "line {line_num}: nested 'change' declaration is not allowed"
                ));
            }
            in_change = true;
            // Parse inline attrs from the change line (e.g. `acl=1.0 base=0`).
            let after_id = line.splitn(3, ' ').nth(2).unwrap_or("").trim();
            if !after_id.is_empty() {
                parse_change_line_attrs(after_id, line_num, &mut acl_version, &mut base)?;
            }
            continue;
        }

        // `end` closes the innermost open context.
        if line == "end" {
            // Close an in-progress block first.
            if let Some(blk) = current_block.take() {
                blocks.push(blk);
                continue;
            }
            // Close a verify block.
            if in_verify_block {
                in_verify_block = false;
                section = Section::TopLevel;
                continue;
            }
            match section {
                Section::Metadata
                | Section::Requires
                | Section::Ops
                | Section::Expect
                | Section::Approval
                | Section::Block
                | Section::Verify => {
                    // Closes the current inner section.
                    section = Section::TopLevel;
                }
                Section::TopLevel => {
                    // Closes the change block itself.
                    in_change = false;
                }
            }
            continue;
        }

        // If we are inside a block body, collect content lines.
        if let Some(ref mut blk) = current_block {
            if !blk.content.is_empty() {
                blk.content.push('\n');
            }
            blk.content.push_str(line);
            continue;
        }

        // `block <kind> @ref [attrs]` opener — only at top level or inside change body.
        if line.starts_with("block ") && section == Section::TopLevel {
            let rest = &line["block ".len()..];
            let blk = parse_block_header(rest, line_num)?;
            current_block = Some(blk);
            continue;
        }

        // `verify` forms at top level.
        if section == Section::TopLevel || section == Section::Verify {
            // `verify` alone → block form opener.
            if line == "verify" {
                in_verify_block = true;
                section = Section::Verify;
                continue;
            }
            // `verify <kv or bare words>` → short form; collect the whole line.
            if let Some(rest) = line.strip_prefix("verify ") {
                // Short form: collect the tail as a single verify entry.
                verify_lines.push(rest.trim().to_string());
                continue;
            }
        }

        // Inside verify block form: collect lines.
        if section == Section::Verify {
            verify_lines.push(line.to_string());
            continue;
        }

        // Section openers.
        match line {
            "metadata" => {
                section = Section::Metadata;
                continue;
            }
            "requires" => {
                section = Section::Requires;
                continue;
            }
            "ops" => {
                section = Section::Ops;
                continue;
            }
            "expect" => {
                section = Section::Expect;
                continue;
            }
            "approval" => {
                section = Section::Approval;
                continue;
            }
            _ => {}
        }

        // Dispatch by current section.
        match section {
            Section::Metadata => {
                parse_metadata_line(
                    line,
                    line_num,
                    &mut author,
                    &mut description,
                    &mut base,
                    &mut acl_version,
                    &mut op_schema_version,
                    &mut graph_schema_version,
                    &mut core_ir_schema_version,
                    &mut diagnostics_schema_version,
                    &mut verification_schema_version,
                    &mut composition,
                )?;
            }
            Section::Requires => {
                parse_precondition_line(line, line_num, &mut preconditions)?;
            }
            Section::TopLevel | Section::Ops => {
                parse_op_or_directive(
                    line,
                    line_num,
                    &mut ops,
                    &mut parsed_ops,
                    &mut author,
                    &mut description,
                    &mut base,
                    &mut acl_version,
                    &mut op_schema_version,
                    &mut graph_schema_version,
                    &mut core_ir_schema_version,
                    &mut diagnostics_schema_version,
                    &mut verification_schema_version,
                    &mut composition,
                    &section,
                )?;
            }
            Section::Expect => {
                // Collect raw expect claim lines.
                expect_claims.push(line.to_string());
            }
            Section::Approval => {
                // Collect raw approval requirement lines.
                approval_reqs.push(line.to_string());
            }
            Section::Block | Section::Verify => {
                // These are handled above; this branch is unreachable in practice.
            }
        }
    }

    // Check for unclosed sections.
    if current_block.is_some() {
        return Err("unclosed block section".to_string());
    }
    if section != Section::TopLevel {
        return Err(format!(
            "unclosed section: {}",
            match section {
                Section::Metadata => "metadata",
                Section::Requires => "requires",
                Section::Ops => "ops",
                Section::Expect => "expect",
                Section::Approval => "approval",
                Section::Block => "block",
                Section::Verify => "verify",
                Section::TopLevel => unreachable!(),
            }
        ));
    }

    // Validate required fields.
    let author = author.ok_or_else(|| "missing required field: author".to_string())?;
    let base_snapshot_id = base.ok_or_else(|| "missing required field: base".to_string())?;
    let description = description.unwrap_or_default();

    Ok(ParsedChangeSet {
        changeset: ChangeSet {
            meta: ChangeSetMeta {
                author,
                description,
                timestamp: Timestamp(0),
            },
            base_snapshot_id,
            ops,
        },
        preconditions,
        parsed_ops,
        acl_version,
        op_schema_version,
        graph_schema_version,
        core_ir_schema_version,
        diagnostics_schema_version,
        verification_schema_version,
        expect: if expect_claims.is_empty() {
            None
        } else {
            Some(ExpectClaims(expect_claims))
        },
        approval: if approval_reqs.is_empty() {
            None
        } else {
            Some(ApprovalRequirements(approval_reqs))
        },
        composition,
        blocks,
        verify: verify_lines,
    })
}

// ── parse_change_line_attrs ───────────────────────────────────────────────

/// Parse inline attrs from the `change <id> <attrs>` header line.
///
/// Recognises `acl=<version>` and `base=<u64|snapshot_NNN>`.
fn parse_change_line_attrs(
    attrs: &str,
    line_num: usize,
    acl_version: &mut String,
    base: &mut Option<SnapshotId>,
) -> Result<(), String> {
    for token in attrs.split_whitespace() {
        if let Some(v) = token.strip_prefix("acl=") {
            *acl_version = v.to_string();
        } else if let Some(v) = token.strip_prefix("base=") {
            *base = Some(parse_snapshot_id(v, line_num)?);
        }
        // Unknown attrs are silently ignored (forward-compatible).
    }
    Ok(())
}

// ── parse_snapshot_id ─────────────────────────────────────────────────────

/// Parse a snapshot id that is either a plain `u64` or `snapshot_<u64>`.
///
/// The doc-style form `snapshot_123` is accepted everywhere a base id appears.
fn parse_snapshot_id(raw: &str, line_num: usize) -> Result<SnapshotId, String> {
    let numeric = if let Some(n) = raw.strip_prefix("snapshot_") {
        n
    } else {
        raw
    };
    numeric
        .parse::<u64>()
        .map(SnapshotId)
        .map_err(|_| format!("line {line_num}: invalid base snapshot id: '{raw}'"))
}

// ── parse_block_header ────────────────────────────────────────────────────

/// Parse the header portion of a `block <kind> @ref [attrs]` line.
///
/// Returns a `ParsedBlock` with empty content (content is collected by
/// the caller as subsequent lines).
fn parse_block_header(rest: &str, line_num: usize) -> Result<ParsedBlock, String> {
    // rest = "<kind> @ref [key=value ...]"
    let mut tokens = rest.splitn(3, ' ');
    let kind = tokens
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("line {line_num}: block requires a kind (e.g. 'expr')"))?
        .to_string();
    let block_ref = tokens
        .next()
        .filter(|s| s.starts_with('@'))
        .ok_or_else(|| format!("line {line_num}: block requires a @ref identifier"))?
        .to_string();
    let attrs_str = tokens.next().unwrap_or("").trim();

    let hash = if attrs_str.is_empty() {
        None
    } else {
        // Look for `hash=<value>` in remaining attrs.
        attrs_str.split_whitespace().find_map(|tok| {
            tok.strip_prefix("hash=")
                .map(|v| extract_string_value(v).to_string())
        })
    };

    Ok(ParsedBlock {
        kind,
        block_ref,
        content: String::new(),
        hash,
    })
}

// ── parse_metadata_line ───────────────────────────────────────────────────

/// Parse a directive that supplies metadata (author, description, base, intent, schema versions).
///
/// Called when section is `TopLevel` or `Metadata`.
#[allow(clippy::too_many_arguments)]
fn parse_metadata_line(
    line: &str,
    line_num: usize,
    author: &mut Option<String>,
    description: &mut Option<String>,
    base: &mut Option<SnapshotId>,
    acl_version: &mut String,
    op_schema_version: &mut Option<String>,
    graph_schema_version: &mut Option<String>,
    core_ir_schema_version: &mut Option<String>,
    diagnostics_schema_version: &mut Option<String>,
    verification_schema_version: &mut Option<String>,
    composition: &mut ChangeComposition,
) -> Result<(), String> {
    if let Some(v) = line.strip_prefix("author ") {
        *author = Some(v.trim().to_string());
    } else if let Some(v) = line.strip_prefix("description ") {
        *description = Some(v.trim().to_string());
    } else if let Some(v) = line.strip_prefix("intent ") {
        *description = Some(extract_string_value(v.trim()));
    } else if let Some(v) = line.strip_prefix("base ") {
        *base = Some(parse_snapshot_id(v.trim(), line_num)?);
    } else if let Some(v) = line.strip_prefix("language ") {
        // `language acl/1.0` → extract "1.0"
        let v = v.trim();
        if let Some(ver) = v.strip_prefix("acl/") {
            *acl_version = ver.to_string();
        }
        // Unknown language ids are silently ignored.
    } else if let Some(v) = line.strip_prefix("acl_version ") {
        // `acl_version acl/1.0` or `acl_version 1.0` → extract version.
        let v = v.trim();
        *acl_version = v.strip_prefix("acl/").unwrap_or(v).to_string();
    } else if let Some(v) = line.strip_prefix("op_schema ") {
        *op_schema_version = Some(v.trim().to_string());
    } else if let Some(v) = line.strip_prefix("graph_schema ") {
        *graph_schema_version = Some(v.trim().to_string());
    } else if let Some(v) = line.strip_prefix("core_ir_schema ") {
        *core_ir_schema_version = Some(v.trim().to_string());
    } else if let Some(v) = line.strip_prefix("diagnostics_schema ") {
        *diagnostics_schema_version = Some(v.trim().to_string());
    } else if let Some(v) = line.strip_prefix("verification_schema ") {
        *verification_schema_version = Some(v.trim().to_string());
    } else if let Some(v) = line.strip_prefix("depends_on ") {
        composition.depends_on.push(v.trim().to_string());
    } else if let Some(v) = line.strip_prefix("supersedes ") {
        composition.supersedes.push(v.trim().to_string());
    } else if let Some(v) = line.strip_prefix("conflicts_with ") {
        composition.conflicts_with.push(v.trim().to_string());
    } else if let Some(v) = line.strip_prefix("part_of ") {
        composition.part_of.push(v.trim().to_string());
    } else if let Some(v) = line.strip_prefix("blocks ") {
        composition.blocks.push(v.trim().to_string());
    } else if let Some(v) = line.strip_prefix("author_role ") {
        // `author_role` is an optional metadata field — accepted silently.
        let _ = v;
    } else if let Some(v) = line.strip_prefix("reason ") {
        // `reason` is optional free-text metadata — accepted silently.
        let _ = v;
    } else if line.starts_with("op ") {
        // `op` lines inside `metadata` section are a syntax error.
        return Err(format!(
            "line {line_num}: 'op' directive is not allowed inside 'metadata' section"
        ));
    } else {
        return Err(format!("line {line_num}: unrecognised directive: '{line}'"));
    }
    Ok(())
}

// ── parse_op_or_directive ─────────────────────────────────────────────────

/// Parse a line that is either an `op` line or a top-level metadata directive.
///
/// When called for `Section::Ops`, only `op` lines are accepted.
/// When called for `Section::TopLevel`, both `op` and metadata directives
/// are accepted.
#[allow(clippy::too_many_arguments)]
fn parse_op_or_directive(
    line: &str,
    line_num: usize,
    ops: &mut Vec<ChangeSetOp>,
    parsed_ops: &mut Vec<ParsedOp>,
    author: &mut Option<String>,
    description: &mut Option<String>,
    base: &mut Option<SnapshotId>,
    acl_version: &mut String,
    op_schema_version: &mut Option<String>,
    graph_schema_version: &mut Option<String>,
    core_ir_schema_version: &mut Option<String>,
    diagnostics_schema_version: &mut Option<String>,
    verification_schema_version: &mut Option<String>,
    composition: &mut ChangeComposition,
    section: &Section,
) -> Result<(), String> {
    if let Some(rest) = line.strip_prefix("op ") {
        // Extract the verb (first token after "op ").
        let mut tokens = rest.splitn(2, |c: char| c.is_whitespace());
        let verb = tokens.next().unwrap_or("").trim();
        let args_str = tokens.next().unwrap_or("").trim();

        let op_kind =
            map_verb(verb).ok_or_else(|| format!("line {line_num}: unknown op verb: '{verb}'"))?;
        let args = parse_kv_args(args_str);

        ops.push(op_kind.clone());
        parsed_ops.push(ParsedOp {
            kind: op_kind,
            verb: verb.to_string(),
            args,
        });
    } else if *section == Section::TopLevel {
        // Allow metadata directives at the top level.
        parse_metadata_line(
            line,
            line_num,
            author,
            description,
            base,
            acl_version,
            op_schema_version,
            graph_schema_version,
            core_ir_schema_version,
            diagnostics_schema_version,
            verification_schema_version,
            composition,
        )?;
    } else {
        return Err(format!(
            "line {line_num}: expected 'op' directive inside 'ops' section, got: '{line}'"
        ));
    }
    Ok(())
}

// ── parse_precondition_line ───────────────────────────────────────────────

/// Parse a precondition line inside a `requires` section.
///
/// Supported forms:
/// - `assert_exists <node_id_u32>`           — numeric NodeRef (legacy)
/// - `assert_exists <node_name>`             — named NodeRef (e.g. `type.Cart`)
/// - `assert_hash <node_id_u32> sig=<hex>`  — numeric NodeRef hash check
/// - `assert_hash <node_name> sig=<hex>`    — named NodeRef hash check
/// - `assert_context <node_name> [hash=<hex>]` — context slice assertion
fn parse_precondition_line(
    line: &str,
    line_num: usize,
    preconditions: &mut Vec<Precondition>,
) -> Result<(), String> {
    if let Some(rest) = line.strip_prefix("assert_exists ") {
        let id_str = rest.trim();
        // Try numeric u32 first (legacy form), fall back to named NodeRef.
        if let Ok(n) = id_str.parse::<u32>() {
            preconditions.push(Precondition::AssertExists(AssertExists {
                node_id: NodeRef(n),
            }));
        } else {
            preconditions.push(Precondition::AssertExistsByName(id_str.to_string()));
        }
    } else if let Some(rest) = line.strip_prefix("assert_hash ") {
        // Expected format: `<node_id_or_name> sig=<hex>`
        let mut parts = rest.splitn(2, ' ');
        let id_part = parts.next().unwrap_or("").trim();
        let kv_part = parts.next().unwrap_or("").trim();

        let hex = extract_kv_value(kv_part, "sig")
            .ok_or_else(|| format!("line {line_num}: assert_hash requires 'sig=<hex>' argument"))?;
        let expected_hash = decode_hex32(&hex, line_num)?;

        // Try numeric u32 first (legacy form), fall back to named NodeRef.
        if let Ok(n) = id_part.parse::<u32>() {
            preconditions.push(Precondition::AssertHash(AssertHash {
                node_id: NodeRef(n),
                expected_hash,
            }));
        } else {
            preconditions.push(Precondition::AssertHashByName {
                name: id_part.to_string(),
                expected_hash,
            });
        }
    } else if let Some(rest) = line.strip_prefix("assert_context ") {
        // Format: `assert_context <target_name> [hash=<hex_or_short>]`
        let mut parts = rest.splitn(2, ' ');
        let target_name = parts.next().unwrap_or("").trim().to_string();
        let kv_part = parts.next().unwrap_or("").trim();
        let context_hash =
            extract_kv_value(kv_part, "hash").or_else(|| extract_kv_value(kv_part, "context_hash"));
        if target_name.is_empty() {
            return Err(format!(
                "line {line_num}: assert_context requires a target name"
            ));
        }
        preconditions.push(Precondition::AssertContext {
            target_name,
            context_hash,
        });
    } else {
        return Err(format!(
            "line {line_num}: unrecognised precondition: '{line}'"
        ));
    }
    Ok(())
}

// ── map_verb ─────────────────────────────────────────────────────────────

/// Map an ACL op verb to the corresponding `ChangeSetOp` variant.
///
/// Returns `None` if the verb does not match any known prefix rule.
fn map_verb(verb: &str) -> Option<ChangeSetOp> {
    // Helper: exact match or `<stem>_` prefix.
    fn matches(verb: &str, stem: &str) -> bool {
        verb == stem || verb.starts_with(&format!("{stem}_"))
    }

    if matches(verb, "create") {
        Some(ChangeSetOp::Create)
    } else if matches(verb, "set") {
        Some(ChangeSetOp::Set)
    } else if matches(verb, "add") {
        Some(ChangeSetOp::Add)
    } else if matches(verb, "remove") {
        Some(ChangeSetOp::Remove)
    } else if matches(verb, "delete") {
        Some(ChangeSetOp::Delete)
    } else if matches(verb, "disconnect") {
        Some(ChangeSetOp::Disconnect)
    } else if matches(verb, "rename") {
        Some(ChangeSetOp::Rename)
    } else if matches(verb, "move") {
        Some(ChangeSetOp::Move)
    } else if matches(verb, "replace") {
        Some(ChangeSetOp::Replace)
    } else if matches(verb, "connect") {
        Some(ChangeSetOp::Connect)
    } else if matches(verb, "bind") {
        Some(ChangeSetOp::Bind)
    } else if matches(verb, "expose") {
        Some(ChangeSetOp::Expose)
    } else if matches(verb, "hide") {
        Some(ChangeSetOp::Hide)
    } else if matches(verb, "grant") {
        Some(ChangeSetOp::Grant)
    } else if matches(verb, "revoke") {
        Some(ChangeSetOp::Revoke)
    } else if matches(verb, "infer") {
        Some(ChangeSetOp::Infer)
    } else if matches(verb, "derive") {
        Some(ChangeSetOp::Derive)
    } else if matches(verb, "generate") {
        Some(ChangeSetOp::Generate)
    } else if matches(verb, "assert") {
        Some(ChangeSetOp::Assert)
    } else if matches(verb, "lock") {
        Some(ChangeSetOp::Lock)
    } else if matches(verb, "refactor") {
        Some(ChangeSetOp::Refactor)
    } else if matches(verb, "migrate") {
        Some(ChangeSetOp::Migrate)
    } else if matches(verb, "approve") {
        Some(ChangeSetOp::Approve)
    } else if matches(verb, "reject") {
        Some(ChangeSetOp::Reject)
    } else if matches(verb, "deprecate") {
        Some(ChangeSetOp::Deprecate)
    } else if matches(verb, "annotate") {
        Some(ChangeSetOp::Annotate)
    } else if matches(verb, "verify") {
        Some(ChangeSetOp::Verify)
    } else {
        None
    }
}

// ── helpers ───────────────────────────────────────────────────────────────

/// Parse a sequence of `key=value` tokens into an `OpArgs` map.
///
/// Handles quoted values (`"..."`) by stripping the surrounding quotes.
/// Tokens that do not contain `=` are silently ignored (forward-compatible).
pub fn parse_kv_args(args_str: &str) -> OpArgs {
    let mut map = BTreeMap::new();
    let mut remaining = args_str.trim();

    while !remaining.is_empty() {
        // Find the next `key=` boundary.
        let eq_pos = match remaining.find('=') {
            Some(p) => p,
            None => break,
        };
        let key = remaining[..eq_pos].trim().to_string();
        remaining = &remaining[eq_pos + 1..];

        // Parse the value: quoted string or bare word.
        let (value, rest) = if let Some(stripped) = remaining.strip_prefix('"') {
            // Scan for the closing quote.
            let end = stripped.find('"').map(|p| p + 2).unwrap_or(remaining.len());
            let raw = &remaining[..end];
            let value = extract_string_value(raw.trim());
            let rest = remaining[end..].trim_start();
            (value, rest)
        } else {
            // Bare values normally end at whitespace, but ACL expression bodies
            // may contain spaces inside parenthesized calls: `body=add(x, y)`.
            let end = bare_value_end(remaining);
            let value = remaining[..end].trim().to_string();
            let rest = remaining[end..].trim_start();
            (value, rest)
        };

        if !key.is_empty() {
            map.insert(key, value);
        }
        remaining = rest;
    }

    map
}

fn bare_value_end(value: &str) -> usize {
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    for (idx, ch) in value.char_indices() {
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            c if c.is_whitespace()
                && paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0 =>
            {
                return idx;
            }
            _ => {}
        }
    }
    value.len()
}

/// Extract the string content from a value token.
///
/// If the token is quoted (`"..."`), returns the content without quotes.
/// Otherwise returns the raw token as-is.
fn extract_string_value(token: &str) -> String {
    if token.starts_with('"') && token.ends_with('"') && token.len() >= 2 {
        token[1..token.len() - 1].to_string()
    } else {
        token.to_string()
    }
}

/// Extract the value for a specific key from a space-separated list of `key=value` pairs.
///
/// Returns `None` if the key is not present.
fn extract_kv_value(kv_str: &str, key: &str) -> Option<String> {
    for pair in kv_str.split_whitespace() {
        if let Some(rest) = pair.strip_prefix(key)
            && let Some(value) = rest.strip_prefix('=')
        {
            return Some(extract_string_value(value));
        }
    }
    None
}

/// Parse a numeric `NodeRef` from a string containing a `u32`.
///
/// Named NodeRefs (like `type.Cart`) are handled by the calling context
/// which dispatches to `Precondition::AssertExistsByName` instead.
#[allow(dead_code)]
fn parse_node_ref(s: &str, line_num: usize) -> Result<NodeRef, String> {
    s.parse::<u32>()
        .map(NodeRef)
        .map_err(|_| format!("line {line_num}: invalid node id (expected u32): '{s}'"))
}

/// Decode a 64-character hex string into a `[u8; 32]` blake3 hash.
fn decode_hex32(hex: &str, line_num: usize) -> Result<BlockHash, String> {
    if hex.len() != 64 {
        return Err(format!(
            "line {line_num}: hash must be 64 hex characters, got {} characters",
            hex.len()
        ));
    }
    let mut bytes = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let hi = hex_nibble(chunk[0], line_num)?;
        let lo = hex_nibble(chunk[1], line_num)?;
        bytes[i] = (hi << 4) | lo;
    }
    Ok(BlockHash(bytes))
}

/// Convert a single ASCII hex character to its nibble value.
fn hex_nibble(c: u8, line_num: usize) -> Result<u8, String> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(format!(
            "line {line_num}: invalid hex character: '{}'",
            c as char
        )),
    }
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "parser_tests.rs"]
mod tests;
