// ── ail-cli::changeset_input ─────────────────────────────────────────────
// `ChangeInput` and `load_changeset` are wired into the `change` command.
#![allow(dead_code)]
//
// Line-oriented `ChangeSet` loader for file and stdin input.
//
// Delegates all parsing to `ail_change::parser::parse_changeset`, which
// supports the full ACL DSL including sections (`metadata`, `requires`, `ops`),
// preconditions (`assert_exists`, `assert_hash`), and the complete op-verb
// prefix mapping.
//
// # I/O wrapper
//
// `load_changeset` is the only I/O entry-point here.  The underlying
// `parse_changeset` (in `ail-change`) is a pure function.

use std::io::Read;
use std::path::PathBuf;

use ail_change::model::ChangeSet;

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

// ── parse_changeset ───────────────────────────────────────────────────────

/// Parse an ACL document from a string into a `ChangeSet`.
///
/// Delegates to `ail_change::parser::parse_changeset` and maps the error
/// type to `CliError::ParseError`.
///
/// Preconditions from a `requires` section are discarded here — the CLI
/// changeset pipeline passes the `ChangeSet` through canonicalization, which
/// re-attaches preconditions via `ParsedChangeSet`.
///
/// # Errors
///
/// Returns `CliError::ParseError` on any ACL grammar violation.
pub fn parse_changeset(src: &str) -> Result<ChangeSet, CliError> {
    ail_change::parser::parse_changeset(src)
        .map(|parsed| parsed.changeset)
        .map_err(CliError::ParseError)
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ail_change::model::{ChangeSetOp, SnapshotId};

    // Scenario: minimal valid input parses correctly.
    //   GIVEN a well-formed ACL ChangeSet string
    //   WHEN parse_changeset is called
    //   THEN the returned ChangeSet has the expected author
    #[test]
    fn parse_minimal_changeset_succeeds() {
        let src = "change minimal\nauthor Alice\ndescription test change\nbase 0\nop create_function id=fn.x\nend\n";
        let cs = parse_changeset(src).expect("minimal changeset must parse successfully");
        assert_eq!(
            cs.meta.author, "Alice",
            "author must be parsed from 'author' line"
        );
    }

    // TRIANGULATE: all op categories parse correctly.
    //   GIVEN a changeset with one op for each of the 7 categories
    //   WHEN parse_changeset is called
    //   THEN the ops vec contains all seven variants in order
    #[test]
    fn parse_all_op_variants() {
        let src = "\
change test
author Bob
base 1
op create_function id=fn.x
op set_return target=fn.x type=Unit
op add_param target=fn.x name=a type=Int
op remove_effect target=fn.x effect=io
op connect source=fn.x relation=uses target=cap.y
op infer_boundary target=fn.x
op verify
end
";
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
        let src = "change x\nauthor Carol\nbase 5\nend\n";
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
        let src = "change x\nbase 0\nop create id=fn.x\nend\n";
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
        let src = "change x\nauthor Dave\nop create id=fn.x\nend\n";
        let result = parse_changeset(src);
        assert!(
            matches!(result, Err(CliError::ParseError(_))),
            "missing base must return ParseError; got: {result:?}"
        );
    }

    // TRIANGULATE: unknown op verb returns an error.
    //   GIVEN a changeset with an unrecognised op verb
    //   WHEN parse_changeset is called
    //   THEN Err(CliError::ParseError) is returned
    #[test]
    fn parse_unknown_op_returns_error() {
        let src = "change x\nauthor Eve\nbase 0\nop frobnicate target=fn.x\nend\n";
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
        let src = "\n# this is a comment\nchange x\nauthor Frank\n\nbase 3\n# another comment\nop set_return target=fn.x type=Unit\nend\n";
        let cs = parse_changeset(src).expect("blank lines and comments must be ignored");
        assert_eq!(cs.meta.author, "Frank");
        assert_eq!(cs.base_snapshot_id, SnapshotId(3));
        assert_eq!(cs.ops, vec![ChangeSetOp::Set]);
    }
}
