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

use serde_json::{Value, json};

/// The JSON output schema version injected into every `--json` response.
///
/// Satisfies spec requirement JV-1: every `--json` output MUST include
/// `"schema_version": "1"` inside the `data` object.
pub const JSON_OUTPUT_VERSION: &str = "1";

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
            // Inject schema_version into the data object before serializing.
            // This satisfies spec JV-1: every --json response has schema_version in data.
            let mut data_obj = data;
            if let Some(obj) = data_obj.as_object_mut() {
                obj.insert("schema_version".to_string(), json!(JSON_OUTPUT_VERSION));
            }
            json!({ "status": "ok", "data": data_obj }).to_string()
        }
    }
}

/// Format a structured error response for `--json` command failures.
pub fn format_error_response(data: Value) -> String {
    let mut data_obj = data;
    if let Some(obj) = data_obj.as_object_mut() {
        obj.insert("schema_version".to_string(), json!(JSON_OUTPUT_VERSION));
    }
    json!({ "status": "error", "data": data_obj }).to_string()
}

// ── print_response ────────────────────────────────────────────────────────

/// Print a command response to stdout.
///
/// Delegates formatting to [`format_response`] then writes a single line
/// to stdout via `println!`.
pub fn print_response(mode: OutputMode, human_msg: &str, data: Value) {
    println!("{}", format_response(mode, human_msg, data));
}

/// Print a structured `--json` error response to stdout.
pub fn print_error_response(data: Value) {
    println!("{}", format_error_response(data));
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
        assert_eq!(
            result, "hello world",
            "Human mode must return message unchanged"
        );
    }

    // Scenario JV-1a/JV-1b: Json mode injects schema_version = "1" into data.
    //   GIVEN OutputMode::Json
    //   WHEN format_response is called
    //   THEN data["schema_version"] == "1" (string, not number)
    #[test]
    fn json_mode_injects_schema_version() {
        let data = json!({ "hash": "abc123" });
        let result = format_response(OutputMode::Json, "ignored", data);

        let parsed: Value =
            serde_json::from_str(&result).expect("Json mode output must be valid JSON");

        assert_eq!(
            parsed["data"]["schema_version"], "1",
            "JSON data must contain schema_version == \"1\""
        );
    }

    // TRIANGULATE: schema_version is a string, not a number.
    //   GIVEN OutputMode::Json with a different payload
    //   WHEN format_response is called
    //   THEN data["schema_version"] is the string "1", not integer 1
    #[test]
    fn json_mode_schema_version_is_string_not_number() {
        let data = json!({ "nodes": [] });
        let result = format_response(OutputMode::Json, "ignored", data);

        let parsed: Value =
            serde_json::from_str(&result).expect("Json mode output must be valid JSON");

        assert!(
            parsed["data"]["schema_version"].is_string(),
            "schema_version must be a JSON string, not a number"
        );
        assert_eq!(
            parsed["data"]["schema_version"].as_str().unwrap(),
            "1",
            "schema_version string value must be \"1\""
        );
    }

    // Scenario: Json mode produces valid envelope with status "ok" and preserves data fields.
    //   GIVEN OutputMode::Json and a non-null data payload
    //   WHEN format_response is called
    //   THEN status == "ok" and the original data fields are accessible
    #[test]
    fn json_mode_produces_envelope_with_status_and_data() {
        let data = json!({ "hash": "abc123" });
        let result = format_response(OutputMode::Json, "ignored", data);

        let parsed: Value =
            serde_json::from_str(&result).expect("Json mode output must be valid JSON");

        assert_eq!(
            parsed["status"], "ok",
            "JSON envelope must have status == \"ok\""
        );
        // Original data fields are preserved alongside schema_version.
        assert_eq!(
            parsed["data"]["hash"], "abc123",
            "JSON envelope must preserve the original data fields"
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
        assert_eq!(
            result, "plain output",
            "Human mode must not include JSON data"
        );
    }
}
