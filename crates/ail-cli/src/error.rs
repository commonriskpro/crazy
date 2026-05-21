// ── ail-cli::error ────────────────────────────────────────────────────────
//
// `CliError` is the single error type for all CLI operations.
//
// # Variants
//
// `Io`              — wraps `std::io::Error` for file/stdin read failures.
// `ParseError`      — structured text describing what could not be parsed.
// `NotFound`        — a named artifact (change-id, snapshot-id) was not found.
// `RebaseRequired`  — apply rejected because base_snapshot_id is stale.
// `PreflightFailed` — runtime preflight did not pass.
// `Domain`          — any other domain-level failure with a description.

use std::fmt;

/// The unified error type for `ail-cli` operations.
#[derive(Debug)]
pub enum CliError {
    /// An I/O operation failed (file read, stdin, directory creation, etc.).
    Io(std::io::Error),
    /// A parsing operation failed; the message describes what was invalid.
    ParseError(String),
    /// A named artifact (change-id, snapshot-id, etc.) was not found.
    NotFound(String),
    /// Apply was rejected because the ChangeSet's base snapshot is stale.
    ///
    /// Carries the id of the current live snapshot.
    RebaseRequired {
        /// The live snapshot id at the time of the rejection.
        current_snapshot_id: u64,
    },
    /// Runtime preflight did not pass.
    PreflightFailed(String),
    /// Any other domain-level failure with a human-readable description.
    Domain(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Io(e) => write!(f, "I/O error: {e}"),
            CliError::ParseError(msg) => write!(f, "parse error: {msg}"),
            CliError::NotFound(msg) => write!(f, "not found: {msg}"),
            CliError::RebaseRequired {
                current_snapshot_id,
            } => write!(
                f,
                "rebase required: current snapshot is {current_snapshot_id}"
            ),
            CliError::PreflightFailed(msg) => write!(f, "preflight failed: {msg}"),
            CliError::Domain(msg) => write!(f, "error: {msg}"),
        }
    }
}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        CliError::Io(e)
    }
}

/// Map a `CliError` to an exit code.
///
/// - `2` for dispatch errors (InvalidSubcommand — handled earlier in `run()`).
/// - `1` for all domain errors.
pub fn exit_code(_err: &CliError) -> i32 {
    1
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Scenario: io::Error converts to CliError::Io via From impl.
    //   GIVEN a std::io::Error
    //   WHEN converted with .into()
    //   THEN the result is CliError::Io wrapping the original error
    #[test]
    fn io_error_converts_to_cli_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let cli_err: CliError = io_err.into();
        assert!(
            matches!(cli_err, CliError::Io(_)),
            "io::Error must convert to CliError::Io"
        );
    }

    // Scenario: ParseError holds the message string.
    //   GIVEN a CliError::ParseError with a specific message
    //   WHEN constructed directly
    //   THEN the variant matches ParseError
    #[test]
    fn parse_error_holds_message() {
        let err = CliError::ParseError("bad input".to_string());
        assert!(
            matches!(err, CliError::ParseError(_)),
            "ParseError variant must be constructible"
        );
    }

    // TRIANGULATE: Display output contains the inner message.
    //   GIVEN a CliError::ParseError("unexpected token")
    //   WHEN formatted with Display
    //   THEN the output contains "unexpected token"
    #[test]
    fn display_includes_message() {
        let err = CliError::ParseError("unexpected token".to_string());
        let msg = format!("{err}");
        assert!(
            msg.contains("unexpected token"),
            "Display must include the error message; got: {msg}"
        );
    }

    // TRIANGULATE: NotFound carries its message.
    //   GIVEN CliError::NotFound("change-id not found: abc")
    //   WHEN formatted with Display
    //   THEN the output contains "not found"
    #[test]
    fn not_found_display_contains_not_found() {
        let err = CliError::NotFound("change-id not found: abc".to_string());
        let msg = format!("{err}");
        assert!(
            msg.contains("not found"),
            "NotFound Display must say 'not found'; got: {msg}"
        );
    }

    // TRIANGULATE: RebaseRequired carries the current snapshot id.
    //   GIVEN CliError::RebaseRequired { current_snapshot_id: 42 }
    //   WHEN formatted with Display
    //   THEN the output contains "42"
    #[test]
    fn rebase_required_display_contains_snapshot_id() {
        let err = CliError::RebaseRequired {
            current_snapshot_id: 42,
        };
        let msg = format!("{err}");
        assert!(
            msg.contains("42"),
            "RebaseRequired Display must include snapshot id; got: {msg}"
        );
    }

    // TRIANGULATE: PreflightFailed carries the reason.
    //   GIVEN CliError::PreflightFailed("hash mismatch")
    //   WHEN formatted with Display
    //   THEN the output contains "preflight failed"
    #[test]
    fn preflight_failed_display_contains_reason() {
        let err = CliError::PreflightFailed("hash mismatch".to_string());
        let msg = format!("{err}");
        assert!(
            msg.contains("preflight failed"),
            "PreflightFailed Display must say 'preflight failed'; got: {msg}"
        );
    }

    // TRIANGULATE: exit_code returns 1 for domain errors.
    //   GIVEN any CliError that is not a dispatch error
    //   WHEN exit_code is called
    //   THEN 1 is returned
    #[test]
    fn exit_code_returns_one_for_domain_errors() {
        assert_eq!(exit_code(&CliError::NotFound("x".to_string())), 1);
        assert_eq!(exit_code(&CliError::Domain("y".to_string())), 1);
        assert_eq!(
            exit_code(&CliError::RebaseRequired {
                current_snapshot_id: 0
            }),
            1
        );
    }
}
