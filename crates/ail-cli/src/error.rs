// ── ail-cli::error ────────────────────────────────────────────────────────
// `CliError` is wired into cli.rs dispatch in PR2; suppress dead_code until then.
#![allow(dead_code)]
//
// `CliError` is the single error type for all CLI operations.
//
// # Variants
//
// `Io`         — wraps `std::io::Error` for file/stdin read failures.
// `ParseError` — structured text describing what could not be parsed.

use std::fmt;

/// The unified error type for `ail-cli` operations.
#[derive(Debug)]
pub enum CliError {
    /// An I/O operation failed (file read, stdin, directory creation, etc.).
    Io(std::io::Error),
    /// A parsing operation failed; the message describes what was invalid.
    ParseError(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Io(e) => write!(f, "I/O error: {e}"),
            CliError::ParseError(msg) => write!(f, "parse error: {msg}"),
        }
    }
}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        CliError::Io(e)
    }
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
}
