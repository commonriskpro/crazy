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
// section_name    = "metadata" | "requires" | "ops"
// section_body    = (directive | op_line | precondition | comment | blank)*
// precondition    = ("assert_exists" | "assert_hash") ws args nl
// kv              = key "=" value
// value           = quoted_string | bare_word
// ```
//
// # Verb → ChangeSetOp mapping
//
// | Prefix / exact match         | Variant  |
// |------------------------------|----------|
// | `create` / `create_*`        | Create   |
// | `set` / `set_*`              | Set      |
// | `add` / `add_*`              | Add      |
// | `remove` / `remove_*` / `disconnect` / `delete` | Remove |
// | `connect`                    | Connect  |
// | `infer` / `infer_*`          | Infer    |
// | `verify`                     | Verify   |
//
// # Pure function
//
// `parse_changeset` is a pure function: it takes `&str` and returns a `Result`.

use ail_core::semantic_graph::NodeRef;

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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedChangeSet {
    /// The typed changeset (ops + metadata + base snapshot).
    pub changeset: ChangeSet,
    /// Preconditions declared in the `requires` section.
    pub preconditions: Vec<Precondition>,
}

// ── Section state ─────────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Eq)]
enum Section {
    TopLevel,
    Metadata,
    Requires,
    Ops,
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
    let mut ops: Vec<ChangeSetOp> = Vec::new();
    let mut preconditions: Vec<Precondition> = Vec::new();

    let mut section = Section::TopLevel;
    let mut line_num: usize = 0;

    // Tracks whether we are inside the outer `change ... end` block.
    // Lines before `change` and after the closing `end` are accepted if blank/comment.
    let mut in_change = false;

    for raw in src.lines() {
        line_num += 1;
        let line = raw.trim();

        // Always skip blanks and comments.
        if line.is_empty() || line.starts_with('#') {
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
            continue;
        }

        // Top-level `end` closes the change block.
        if line == "end" {
            match section {
                Section::Metadata | Section::Requires | Section::Ops => {
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
            _ => {}
        }

        // Dispatch by current section.
        match section {
            Section::Metadata => {
                parse_metadata_line(line, line_num, &mut author, &mut description, &mut base)?;
            }
            Section::Requires => {
                parse_precondition_line(line, line_num, &mut preconditions)?;
            }
            Section::TopLevel | Section::Ops => {
                parse_op_or_directive(
                    line,
                    line_num,
                    &mut ops,
                    &mut author,
                    &mut description,
                    &mut base,
                    &section,
                )?;
            }
        }
    }

    // Check for unclosed sections.
    if section != Section::TopLevel {
        return Err(format!(
            "unclosed section: {}",
            match section {
                Section::Metadata => "metadata",
                Section::Requires => "requires",
                Section::Ops => "ops",
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
    })
}

// ── parse_metadata_line ───────────────────────────────────────────────────

/// Parse a directive that supplies metadata (author, description, base, intent).
///
/// Called when section is `TopLevel` or `Metadata`.
fn parse_metadata_line(
    line: &str,
    line_num: usize,
    author: &mut Option<String>,
    description: &mut Option<String>,
    base: &mut Option<SnapshotId>,
) -> Result<(), String> {
    if let Some(v) = line.strip_prefix("author ") {
        *author = Some(v.trim().to_string());
    } else if let Some(v) = line.strip_prefix("description ") {
        *description = Some(v.trim().to_string());
    } else if let Some(v) = line.strip_prefix("intent ") {
        *description = Some(extract_string_value(v.trim()));
    } else if let Some(v) = line.strip_prefix("base ") {
        let raw = v.trim();
        let id: u64 = raw
            .parse()
            .map_err(|_| format!("line {line_num}: invalid base snapshot id: '{raw}'"))?;
        *base = Some(SnapshotId(id));
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
fn parse_op_or_directive(
    line: &str,
    line_num: usize,
    ops: &mut Vec<ChangeSetOp>,
    author: &mut Option<String>,
    description: &mut Option<String>,
    base: &mut Option<SnapshotId>,
    section: &Section,
) -> Result<(), String> {
    if let Some(rest) = line.strip_prefix("op ") {
        // Extract the verb (first token after "op ").
        let verb = rest.split_whitespace().next().unwrap_or("");
        let op =
            map_verb(verb).ok_or_else(|| format!("line {line_num}: unknown op verb: '{verb}'"))?;
        ops.push(op);
    } else if *section == Section::TopLevel {
        // Allow metadata directives at the top level.
        parse_metadata_line(line, line_num, author, description, base)?;
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
/// - `assert_exists <node_id_u32>`
/// - `assert_hash <node_id_u32> sig=<64-hex-chars>`
fn parse_precondition_line(
    line: &str,
    line_num: usize,
    preconditions: &mut Vec<Precondition>,
) -> Result<(), String> {
    if let Some(rest) = line.strip_prefix("assert_exists ") {
        let node_id = parse_node_ref(rest.trim(), line_num)?;
        preconditions.push(Precondition::AssertExists(AssertExists { node_id }));
    } else if let Some(rest) = line.strip_prefix("assert_hash ") {
        // Expected format: `<node_id> sig=<hex>`
        let mut parts = rest.splitn(2, ' ');
        let id_part = parts.next().unwrap_or("").trim();
        let kv_part = parts.next().unwrap_or("").trim();

        let node_id = parse_node_ref(id_part, line_num)?;
        let hex = extract_kv_value(kv_part, "sig")
            .ok_or_else(|| format!("line {line_num}: assert_hash requires 'sig=<hex>' argument"))?;
        let expected_hash = decode_hex32(&hex, line_num)?;
        preconditions.push(Precondition::AssertHash(AssertHash {
            node_id,
            expected_hash,
        }));
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
    if verb == "create" || verb.starts_with("create_") {
        Some(ChangeSetOp::Create)
    } else if verb == "set" || verb.starts_with("set_") {
        Some(ChangeSetOp::Set)
    } else if verb == "add" || verb.starts_with("add_") {
        Some(ChangeSetOp::Add)
    } else if verb == "remove"
        || verb.starts_with("remove_")
        || verb == "disconnect"
        || verb == "delete"
    {
        Some(ChangeSetOp::Remove)
    } else if verb == "connect" {
        Some(ChangeSetOp::Connect)
    } else if verb == "infer" || verb.starts_with("infer_") {
        Some(ChangeSetOp::Infer)
    } else if verb == "verify" {
        Some(ChangeSetOp::Verify)
    } else {
        None
    }
}

// ── helpers ───────────────────────────────────────────────────────────────

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

/// Parse a `NodeRef` from a string containing a `u32`.
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

    // Scenario: `delete` and `disconnect` map to Remove.
    #[test]
    fn parse_delete_and_disconnect_map_to_remove() {
        let src = "change x\nauthor D\nbase 0\nop delete target=fn.old\nop disconnect source=a relation=r target=b\nend\n";
        let result = parse_changeset(src).expect("must parse");
        assert_eq!(
            result.changeset.ops,
            vec![ChangeSetOp::Remove, ChangeSetOp::Remove]
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
}
