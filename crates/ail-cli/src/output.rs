// ── ail-cli::output ───────────────────────────────────────────────────────
// `OutputMode` and helpers are wired into cli.rs dispatch in PR2.
#![allow(dead_code)]
//
// Human/JSON output formatting for all CLI commands.
//
// # Design
//
// `format_response` is a pure function — it returns a `String` with no side
// effects and is the primary target for unit tests.  `print_response` is a
// thin wrapper that writes the formatted string to stdout.
//
// # JSON envelope
//
// In `Json` mode every command response is wrapped as:
//
// ```json
// { "status": "ok", "data": <payload> }
// ```
//
// This matches the spec requirement: every `--json` response MUST have
// top-level `status` and `data` fields.

use serde_json::Value;

// ── OutputMode ────────────────────────────────────────────────────────────

/// Selects the output format for a CLI command.
///
/// `Human` — free-form human-readable text on stdout.
/// `Json`  — a JSON object with `status` and `data` fields on stdout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Human-readable output (default).
    Human,
    /// Machine-readable JSON output (`--json` flag).
    Json,
}

// ── format_response (pure) ────────────────────────────────────────────────

/// Format a command response as a `String`.
///
/// In `Human` mode the `human_msg` argument is returned as-is.
/// In `Json` mode a `{ "status": "ok", "data": <data> }` envelope is
/// serialized to a compact JSON string.
///
/// This is a **pure function**: no I/O, no side effects, deterministic output.
pub fn format_response(mode: OutputMode, human_msg: &str, data: Value) -> String {
    match mode {
        OutputMode::Human => human_msg.to_string(),
        OutputMode::Json => {
            serde_json::json!({ "status": "ok", "data": data }).to_string()
        }
    }
}

// ── print_response ────────────────────────────────────────────────────────

/// Print a command response to stdout.
///
/// Delegates formatting to [`format_response`] then writes a single line
/// to stdout via `println!`.
pub fn print_response(mode: OutputMode, human_msg: &str, data: Value) {
    println!("{}", format_response(mode, human_msg, data));
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Scenario: Human mode returns the human message unchanged.
    //   GIVEN OutputMode::Human
    //   WHEN format_response is called
    //   THEN the returned string equals the human_msg argument exactly
    #[test]
    fn human_mode_returns_message_as_is() {
        let result = format_response(OutputMode::Human, "hello world", Value::Null);
        assert_eq!(result, "hello world", "Human mode must return message unchanged");
    }

    // TRIANGULATE: Json mode returns valid JSON with status and data fields.
    //   GIVEN OutputMode::Json and a non-null data payload
    //   WHEN format_response is called
    //   THEN the result parses as JSON with "status" == "ok" and "data" set
    #[test]
    fn json_mode_produces_envelope_with_status_and_data() {
        let data = json!({ "hash": "abc123" });
        let result = format_response(OutputMode::Json, "ignored", data.clone());

        let parsed: Value = serde_json::from_str(&result)
            .expect("Json mode output must be valid JSON");

        assert_eq!(
            parsed["status"], "ok",
            "JSON envelope must have status == \"ok\""
        );
        assert_eq!(
            parsed["data"], data,
            "JSON envelope must preserve the data payload"
        );
    }

    // TRIANGULATE: Human mode ignores the data argument entirely.
    //   GIVEN OutputMode::Human with a rich JSON data value
    //   WHEN format_response is called
    //   THEN the output contains no JSON — only the human message
    #[test]
    fn human_mode_ignores_data() {
        let data = json!({ "irrelevant": true });
        let result = format_response(OutputMode::Human, "plain output", data);
        assert_eq!(result, "plain output", "Human mode must not include JSON data");
    }
}
