// ── ail-cli::changeset_input ─────────────────────────────────────────────
// `ChangeInput` and `load_changeset` are wired into the `change` command in PR2.
#![allow(dead_code)]
//
// Line-oriented `ChangeSet` loader for file and stdin input.
//
// # Line format
//
// ```text
// author   <name>
// description <text>
// base     <u64 snapshot id>
// op       Create | Set | Add | Remove | Connect | Infer | Verify
// ```
//
// Blank lines and lines starting with `#` are ignored.
// `description` is optional; all other fields are required.
// Multiple `op` lines are collected in order.
//
// # Pure function
//
// `parse_changeset` is a pure function: it takes `&str` and returns a `Result`.
// `load_changeset` is the I/O wrapper that reads from a file or stdin.

use std::io::Read;
use std::path::PathBuf;

use ail_change::model::{ChangeSet, ChangeSetMeta, ChangeSetOp, SnapshotId, Timestamp};

use crate::error::CliError;

// ── ChangeInput ───────────────────────────────────────────────────────────

/// Input source for a `ChangeSet` document.
pub enum ChangeInput {
    /// Read from a file at the given path.
    File(PathBuf),
    /// Read from standard input.
    Stdin,
}

// ── load_changeset ────────────────────────────────────────────────────────

/// Load and parse a `ChangeSet` from the given input source.
///
/// Returns `Err(CliError::Io(_))` on read failures and
/// `Err(CliError::ParseError(_))` on format violations.
pub fn load_changeset(input: ChangeInput) -> Result<ChangeSet, CliError> {
    let content = match input {
        ChangeInput::File(path) => std::fs::read_to_string(path)?,
        ChangeInput::Stdin => {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            buf
        }
    };
    parse_changeset(&content)
}

// ── parse_changeset (pure) ────────────────────────────────────────────────

/// Parse a line-oriented `ChangeSet` document from a string.
///
/// This is a **pure function**: no I/O, no side effects.
///
/// # Errors
///
/// Returns `CliError::ParseError` if:
/// - `author` is missing
/// - `base` is missing or not a valid `u64`
/// - An `op` name is not one of the seven canonical ops
/// - An unrecognised directive is encountered
pub fn parse_changeset(src: &str) -> Result<ChangeSet, CliError> {
    let mut author: Option<String> = None;
    let mut description: Option<String> = None;
    let mut base: Option<SnapshotId> = None;
    let mut ops: Vec<ChangeSetOp> = Vec::new();

    for line in src.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(v) = line.strip_prefix("author ") {
            author = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("description ") {
            description = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("base ") {
            let id: u64 = v
                .trim()
                .parse()
                .map_err(|_| CliError::ParseError(format!("invalid base snapshot id: '{v}'")))?;
            base = Some(SnapshotId(id));
        } else if let Some(v) = line.strip_prefix("op ") {
            let op = parse_op(v.trim())?;
            ops.push(op);
        } else {
            return Err(CliError::ParseError(format!(
                "unrecognised directive: '{line}'"
            )));
        }
    }

    let author =
        author.ok_or_else(|| CliError::ParseError("missing required field: author".to_string()))?;
    let base_snapshot_id =
        base.ok_or_else(|| CliError::ParseError("missing required field: base".to_string()))?;
    let description = description.unwrap_or_default();

    Ok(ChangeSet {
        meta: ChangeSetMeta {
            author,
            description,
            // The CLI does not set the timestamp; downstream canonicalization
            // may fill this in. Using epoch zero as a neutral sentinel.
            timestamp: Timestamp(0),
        },
        base_snapshot_id,
        ops,
    })
}

// ── parse_op (pure, private) ──────────────────────────────────────────────

/// Parse a single op name into a `ChangeSetOp` variant.
fn parse_op(name: &str) -> Result<ChangeSetOp, CliError> {
    match name {
        "Create" => Ok(ChangeSetOp::Create),
        "Set" => Ok(ChangeSetOp::Set),
        "Add" => Ok(ChangeSetOp::Add),
        "Remove" => Ok(ChangeSetOp::Remove),
        "Connect" => Ok(ChangeSetOp::Connect),
        "Infer" => Ok(ChangeSetOp::Infer),
        "Verify" => Ok(ChangeSetOp::Verify),
        other => Err(CliError::ParseError(format!("unknown op: '{other}'"))),
    }
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ail_change::model::ChangeSetOp;

    // Scenario: minimal valid input parses correctly.
    //   GIVEN a well-formed line-oriented ChangeSet string
    //   WHEN parse_changeset is called
    //   THEN the returned ChangeSet has the expected author
    #[test]
    fn parse_minimal_changeset_succeeds() {
        let src = "author Alice\ndescription test change\nbase 0\nop Create\n";
        let cs = parse_changeset(src).expect("minimal changeset must parse successfully");
        assert_eq!(
            cs.meta.author, "Alice",
            "author must be parsed from 'author' line"
        );
    }

    // TRIANGULATE: all op variants parse correctly.
    //   GIVEN a changeset with all seven op types
    //   WHEN parse_changeset is called
    //   THEN the ops vec contains all seven variants in order
    #[test]
    fn parse_all_op_variants() {
        let src = "author Bob\nbase 1\n\
            op Create\nop Set\nop Add\nop Remove\nop Connect\nop Infer\nop Verify\n";
        let cs = parse_changeset(src).expect("all ops must parse");
        assert_eq!(
            cs.ops,
            vec![
                ChangeSetOp::Create,
                ChangeSetOp::Set,
                ChangeSetOp::Add,
                ChangeSetOp::Remove,
                ChangeSetOp::Connect,
                ChangeSetOp::Infer,
                ChangeSetOp::Verify,
            ],
            "all seven op variants must be parsed in order"
        );
    }

    // TRIANGULATE: empty ops vec is valid.
    //   GIVEN a changeset with no op lines
    //   WHEN parse_changeset is called
    //   THEN the ops vec is empty (identity changeset)
    #[test]
    fn parse_changeset_with_no_ops_is_valid() {
        let src = "author Carol\nbase 5\n";
        let cs = parse_changeset(src).expect("identity changeset must parse");
        assert!(
            cs.ops.is_empty(),
            "changeset with no ops must have empty ops vec"
        );
        assert_eq!(cs.base_snapshot_id, SnapshotId(5));
    }

    // TRIANGULATE: missing author returns an error.
    //   GIVEN a changeset with no author line
    //   WHEN parse_changeset is called
    //   THEN Err(CliError::ParseError) is returned
    #[test]
    fn parse_missing_author_returns_error() {
        let src = "base 0\nop Create\n";
        let result = parse_changeset(src);
        assert!(
            matches!(result, Err(CliError::ParseError(_))),
            "missing author must return ParseError; got: {result:?}"
        );
    }

    // TRIANGULATE: missing base returns an error.
    //   GIVEN a changeset with no base line
    //   WHEN parse_changeset is called
    //   THEN Err(CliError::ParseError) is returned
    #[test]
    fn parse_missing_base_returns_error() {
        let src = "author Dave\nop Create\n";
        let result = parse_changeset(src);
        assert!(
            matches!(result, Err(CliError::ParseError(_))),
            "missing base must return ParseError; got: {result:?}"
        );
    }

    // TRIANGULATE: unknown op name returns an error.
    //   GIVEN a changeset with an unrecognised op name
    //   WHEN parse_changeset is called
    //   THEN Err(CliError::ParseError) is returned
    #[test]
    fn parse_unknown_op_returns_error() {
        let src = "author Eve\nbase 0\nop Frobnicate\n";
        let result = parse_changeset(src);
        assert!(
            matches!(result, Err(CliError::ParseError(_))),
            "unknown op must return ParseError; got: {result:?}"
        );
    }

    // TRIANGULATE: blank lines and comments are ignored.
    //   GIVEN a changeset with blank lines and '#' comment lines
    //   WHEN parse_changeset is called
    //   THEN the parse succeeds and only content lines are processed
    #[test]
    fn parse_ignores_blank_lines_and_comments() {
        let src = "\n# this is a comment\nauthor Frank\n\nbase 3\n# another comment\nop Set\n";
        let cs = parse_changeset(src).expect("blank lines and comments must be ignored");
        assert_eq!(cs.meta.author, "Frank");
        assert_eq!(cs.base_snapshot_id, SnapshotId(3));
        assert_eq!(cs.ops, vec![ChangeSetOp::Set]);
    }
}
