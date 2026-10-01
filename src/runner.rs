//! Shared CLI runner helpers.

use crate::{JsonOutput, err_response};
use serde_json::json;
use std::fmt::Display;

/// Shared fatal CLI error state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FatalCliError {
    command: String,
    output: JsonOutput,
    message: String,
}

impl FatalCliError {
    /// Create a fatal CLI error with the shared renderer contract.
    #[must_use]
    pub fn new(command: impl Into<String>, output: JsonOutput, message: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            output,
            message: message.into(),
        }
    }

    /// Return the command label used in shared error output.
    #[must_use]
    pub fn command(&self) -> &str {
        &self.command
    }

    /// Return the output mode used for shared error rendering.
    #[must_use]
    pub const fn output(&self) -> JsonOutput {
        self.output
    }

    /// Return the fatal message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Render the fatal error as a string without emitting it.
    #[must_use]
    pub fn render(&self) -> String {
        if self.output.is_json() {
            err_response(self.command(), "ERROR", self.message(), json!({})).to_string()
        } else {
            format!("error: {}", self.message())
        }
    }

    /// Emit the fatal error to the correct output stream.
    pub fn emit(&self) {
        if self.output.is_json() {
            println!("{}", self.render());
        } else {
            eprintln!("{}", self.render());
        }
    }

    /// Emit the fatal error and return the shared failure exit code.
    #[must_use]
    pub fn emit_and_exit_code(self) -> i32 {
        self.emit();
        1
    }
}

/// Run a fallible CLI closure and convert shared fatal errors into exit codes.
#[must_use]
pub fn run_with_fatal_handler<F>(run: F) -> i32
where
    F: FnOnce() -> Result<i32, FatalCliError>,
{
    match run() {
        Ok(exit_code) => exit_code,
        Err(error) => error.emit_and_exit_code(),
    }
}

/// Run a fallible CLI closure with shared fatal rendering for any displayable error type.
#[must_use]
pub fn run_with_display_error_handler<F, E>(command: &str, output: JsonOutput, run: F) -> i32
where
    F: FnOnce() -> Result<i32, E>,
    E: Display,
{
    run_with_fatal_handler(|| {
        run().map_err(|error| FatalCliError::new(command, output, error.to_string()))
    })
}

/// Parse CLI state with one closure and execute it with another.
#[must_use]
pub fn parse_and_run<T, P, F>(parse: P, run: F) -> i32
where
    P: FnOnce() -> T,
    F: FnOnce(T) -> Result<i32, FatalCliError>,
{
    run_with_fatal_handler(|| run(parse()))
}

/// Parse CLI state, run the handler, and exit the process with the resulting code.
pub fn parse_and_exit<T, P, F>(parse: P, run: F) -> !
where
    P: FnOnce() -> T,
    F: FnOnce(T) -> Result<i32, FatalCliError>,
{
    std::process::exit(parse_and_run(parse, run))
}

#[cfg(test)]
mod tests {
    use std::fmt;

    use crate::error::fatal_error;

    use super::*;

    #[derive(Debug)]
    struct DisplayOnlyError(&'static str);

    impl fmt::Display for DisplayOnlyError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    #[test]
    fn run_with_fatal_handler_returns_success_code() {
        let exit_code = run_with_fatal_handler(|| Ok(7));
        assert_eq!(exit_code, 7);
    }

    #[test]
    fn run_with_fatal_handler_converts_fatal_error_to_failure_code() {
        let exit_code =
            run_with_fatal_handler(|| Err(fatal_error("scan", JsonOutput::Text, "bad")));
        assert_eq!(exit_code, 1);
    }

    fn expect_parsed(cli: String) -> Result<i32, FatalCliError> {
        if cli == "parsed" {
            Ok(0)
        } else {
            Err(fatal_error("scan", JsonOutput::Text, cli))
        }
    }

    #[test]
    fn parse_and_run_passes_parsed_value_to_runner() {
        let exit_code = parse_and_run(|| String::from("parsed"), expect_parsed);
        assert_eq!(exit_code, 0);
    }

    #[test]
    fn parse_and_run_reports_unexpected_cli() {
        let exit_code = parse_and_run(|| String::from("other"), expect_parsed);
        assert_eq!(exit_code, 1);
    }

    #[test]
    fn fatal_cli_error_json_emit_returns_failure_code() {
        let exit_code = FatalCliError::new("scan", JsonOutput::Json, "boom").emit_and_exit_code();
        assert_eq!(exit_code, 1);
    }

    #[test]
    fn fatal_cli_error_renders_json_when_requested() {
        let rendered = FatalCliError::new("scan", JsonOutput::Json, "bad").render();
        assert!(rendered.contains("\"ok\":false"));
        assert!(rendered.contains("\"code\":\"ERROR\""));
        assert!(rendered.contains("\"command\":\"scan\""));
    }

    #[test]
    fn run_with_display_error_handler_returns_success_code() {
        let exit_code = run_with_display_error_handler("scan", JsonOutput::Text, || {
            Ok::<i32, DisplayOnlyError>(9)
        });
        assert_eq!(exit_code, 9);
    }

    #[test]
    fn run_with_display_error_handler_converts_display_errors() {
        let exit_code = run_with_display_error_handler("scan", JsonOutput::Text, || {
            Err::<i32, DisplayOnlyError>(DisplayOnlyError("bad"))
        });
        assert_eq!(exit_code, 1);
    }
}
