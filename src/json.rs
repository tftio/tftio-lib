//! Shared JSON response helpers.

use serde_json::{Value, json};

/// Shared text-vs-JSON output mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonOutput {
    /// Render human-readable text output.
    Text,
    /// Render JSON output.
    Json,
}

impl JsonOutput {
    /// Convert a `--json` flag into a shared output mode.
    #[must_use]
    pub const fn from_flag(json: bool) -> Self {
        if json { Self::Json } else { Self::Text }
    }

    /// Whether this mode renders JSON.
    #[must_use]
    pub const fn is_json(self) -> bool {
        matches!(self, Self::Json)
    }
}

/// Build a success response envelope.
#[must_use]
#[allow(
    clippy::needless_pass_by_value,
    reason = "data is moved into the response envelope, so taking it by value avoids a clone"
)]
pub fn ok_response(command: &str, data: Value) -> Value {
    json!({
        "ok": true,
        "command": command,
        "data": data
    })
}

/// Build an error response envelope.
#[must_use]
#[allow(
    clippy::needless_pass_by_value,
    reason = "details is moved into the response envelope, so taking it by value avoids a clone"
)]
pub fn err_response(command: &str, code: &str, message: &str, details: Value) -> Value {
    json!({
        "ok": false,
        "command": command,
        "error": {
            "code": code,
            "message": message,
            "details": details
        }
    })
}

/// Render either the shared JSON envelope or plain text for a command response.
#[must_use]
#[allow(
    clippy::needless_pass_by_value,
    reason = "data is moved into the render closure, so taking it by value avoids a clone"
)]
pub fn render_response(
    command: &str,
    output: JsonOutput,
    data: Value,
    text: impl Into<String>,
) -> String {
    render_response_parts(command, output, || data, || text.into())
}

/// Render either the shared JSON envelope or lazily-built plain text for a command response.
#[must_use]
#[allow(
    clippy::needless_pass_by_value,
    reason = "data is moved into the render closure, so taking it by value avoids a clone"
)]
pub fn render_response_with<F>(command: &str, output: JsonOutput, data: Value, text: F) -> String
where
    F: FnOnce() -> String,
{
    render_response_parts(command, output, || data, text)
}

/// Render either the shared JSON envelope or lazily-built command data and plain text.
#[must_use]
#[allow(
    clippy::needless_pass_by_value,
    reason = "the data and text closures are consumed (called once) inside the function, so by-value is correct"
)]
pub fn render_response_parts<D, T>(command: &str, output: JsonOutput, data: D, text: T) -> String
where
    D: FnOnce() -> Value,
    T: FnOnce() -> String,
{
    if output.is_json() {
        ok_response(command, data()).to_string()
    } else {
        text()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use serde_json::json;

    use super::*;

    /// Text builder that records whether it was invoked.
    fn tracking_text(called: &Cell<bool>) -> String {
        called.set(true);
        String::from("text")
    }

    /// Data builder that records whether it was invoked.
    fn tracking_data(called: &Cell<bool>) -> Value {
        called.set(true);
        json!({ "x": 1 })
    }

    #[test]
    fn json_output_tracks_flag_state() {
        assert_eq!(JsonOutput::from_flag(false), JsonOutput::Text);
        assert_eq!(JsonOutput::from_flag(true), JsonOutput::Json);
        assert!(!JsonOutput::Text.is_json());
        assert!(JsonOutput::Json.is_json());
    }

    #[test]
    fn ok_response_contains_expected_shape() {
        let value = ok_response("list", json!({ "x": 1 }));
        assert_eq!(value["ok"], json!(true));
        assert_eq!(value["command"], json!("list"));
        assert_eq!(value["data"]["x"], json!(1));
    }

    #[test]
    fn err_response_contains_expected_shape() {
        let value = err_response("list", "ERROR", "bad", json!({}));
        assert_eq!(value["ok"], json!(false));
        assert_eq!(value["error"]["code"], json!("ERROR"));
        assert_eq!(value["error"]["message"], json!("bad"));
    }

    #[test]
    fn render_response_uses_json_envelope_when_requested() {
        let value = render_response("list", JsonOutput::Json, json!({"x": 1}), "text");
        assert!(value.contains("\"ok\":true"));
    }

    #[test]
    fn render_response_with_skips_text_builder_for_json_output() {
        let called = Cell::new(false);
        let value = render_response_with("list", JsonOutput::Json, json!({"x": 1}), || {
            tracking_text(&called)
        });

        assert!(value.contains("\"ok\":true"));
        assert!(!called.get());
    }

    #[test]
    fn render_response_with_builds_text_for_text_output() {
        let called = Cell::new(false);
        let value = render_response_with("list", JsonOutput::Text, json!({"x": 1}), || {
            tracking_text(&called)
        });
        assert_eq!(value, "text");
        assert!(called.get());
    }

    #[test]
    fn render_response_parts_skips_text_builder_for_json_output() {
        let data_called = Cell::new(false);
        let text_called = Cell::new(false);
        let value = render_response_parts(
            "list",
            JsonOutput::Json,
            || tracking_data(&data_called),
            || tracking_text(&text_called),
        );

        assert!(value.contains("\"ok\":true"));
        assert!(data_called.get());
        assert!(!text_called.get());
    }

    #[test]
    fn render_response_parts_skips_data_builder_for_text_output() {
        let data_called = Cell::new(false);
        let text_called = Cell::new(false);
        let value = render_response_parts(
            "list",
            JsonOutput::Text,
            || tracking_data(&data_called),
            || tracking_text(&text_called),
        );

        assert_eq!(value, "text");
        assert!(!data_called.get());
        assert!(text_called.get());
    }
}
