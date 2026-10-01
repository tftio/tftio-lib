//! Health check and diagnostics module.
//!
//! This module provides a framework for running health checks on CLI tools
//! with tool-specific diagnostics.

use crate::{
    JsonOutput,
    types::{DoctorCheck, RepoInfo},
};
use serde_json::{Map, Value, json};
use std::fmt::Write as _;

/// Structured doctor report reusable for text and JSON output.
#[derive(Debug, Clone)]
pub struct DoctorReport {
    header: String,
    checks: Vec<DoctorCheck>,
    errors: Vec<String>,
    warnings: Vec<String>,
    info: Vec<String>,
    version: Option<String>,
    details: Map<String, Value>,
}

impl DoctorReport {
    /// Create an empty doctor report.
    #[must_use]
    pub fn new(header: impl Into<String>) -> Self {
        Self {
            header: header.into(),
            checks: Vec::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
            info: Vec::new(),
            version: None,
            details: Map::new(),
        }
    }

    /// Create a doctor report scaffold for a tool using the standard header, version, and checks.
    #[must_use]
    pub fn for_tool<T: DoctorChecks>(tool: &T) -> Self {
        Self::with_tool_header(tool, format!("🏥 {} health check", T::repo_info().name))
    }

    /// Create a doctor report scaffold for a tool with a caller-provided header.
    #[must_use]
    pub fn with_tool_header<T: DoctorChecks>(tool: &T, header: impl Into<String>) -> Self {
        Self::new(header)
            .with_checks(tool.tool_checks())
            .with_version(T::current_version())
    }

    /// Set the report checks.
    #[must_use]
    pub fn with_checks(mut self, checks: Vec<DoctorCheck>) -> Self {
        self.checks = checks;
        self
    }

    /// Set the reported version string.
    #[must_use]
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Add an error line.
    #[must_use]
    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.errors.push(error.into());
        self
    }

    /// Add a warning line.
    #[must_use]
    pub fn with_warning(mut self, warning: impl Into<String>) -> Self {
        self.warnings.push(warning.into());
        self
    }

    /// Add an informational line.
    #[must_use]
    pub fn with_info(mut self, info: impl Into<String>) -> Self {
        self.info.push(info.into());
        self
    }

    /// Add a custom JSON detail field.
    #[must_use]
    pub fn with_detail(mut self, key: impl Into<String>, value: Value) -> Self {
        self.details.insert(key.into(), value);
        self
    }

    /// Access the underlying checks.
    #[must_use]
    pub fn checks(&self) -> &[DoctorCheck] {
        &self.checks
    }

    fn failed_checks(&self) -> usize {
        self.checks.iter().filter(|check| !check.passed).count()
    }

    /// Return the process exit code implied by this report.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        i32::from(self.failed_checks() > 0 || !self.errors.is_empty())
    }

    /// Render the report as JSON.
    #[must_use]
    pub fn to_json_value(&self) -> Value {
        let mut value = json!({
            "ok": self.exit_code() == 0,
            "header": self.header,
            "checks": self
                .checks
                .iter()
                .map(|check| json!({
                    "name": check.name,
                    "passed": check.passed,
                    "message": check.message,
                }))
                .collect::<Vec<_>>(),
            "errors": self.errors,
            "warnings": self.warnings,
            "info": self.info,
            "version": self.version,
        });

        if let Some(object) = value.as_object_mut() {
            for (key, detail) in &self.details {
                object.insert(key.clone(), detail.clone());
            }
        }
        value
    }

    /// Render the report as plain text.
    #[must_use]
    pub fn render_text(&self) -> String {
        let mut output = String::new();
        output.push_str(&self.header);
        output.push('\n');
        output.push_str(&"=".repeat(self.header.chars().count()));
        output.push_str("\n\n");

        if !self.checks.is_empty() {
            output.push_str("Configuration:\n");
            for check in &self.checks {
                if check.passed {
                    writeln!(&mut output, "  ✅ {}", check.name).unwrap_or_default();
                } else {
                    writeln!(&mut output, "  ❌ {}", check.name).unwrap_or_default();
                    if let Some(message) = &check.message {
                        writeln!(&mut output, "     {message}").unwrap_or_default();
                    }
                }
            }
            output.push('\n');
        }

        if !self.info.is_empty() {
            output.push_str("Info:\n");
            for info in &self.info {
                writeln!(&mut output, "  ℹ️  {info}").unwrap_or_default();
            }
            output.push('\n');
        }

        if !self.warnings.is_empty() {
            output.push_str("Warnings:\n");
            for warning in &self.warnings {
                writeln!(&mut output, "  ⚠️  {warning}").unwrap_or_default();
            }
            output.push('\n');
        }

        if self.exit_code() == 0 {
            output.push_str("✨ Everything looks healthy!\n");
        } else {
            output.push_str("❌ Issues found - see above for details\n");
        }

        output
    }

    /// Emit the report in the selected format and return its exit code.
    #[must_use]
    pub fn emit_output(&self, output: JsonOutput) -> i32 {
        if output.is_json() {
            print_doctor_report_json(self)
        } else {
            print_doctor_report_text(self)
        }
    }
}

/// Trait for tools that support doctor health checks.
///
/// Implement this trait to provide tool-specific health checks.
pub trait DoctorChecks {
    /// Get the repository information for this tool.
    fn repo_info() -> RepoInfo;

    /// Get the current version of this tool.
    fn current_version() -> &'static str;

    /// Run tool-specific health checks.
    ///
    /// Return a vector of check results. Default implementation returns empty vector.
    fn tool_checks(&self) -> Vec<DoctorCheck> {
        Vec::new()
    }
}

/// Run doctor command to check health and configuration.
///
/// Returns exit code: 0 if healthy, 1 if issues found.
///
/// # Type Parameters
/// * `T` - A type that implements `DoctorChecks`
pub fn run_doctor<T: DoctorChecks>(tool: &T) -> i32 {
    let header = format!("🏥 {} health check", T::repo_info().name);
    run_doctor_with_output_and_header(tool, &header, JsonOutput::Text)
}

fn build_doctor_report<T: DoctorChecks>(tool: &T, header: &str) -> DoctorReport {
    DoctorReport::with_tool_header(tool, header)
}

fn render_doctor_with_header<T: DoctorChecks>(tool: &T, header: &str) -> (String, i32) {
    let report = build_doctor_report(tool, header);
    (report.render_text(), report.exit_code())
}

/// Run doctor output with a custom header.
pub fn run_doctor_with_header<T: DoctorChecks>(tool: &T, header: &str) -> i32 {
    run_doctor_with_output_and_header(tool, header, JsonOutput::Text)
}

/// Run doctor output in the selected format.
pub fn run_doctor_with_output<T: DoctorChecks>(tool: &T, output: JsonOutput) -> i32 {
    let header = format!("🏥 {} health check", T::repo_info().name);
    run_doctor_with_output_and_header(tool, &header, output)
}

/// Run doctor output with a custom header and output mode.
pub fn run_doctor_with_output_and_header<T: DoctorChecks>(
    tool: &T,
    header: &str,
    output: JsonOutput,
) -> i32 {
    if output.is_json() {
        let report = build_doctor_report(tool, header);
        print_doctor_report_json(&report)
    } else {
        let (rendered, exit_code) = render_doctor_with_header(tool, header);
        print!("{rendered}");
        exit_code
    }
}

/// Print a structured doctor report as JSON and return its exit code.
#[must_use]
pub fn print_doctor_report_json(report: &DoctorReport) -> i32 {
    let value = report.to_json_value();
    match serde_json::to_string_pretty(&value) {
        Ok(rendered) => println!("{rendered}"),
        Err(error) => println!("{}", json!({ "ok": false, "error": error.to_string() })),
    }
    report.exit_code()
}

/// Print a structured doctor report as plain text and return its exit code.
#[must_use]
pub fn print_doctor_report_text(report: &DoctorReport) -> i32 {
    print!("{}", report.render_text());
    report.exit_code()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestTool;

    impl DoctorChecks for TestTool {
        fn repo_info() -> RepoInfo {
            RepoInfo::new("example", "test-tool")
        }

        fn current_version() -> &'static str {
            "1.0.0"
        }

        fn tool_checks(&self) -> Vec<DoctorCheck> {
            vec![
                DoctorCheck::pass("Test check 1"),
                DoctorCheck::fail("Test check 2", "This is a failure"),
            ]
        }
    }

    #[test]
    fn test_run_doctor() {
        let tool = TestTool;
        let exit_code = run_doctor(&tool);
        // Should return 1 because we have a failing check
        assert_eq!(exit_code, 1);
    }

    #[test]
    fn test_run_doctor_with_custom_header() {
        let tool = TestTool;
        let (output, exit_code) = render_doctor_with_header(&tool, "Custom Header");
        assert!(output.contains("Custom Header"));
        assert_eq!(exit_code, 1);
    }

    #[test]
    fn doctor_report_json_includes_details() {
        let report = DoctorReport::new("Header")
            .with_checks(vec![DoctorCheck::pass("check")])
            .with_detail("config_file_exists", json!(true));

        let value = report.to_json_value();
        assert_eq!(value["config_file_exists"], json!(true));
        assert_eq!(value["ok"], json!(true));
    }

    #[test]
    fn doctor_report_for_tool_uses_repo_name_version_and_checks() {
        let report = DoctorReport::for_tool(&TestTool);
        let value = report.to_json_value();

        assert_eq!(value["header"], json!("🏥 test-tool health check"));
        assert_eq!(value["version"], json!("1.0.0"));
        assert_eq!(value["checks"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn doctor_report_emit_returns_exit_code_for_selected_format() {
        let report = DoctorReport::for_tool(&TestTool);
        assert_eq!(report.emit_output(JsonOutput::Json), 1);
    }

    #[test]
    fn run_doctor_with_output_supports_json_mode() {
        let tool = TestTool;
        let exit_code = run_doctor_with_output(&tool, JsonOutput::Json);
        assert_eq!(exit_code, 1);
    }

    #[test]
    fn doctor_report_accumulates_errors_warnings_and_info() {
        let report = DoctorReport::new("Header")
            .with_checks(vec![
                DoctorCheck::pass("c1"),
                DoctorCheck::fail("c2", "boom"),
            ])
            .with_error("an error")
            .with_warning("a warning")
            .with_info("some info");

        assert_eq!(report.checks().len(), 2);

        let value = report.to_json_value();
        assert_eq!(value["errors"], json!(["an error"]));
        assert_eq!(value["warnings"], json!(["a warning"]));
        assert_eq!(value["info"], json!(["some info"]));
        assert_eq!(value["ok"], json!(false));
    }

    #[test]
    fn doctor_report_exit_code_reflects_errors_without_failing_checks() {
        let clean = DoctorReport::new("Header").with_checks(vec![DoctorCheck::pass("ok")]);
        assert_eq!(clean.exit_code(), 0);

        let with_error = clean.with_error("something went wrong");
        assert_eq!(with_error.exit_code(), 1);
    }

    #[test]
    fn render_text_renders_checks_info_and_warnings() {
        let report = DoctorReport::new("Doctor Report")
            .with_checks(vec![
                DoctorCheck::pass("passing check"),
                DoctorCheck::fail("failing check", "the failure detail"),
            ])
            .with_info("informational note")
            .with_warning("cautionary note");

        let text = report.render_text();
        assert!(text.contains("Doctor Report"), "header missing: {text}");
        assert!(text.contains("Configuration:"), "checks section missing");
        assert!(text.contains("✅ passing check"), "passing mark missing");
        assert!(text.contains("❌ failing check"), "failing mark missing");
        assert!(
            text.contains("the failure detail"),
            "failure message missing"
        );
        assert!(text.contains("Info:"), "info section missing");
        assert!(text.contains("informational note"), "info line missing");
        assert!(text.contains("Warnings:"), "warnings section missing");
        assert!(text.contains("cautionary note"), "warning line missing");
        assert!(text.contains("Issues found"), "unhealthy footer missing");
    }

    #[test]
    fn render_text_reports_healthy_when_all_checks_pass() {
        let report = DoctorReport::new("Healthy").with_checks(vec![DoctorCheck::pass("all good")]);
        let text = report.render_text();
        assert!(
            text.contains("Everything looks healthy"),
            "healthy footer missing"
        );
        assert!(!text.contains("Issues found"), "should not report issues");
    }

    #[test]
    fn run_doctor_with_header_returns_failure_exit_code() {
        let tool = TestTool;
        let exit_code = run_doctor_with_header(&tool, "Custom Doctor Header");
        assert_eq!(exit_code, 1);
    }
}
