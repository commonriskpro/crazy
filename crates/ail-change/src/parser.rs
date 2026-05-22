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

// ── OpArgs ────────────────────────────────────────────────────────────────

/// Parsed key=value arguments for a single op line.
///
/// Keys and values are kept as strings; semantic validation happens downstream
/// in op-schema validation and the canonicalizer.
pub type OpArgs = BTreeMap<String, String>;

// ── ParsedOp ──────────────────────────────────────────────────────────────

/// A fully parsed op line: verb kind, full verb string, and kv args.
///
/// `kind` is the coarse `ChangeSetOp` variant derived from the verb prefix.
/// `verb` is the complete verb as written (e.g. `"create_function"`).
/// `args` holds every `key=value` pair from the op line, in sorted key order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedOp {
    /// Coarse variant category of the op.
    pub kind: ChangeSetOp,
    /// Full verb as written in the source (e.g. `"create_function"`).
    pub verb: String,
    /// Key/value arguments from the op line.
    pub args: OpArgs,
}

// ── ChangeComposition ─────────────────────────────────────────────────────

/// Cross-changeset composition relationships.
///
/// All fields default to empty vecs; absent directives produce no entries.
#[derive(Clone, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct ChangeComposition {
    /// ChangeSet IDs that must be applied before this one.
    pub depends_on: Vec<String>,
    /// ChangeSet IDs superseded by this one.
    pub supersedes: Vec<String>,
    /// ChangeSet IDs that conflict with this one (require resolution).
    pub conflicts_with: Vec<String>,
    /// Parent epic or umbrella changeset this belongs to.
    pub part_of: Vec<String>,
    /// ChangeSet IDs blocked from applying until this one resolves.
    pub blocks: Vec<String>,
}

// ── ParsedBlock ───────────────────────────────────────────────────────────

/// A typed block section (`block <kind> @ref ... end`).
///
/// Blocks carry free-form content (expressions, schemas, docs, ranges) that
/// is parsed by a subgrammar downstream. The canonicalizer carries them
/// through intact and records their blake3 hash.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ParsedBlock {
    /// Block type: `expr`, `schema`, `doc`, `range`, `policy`, `test`.
    pub kind: String,
    /// The `@ref` identifier for this block (e.g. `@expr.checkout_body`).
    pub block_ref: String,
    /// Raw content lines between the block header and `end`.
    pub content: String,
    /// Optional hash declared inline on the block header (`hash=<value>`).
    pub hash: Option<String>,
}

// ── ExpectClaims ─────────────────────────────────────────────────────────

/// Claims about the expected diff, written by the LLM in the `expect` section.
///
/// These are verifiable claims (not authoritative policy) — the toolchain
/// checks them against the actual structural diff.
#[derive(Clone, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct ExpectClaims(pub Vec<String>);

// ── ApprovalRequirements ─────────────────────────────────────────────────

/// Approval requirements declared in the `approval` section.
///
/// Each string is a raw `require_if <condition>` directive.
#[derive(Clone, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct ApprovalRequirements(pub Vec<String>);

use crate::{
    canonical::Precondition,
    model::{
        AssertExists, AssertHash, BlockHash, ChangeSet, ChangeSetMeta, ChangeSetOp, SnapshotId,
        Timestamp,
    },
};

// ── ParsedChangeSet ───────────────────────────────────────────────────────

/// Result of parsing an ACL document.
///
/// Carries the typed `ChangeSet` (ops + metadata) plus any preconditions
/// declared in the `requires` section.  Preconditions are kept separate
/// because `ChangeSet` itself is a pure value type with no precondition
/// field; preconditions are attached during canonicalization.
///
/// `parsed_ops` mirrors `changeset.ops` but carries the full verb and
/// kv args; use these for canonicalization and op-schema validation.
/// `changeset.ops` is preserved for backward compatibility with apply tests.
///
/// ## Schema versions (from doc §Versioning y schema evolution)
///
/// Schemas evolve independently; each field tracks the declared version for
/// the corresponding schema component.  All fields default to `None` when
/// the corresponding directive is absent from the metadata section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedChangeSet {
    /// The typed changeset (ops + metadata + base snapshot).
    pub changeset: ChangeSet,
    /// Preconditions declared in the `requires` section.
    pub preconditions: Vec<Precondition>,
    /// Enriched ops with full verb and kv args (parallel to `changeset.ops`).
    pub parsed_ops: Vec<ParsedOp>,
    /// ACL language syntax version (defaults to `"1.0"`).
    pub acl_version: String,
    /// Op-schema version declared by this changeset (`op_schema <N>`).
    pub op_schema_version: Option<String>,
    /// Semantic Graph schema version (`graph_schema <N>`).
    pub graph_schema_version: Option<String>,
    /// Core IR schema version (`core_ir_schema <N>`).
    pub core_ir_schema_version: Option<String>,
    /// Diagnostics format version (`diagnostics_schema <N>`).
    pub diagnostics_schema_version: Option<String>,
    /// Verification report format version (`verification_schema <N>`).
    pub verification_schema_version: Option<String>,
    /// Claims about the expected diff (from `expect` section).
    pub expect: Option<ExpectClaims>,
    /// Approval requirements (from `approval` section).
    pub approval: Option<ApprovalRequirements>,
    /// Cross-changeset composition relationships (from `metadata` section).
    pub composition: ChangeComposition,
    /// Typed block sections (`block <kind> @ref ... end`).
    pub blocks: Vec<ParsedBlock>,
    /// Verify directives: short form lines and block form lines combined.
    pub verify: Vec<String>,
}

impl Default for ParsedChangeSet {
    fn default() -> Self {
        Self {
            changeset: ChangeSet {
                meta: ChangeSetMeta {
                    author: String::new(),
                    description: String::new(),
                    timestamp: Timestamp(0),
                },
                base_snapshot_id: SnapshotId(0),
                ops: Vec::new(),
            },
            preconditions: Vec::new(),
            parsed_ops: Vec::new(),
            acl_version: "1.0".to_string(),
            op_schema_version: None,
            graph_schema_version: None,
            core_ir_schema_version: None,
            diagnostics_schema_version: None,
            verification_schema_version: None,
            expect: None,
            approval: None,
            composition: ChangeComposition::default(),
            blocks: Vec::new(),
            verify: Vec::new(),
        }
    }
}

// ── Section state ─────────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Eq)]
enum Section {
    TopLevel,
    Metadata,
    Requires,
    Ops,
    Expect,
    Approval,
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
            if line.starts_with("verify ") {
                // Short form: collect the tail as a single verify entry.
                verify_lines.push(line["verify ".len()..].trim().to_string());
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
        let context_hash = extract_kv_value(kv_part, "hash")
            .or_else(|| extract_kv_value(kv_part, "context_hash"));
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
        let (value, rest) = if remaining.starts_with('"') {
            // Scan for the closing quote.
            let end = remaining[1..]
                .find('"')
                .map(|p| p + 2)
                .unwrap_or(remaining.len());
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
mod tests {
    use super::*;
    use crate::model::{ChangeSetOp, SnapshotId};

    // Scenario: minimal valid input (author + base, no ops).
    //   GIVEN a document with only required fields
    //   WHEN parse_changeset is called
    //   THEN the returned ParsedChangeSet has the expected author and snapshot id
    #[test]
    fn parse_minimal_changeset_succeeds() {
        let src = "change minimal\nauthor Alice\nbase 0\nend\n";
        let result = parse_changeset(src).expect("minimal changeset must parse");
        assert_eq!(result.changeset.meta.author, "Alice");
        assert_eq!(result.changeset.base_snapshot_id, SnapshotId(0));
        assert!(result.changeset.ops.is_empty(), "no ops expected");
        assert!(result.preconditions.is_empty(), "no preconditions expected");
    }

    // Scenario: description defaults to empty when absent.
    //   GIVEN no description or intent line
    //   WHEN parse_changeset is called
    //   THEN description is empty string
    #[test]
    fn parse_missing_description_defaults_to_empty() {
        let src = "change x\nauthor Bob\nbase 1\nend\n";
        let result = parse_changeset(src).expect("must parse");
        assert_eq!(result.changeset.meta.description, "");
    }

    // Scenario: intent line sets description.
    //   GIVEN `intent "Add cart total"`
    //   WHEN parse_changeset is called
    //   THEN description equals the unquoted content
    #[test]
    fn parse_intent_line_sets_description() {
        let src = "change x\nauthor Bob\nbase 1\nintent \"Add cart total\"\nend\n";
        let result = parse_changeset(src).expect("must parse");
        assert_eq!(result.changeset.meta.description, "Add cart total");
    }

    // Scenario: description line sets description.
    #[test]
    fn parse_description_line_sets_description() {
        let src = "change x\nauthor Bob\nbase 1\ndescription My change\nend\n";
        let result = parse_changeset(src).expect("must parse");
        assert_eq!(result.changeset.meta.description, "My change");
    }

    // Scenario: all 7 op verb prefix groups are mapped correctly.
    //   GIVEN one representative op for each of the 7 categories
    //   WHEN parse_changeset is called
    //   THEN ops vec contains the 7 variants in order
    #[test]
    fn parse_all_seven_op_categories() {
        let src = "\
change test
author Carol
base 2
op create_function id=fn.checkout
op set_return target=fn.checkout type=\"Result\"
op add_param target=fn.checkout name=x type=CartId
op remove_effect target=fn.checkout effect=io
op connect source=fn.checkout relation=uses target=cap.pay
op infer_boundary target=fn.checkout
op verify
end
";
        let result = parse_changeset(src).expect("all 7 ops must parse");
        assert_eq!(
            result.changeset.ops,
            vec![
                ChangeSetOp::Create,
                ChangeSetOp::Set,
                ChangeSetOp::Add,
                ChangeSetOp::Remove,
                ChangeSetOp::Connect,
                ChangeSetOp::Infer,
                ChangeSetOp::Verify,
            ]
        );
    }

    // Scenario: `delete` maps to Delete and `disconnect` maps to Disconnect.
    #[test]
    fn parse_delete_and_disconnect_map_to_own_variants() {
        let src = "change x\nauthor D\nbase 0\nop delete target=fn.old\nop disconnect source=a relation=r target=b\nend\n";
        let result = parse_changeset(src).expect("must parse");
        assert_eq!(
            result.changeset.ops,
            vec![ChangeSetOp::Delete, ChangeSetOp::Disconnect]
        );
    }

    // Scenario: ops inside `ops ... end` section form.
    //   GIVEN ops wrapped in an explicit `ops` section
    //   WHEN parsed
    //   THEN same result as short form
    #[test]
    fn parse_section_form_ops() {
        let src = "\
change x
author Eve
base 5
ops
  op create_function id=fn.checkout
  op set_return target=fn.checkout type=Unit
end
end
";
        let result = parse_changeset(src).expect("section form must parse");
        assert_eq!(
            result.changeset.ops,
            vec![ChangeSetOp::Create, ChangeSetOp::Set]
        );
        assert_eq!(result.changeset.base_snapshot_id, SnapshotId(5));
    }

    // Scenario: short form — ops directly under change, no `ops` section.
    #[test]
    fn parse_short_form_ops() {
        let src = "change x\nauthor Frank\nbase 3\nop create id=fn.x\nop verify\nend\n";
        let result = parse_changeset(src).expect("short form must parse");
        assert_eq!(
            result.changeset.ops,
            vec![ChangeSetOp::Create, ChangeSetOp::Verify]
        );
    }

    // Scenario: `requires` block with assert_exists.
    //   GIVEN `assert_exists <node_id>` inside a requires block
    //   WHEN parsed
    //   THEN preconditions contains AssertExists with the correct node id
    #[test]
    fn parse_requires_assert_exists() {
        let src = "\
change x
author Grace
base 0
requires
  assert_exists 42
end
end
";
        let result = parse_changeset(src).expect("assert_exists must parse");
        assert_eq!(result.preconditions.len(), 1);
        let crate::canonical::Precondition::AssertExists(ae) = &result.preconditions[0] else {
            panic!("expected AssertExists precondition");
        };
        assert_eq!(ae.node_id, NodeRef(42));
    }

    // Scenario: `requires` block with assert_hash.
    //   GIVEN `assert_hash <node_id> sig=<64 hex chars>`
    //   WHEN parsed
    //   THEN preconditions contains AssertHash with the correct node id and decoded hash
    #[test]
    fn parse_requires_assert_hash() {
        let hex = "a".repeat(64);
        let src = format!(
            "change x\nauthor Hank\nbase 0\nrequires\n  assert_hash 7 sig={hex}\nend\nend\n"
        );
        let result = parse_changeset(&src).expect("assert_hash must parse");
        assert_eq!(result.preconditions.len(), 1);
        let crate::canonical::Precondition::AssertHash(ah) = &result.preconditions[0] else {
            panic!("expected AssertHash precondition");
        };
        assert_eq!(ah.node_id, NodeRef(7));
        // 0xaa repeated 32 times.
        assert_eq!(ah.expected_hash.0, [0xaa_u8; 32]);
    }

    // Scenario: metadata block sets author and description.
    //   GIVEN a `metadata ... end` block with author and description
    //   WHEN parsed
    //   THEN author and description are correctly set
    #[test]
    fn parse_metadata_block() {
        let src = "\
change x
base 0
metadata
  author Iris
  description From metadata block
end
end
";
        let result = parse_changeset(src).expect("metadata block must parse");
        assert_eq!(result.changeset.meta.author, "Iris");
        assert_eq!(result.changeset.meta.description, "From metadata block");
    }

    // Scenario: comments and blank lines are ignored.
    //   GIVEN a document with # comments and blank lines interspersed
    //   WHEN parsed
    //   THEN parse succeeds and content lines are processed normally
    #[test]
    fn parse_ignores_comments_and_blanks() {
        let src = "\
# this is a preamble comment
change x

# set metadata
author Jack
base 3

# one op
op create_function id=fn.x

end
";
        let result = parse_changeset(src).expect("comments and blanks must be ignored");
        assert_eq!(result.changeset.meta.author, "Jack");
        assert_eq!(result.changeset.base_snapshot_id, SnapshotId(3));
        assert_eq!(result.changeset.ops, vec![ChangeSetOp::Create]);
    }

    // Scenario: missing author → ParseError.
    #[test]
    fn parse_missing_author_returns_error() {
        let src = "change x\nbase 0\nend\n";
        let err = parse_changeset(src).expect_err("missing author must error");
        assert!(
            err.contains("author"),
            "error must mention 'author'; got: {err}"
        );
    }

    // Scenario: missing base → ParseError.
    #[test]
    fn parse_missing_base_returns_error() {
        let src = "change x\nauthor Kim\nend\n";
        let err = parse_changeset(src).expect_err("missing base must error");
        assert!(
            err.contains("base"),
            "error must mention 'base'; got: {err}"
        );
    }

    // Scenario: invalid base (non-u64) → ParseError.
    #[test]
    fn parse_invalid_base_returns_error() {
        let src = "change x\nauthor Lee\nbase not_a_number\nend\n";
        let err = parse_changeset(src).expect_err("invalid base must error");
        assert!(
            err.contains("invalid base snapshot id"),
            "error must describe the problem; got: {err}"
        );
    }

    // Scenario: unknown op verb → ParseError.
    #[test]
    fn parse_unknown_op_verb_returns_error() {
        let src = "change x\nauthor Mia\nbase 0\nop frobnicate target=fn.x\nend\n";
        let err = parse_changeset(src).expect_err("unknown op verb must error");
        assert!(
            err.contains("frobnicate"),
            "error must name the unknown verb; got: {err}"
        );
    }

    // Scenario: assert_hash with wrong hex length → ParseError.
    #[test]
    fn parse_assert_hash_wrong_length_returns_error() {
        let src =
            "change x\nauthor Ned\nbase 0\nrequires\n  assert_hash 1 sig=deadbeef\nend\nend\n";
        let err = parse_changeset(src).expect_err("short hex must error");
        assert!(
            err.contains("64 hex characters"),
            "error must describe hex length; got: {err}"
        );
    }

    // Scenario: assert_hash with missing sig= → ParseError.
    #[test]
    fn parse_assert_hash_missing_sig_returns_error() {
        let src = "change x\nauthor Ned\nbase 0\nrequires\n  assert_hash 1\nend\nend\n";
        let err = parse_changeset(src).expect_err("missing sig must error");
        assert!(err.contains("sig"), "error must mention 'sig'; got: {err}");
    }

    // Scenario: extract_string_value strips quotes.
    #[test]
    fn extract_string_value_strips_quotes() {
        assert_eq!(extract_string_value("\"hello world\""), "hello world");
        assert_eq!(extract_string_value("bare"), "bare");
        assert_eq!(extract_string_value("\"\""), "");
    }

    // Scenario: extract_kv_value finds the right key.
    #[test]
    fn extract_kv_value_finds_key() {
        assert_eq!(
            extract_kv_value("target=fn.x type=Int", "target"),
            Some("fn.x".to_string())
        );
        assert_eq!(
            extract_kv_value("target=fn.x type=Int", "type"),
            Some("Int".to_string())
        );
        assert_eq!(extract_kv_value("target=fn.x", "missing"), None);
    }

    #[test]
    fn parse_kv_args_keeps_parenthesized_body_with_spaces() {
        let args = parse_kv_args("target=fn.add body=add(x, y) return=Int");

        assert_eq!(args.get("target").map(String::as_str), Some("fn.add"));
        assert_eq!(args.get("body").map(String::as_str), Some("add(x, y)"));
        assert_eq!(args.get("return").map(String::as_str), Some("Int"));
    }

    // ── Gap 3: Set/list kv grammar ────────────────────────────────────────

    // Scenario: set literal value `{a,b}` is captured whole.
    //   GIVEN `effects={database.read:Cart,payment.charge:PaymentProvider}`
    //   WHEN parse_kv_args is called
    //   THEN `effects` maps to the full set literal string
    #[test]
    fn parse_kv_args_set_literal_captured_whole() {
        let args = parse_kv_args(
            "target=fn.checkout effects={database.read:Cart,payment.charge:PaymentProvider}",
        );
        assert_eq!(
            args.get("target").map(String::as_str),
            Some("fn.checkout")
        );
        assert_eq!(
            args.get("effects").map(String::as_str),
            Some("{database.read:Cart,payment.charge:PaymentProvider}")
        );
    }

    // Scenario: list literal value `[a,b]` is captured whole.
    //   GIVEN `items=[one,two,three]`
    //   WHEN parse_kv_args is called
    //   THEN `items` maps to the full list literal string
    #[test]
    fn parse_kv_args_list_literal_captured_whole() {
        let args = parse_kv_args("items=[one,two,three] other=value");
        assert_eq!(
            args.get("items").map(String::as_str),
            Some("[one,two,three]")
        );
        assert_eq!(args.get("other").map(String::as_str), Some("value"));
    }

    // TRIANGULATE: nested set `{a,{b,c}}` is captured with inner braces intact.
    #[test]
    fn parse_kv_args_nested_set_literal_captured_whole() {
        let args = parse_kv_args("val={a,{b,c}} key=x");
        assert_eq!(
            args.get("val").map(String::as_str),
            Some("{a,{b,c}}")
        );
    }

    // ── Gap 4: assert_context precondition ────────────────────────────────

    // Scenario: `assert_context` with target and hash is parsed.
    //   GIVEN `assert_context fn.checkout hash=abc123`
    //   WHEN parsed
    //   THEN preconditions contains AssertContext with target and context_hash
    #[test]
    fn parse_assert_context_with_hash() {
        let src = "\
change x
author A
base 0
requires
  assert_context fn.checkout hash=abc123
end
end
";
        let result = parse_changeset(src).expect("assert_context must parse");
        assert_eq!(result.preconditions.len(), 1);
        match &result.preconditions[0] {
            crate::canonical::Precondition::AssertContext {
                target_name,
                context_hash,
            } => {
                assert_eq!(target_name, "fn.checkout");
                assert_eq!(context_hash.as_deref(), Some("abc123"));
            }
            other => panic!("expected AssertContext, got {other:?}"),
        }
    }

    // Scenario: `assert_context` without hash is parsed (target-only form).
    #[test]
    fn parse_assert_context_target_only() {
        let src = "\
change x
author A
base 0
requires
  assert_context type.Cart
end
end
";
        let result = parse_changeset(src).expect("assert_context target-only must parse");
        match &result.preconditions[0] {
            crate::canonical::Precondition::AssertContext {
                target_name,
                context_hash,
            } => {
                assert_eq!(target_name, "type.Cart");
                assert!(context_hash.is_none());
            }
            other => panic!("expected AssertContext, got {other:?}"),
        }
    }

    // ── Gap 5: Named NodeRefs in assert_exists / assert_hash ─────────────

    // Scenario: `assert_exists type.Cart` is parsed as AssertExistsByName.
    //   GIVEN `assert_exists type.Cart` inside a requires section
    //   WHEN parsed
    //   THEN preconditions contains AssertExistsByName("type.Cart")
    #[test]
    fn parse_assert_exists_named_node_ref() {
        let src = "\
change x
author A
base 0
requires
  assert_exists type.Cart
  assert_exists fn.cart_total
end
end
";
        let result = parse_changeset(src).expect("named assert_exists must parse");
        assert_eq!(result.preconditions.len(), 2);
        match &result.preconditions[0] {
            crate::canonical::Precondition::AssertExistsByName(name) => {
                assert_eq!(name, "type.Cart");
            }
            other => panic!("expected AssertExistsByName, got {other:?}"),
        }
        match &result.preconditions[1] {
            crate::canonical::Precondition::AssertExistsByName(name) => {
                assert_eq!(name, "fn.cart_total");
            }
            other => panic!("expected AssertExistsByName, got {other:?}"),
        }
    }

    // Scenario: `assert_hash fn.cart_total sig=<hex>` is parsed as AssertHashByName.
    //   GIVEN `assert_hash fn.cart_total sig=<64-hex-chars>`
    //   WHEN parsed
    //   THEN preconditions contains AssertHashByName with the correct name and hash
    #[test]
    fn parse_assert_hash_named_node_ref() {
        let hex = "b".repeat(64);
        let src = format!(
            "change x\nauthor A\nbase 0\nrequires\n  assert_hash fn.cart_total sig={hex}\nend\nend\n"
        );
        let result = parse_changeset(&src).expect("named assert_hash must parse");
        assert_eq!(result.preconditions.len(), 1);
        match &result.preconditions[0] {
            crate::canonical::Precondition::AssertHashByName { name, expected_hash } => {
                assert_eq!(name, "fn.cart_total");
                assert_eq!(expected_hash.0, [0xbb_u8; 32]);
            }
            other => panic!("expected AssertHashByName, got {other:?}"),
        }
    }

    // TRIANGULATE: numeric assert_exists still works (backward-compatible).
    #[test]
    fn parse_assert_exists_numeric_still_works() {
        let src = "\
change x
author A
base 0
requires
  assert_exists 42
end
end
";
        let result = parse_changeset(src).expect("numeric assert_exists must parse");
        let crate::canonical::Precondition::AssertExists(ae) = &result.preconditions[0] else {
            panic!("expected AssertExists, got {:?}", result.preconditions[0]);
        };
        assert_eq!(ae.node_id, NodeRef(42));
    }

    // ── Gap 2: Schema versioning fields ───────────────────────────────────

    // Scenario: all five schema version directives in metadata section are parsed.
    //   GIVEN a metadata section with all five schema version fields
    //   WHEN parsed
    //   THEN ParsedChangeSet carries correct versions for each field
    #[test]
    fn parse_all_schema_version_fields() {
        let src = "\
change test_versions
author Agent
base 0
metadata
  acl_version acl/1.0
  op_schema 1
  graph_schema 3
  core_ir_schema 2
  diagnostics_schema 1
  verification_schema 1
end
end
";
        let result = parse_changeset(src).expect("schema versions must parse");
        assert_eq!(result.acl_version, "1.0");
        assert_eq!(result.op_schema_version.as_deref(), Some("1"));
        assert_eq!(result.graph_schema_version.as_deref(), Some("3"));
        assert_eq!(result.core_ir_schema_version.as_deref(), Some("2"));
        assert_eq!(result.diagnostics_schema_version.as_deref(), Some("1"));
        assert_eq!(result.verification_schema_version.as_deref(), Some("1"));
    }

    // Scenario: schema version fields default to None when absent.
    #[test]
    fn parse_schema_versions_default_to_none_when_absent() {
        let src = "change x\nauthor A\nbase 0\nend\n";
        let result = parse_changeset(src).expect("must parse");
        assert!(result.op_schema_version.is_none());
        assert!(result.graph_schema_version.is_none());
        assert!(result.core_ir_schema_version.is_none());
        assert!(result.diagnostics_schema_version.is_none());
        assert!(result.verification_schema_version.is_none());
    }

    // TRIANGULATE: schema versions are carried through canonicalize_parsed.
    #[test]
    fn schema_versions_carried_through_canonicalize() {
        use crate::canonical::canonicalize_parsed;

        let src = "\
change test
author tester
base 0
metadata
  graph_schema 5
  core_ir_schema 3
end
end
";
        let parsed = parse_changeset(src).expect("must parse");
        let canonical = canonicalize_parsed(parsed);
        assert_eq!(canonical.graph_schema_version.as_deref(), Some("5"));
        assert_eq!(canonical.core_ir_schema_version.as_deref(), Some("3"));
        assert!(canonical.op_schema_version.is_none());
    }

    // Scenario: identity changeset (no ops) parses successfully.
    #[test]
    fn parse_identity_changeset_is_valid() {
        let src = "change x\nauthor Olivia\nbase 99\nend\n";
        let result = parse_changeset(src).expect("identity changeset must parse");
        assert!(result.changeset.ops.is_empty());
        assert_eq!(result.changeset.base_snapshot_id, SnapshotId(99));
    }

    // Scenario: multiple requires assertions are all captured.
    #[test]
    fn parse_multiple_preconditions() {
        let src = "\
change x
author Paula
base 0
requires
  assert_exists 1
  assert_exists 2
  assert_exists 3
end
end
";
        let result = parse_changeset(src).expect("multiple preconditions must parse");
        assert_eq!(result.preconditions.len(), 3);
    }

    // Scenario: unclosed section returns ParseError.
    #[test]
    fn parse_unclosed_section_returns_error() {
        let src = "change x\nauthor Quinn\nbase 0\nops\nop create_function id=fn.x\n";
        let err = parse_changeset(src).expect_err("unclosed section must error");
        assert!(
            err.contains("unclosed"),
            "error must say 'unclosed'; got: {err}"
        );
    }

    // TRIANGULATE: bare `create`, `set`, `add`, `infer` (no underscore suffix) map correctly.
    #[test]
    fn parse_bare_verb_variants() {
        let src = "change x\nauthor R\nbase 0\nop create\nop set\nop add\nop infer\nend\n";
        let result = parse_changeset(src).expect("bare verbs must parse");
        assert_eq!(
            result.changeset.ops,
            vec![
                ChangeSetOp::Create,
                ChangeSetOp::Set,
                ChangeSetOp::Add,
                ChangeSetOp::Infer,
            ]
        );
    }

    // Scenario: all 20 new verbs (phase 1-4 additions) parse to their own variants.
    //   GIVEN one representative op for each new verb
    //   WHEN parse_changeset is called
    //   THEN each verb maps to its dedicated ChangeSetOp variant
    #[test]
    fn parse_all_new_verb_variants() {
        let src = "\
change test_new_verbs
author TestAgent
base 0
op delete target=fn.old
op disconnect source=fn.a relation=uses target=cap.b
op rename target=fn.old name=fn.new
op move target=fn.util to=module.utils
op replace target=fn.checkout.body with=@expr.v2
op bind_handler capability=payment.charge handler=handler.Stripe profile=prod
op expose target=fn.checkout as=api.checkout
op hide target=fn.internal_helper
op grant target=module.checkout capability=database.read profile=prod
op revoke target=module.checkout capability=file.write profile=prod
op derive_eq target=type.Address mode=structural
op generate_tests target=fn.checkout from=contracts
op assert_exists target=fn.checkout
op lock_behavior target=fn.checkout
op refactor_extract_function from=fn.checkout range=@range.payment to=fn.charge
op migrate_api target=fn.checkout from=sig.v1 to=sig.v2
op approve_inferred_boundary target=fn.checkout version=sig_123
op reject_inferred_boundary target=fn.checkout version=sig_124
op deprecate target=fn.old_checkout replacement=fn.checkout_v2
op annotate target=fn.checkout key=rationale value=\"Checkout must be idempotent\"
end
";
        let result = parse_changeset(src).expect("all new verbs must parse");
        assert_eq!(
            result.changeset.ops,
            vec![
                ChangeSetOp::Delete,
                ChangeSetOp::Disconnect,
                ChangeSetOp::Rename,
                ChangeSetOp::Move,
                ChangeSetOp::Replace,
                ChangeSetOp::Bind,
                ChangeSetOp::Expose,
                ChangeSetOp::Hide,
                ChangeSetOp::Grant,
                ChangeSetOp::Revoke,
                ChangeSetOp::Derive,
                ChangeSetOp::Generate,
                ChangeSetOp::Assert,
                ChangeSetOp::Lock,
                ChangeSetOp::Refactor,
                ChangeSetOp::Migrate,
                ChangeSetOp::Approve,
                ChangeSetOp::Reject,
                ChangeSetOp::Deprecate,
                ChangeSetOp::Annotate,
            ]
        );
    }

    // Scenario: bare new verbs (no underscore suffix) also map correctly.
    #[test]
    fn parse_bare_new_verb_variants() {
        let src = "\
change x
author S
base 0
op delete
op disconnect
op rename
op move
op replace
op bind
op expose
op hide
op grant
op revoke
op derive
op generate
op assert
op lock
op refactor
op migrate
op approve
op reject
op deprecate
op annotate
end
";
        let result = parse_changeset(src).expect("bare new verbs must parse");
        assert_eq!(
            result.changeset.ops,
            vec![
                ChangeSetOp::Delete,
                ChangeSetOp::Disconnect,
                ChangeSetOp::Rename,
                ChangeSetOp::Move,
                ChangeSetOp::Replace,
                ChangeSetOp::Bind,
                ChangeSetOp::Expose,
                ChangeSetOp::Hide,
                ChangeSetOp::Grant,
                ChangeSetOp::Revoke,
                ChangeSetOp::Derive,
                ChangeSetOp::Generate,
                ChangeSetOp::Assert,
                ChangeSetOp::Lock,
                ChangeSetOp::Refactor,
                ChangeSetOp::Migrate,
                ChangeSetOp::Approve,
                ChangeSetOp::Reject,
                ChangeSetOp::Deprecate,
                ChangeSetOp::Annotate,
            ]
        );
    }
}
