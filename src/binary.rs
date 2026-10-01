//! Shared binary entrypoint helpers for workspace CLI tools.

use clap::{CommandFactory, FromArgMatches};

use crate::{
    AgentDispatch, DoctorChecks, FatalCliError, ProcessEnv, ToolSpec,
    command::run_standard_command, parse_command_with_agent_surface_from,
    run_standard_command_no_doctor, run_with_fatal_handler,
};

fn run_cli_with_parsed<T, D, M, F>(
    spec: &'static ToolSpec,
    env: &ProcessEnv,
    parsed: AgentDispatch<T>,
    doctor: Option<&D>,
    metadata_command: M,
    run: F,
) -> i32
where
    T: CommandFactory,
    D: DoctorChecks,
    M: FnOnce(&T) -> Option<crate::StandardCommand>,
    F: FnOnce(T) -> Result<i32, FatalCliError>,
{
    match parsed {
        AgentDispatch::Cli(cli) => {
            if let Some(command) = metadata_command(&cli) {
                return run_standard_command::<T, D>(spec, env, &command, doctor);
            }
            run_with_fatal_handler(|| run(cli))
        }
        AgentDispatch::Printed(code) => code,
    }
}

/// Parse CLI arguments from a caller-provided argv, route shared metadata commands, and run the domain handler.
///
/// `env` carries the process-edge environment reads (agent tokens, `HOME`)
/// performed in the binary's `main` (`REPO_INVARIANTS.md` ENG-008).
#[must_use]
pub fn run_cli_from<T, I, D, M, F>(
    spec: &'static ToolSpec,
    env: &ProcessEnv,
    argv: I,
    doctor: &D,
    metadata_command: M,
    run: F,
) -> i32
where
    T: CommandFactory + FromArgMatches,
    I: IntoIterator,
    I::Item: Into<std::ffi::OsString> + Clone,
    D: DoctorChecks,
    M: FnOnce(&T) -> Option<crate::StandardCommand>,
    F: FnOnce(T) -> Result<i32, FatalCliError>,
{
    match parse_command_with_agent_surface_from::<T, I>(spec, &env.agent, argv) {
        Ok(parsed) => run_cli_with_parsed(spec, env, parsed, Some(doctor), metadata_command, run),
        Err(error) => error.exit(),
    }
}

/// Parse caller-provided argv, route shared metadata commands, and run the domain handler for a tool without doctor support.
///
/// `env` carries the process-edge environment reads (agent tokens, `HOME`)
/// performed in the binary's `main` (`REPO_INVARIANTS.md` ENG-008).
#[must_use]
pub fn run_cli_no_doctor_from<T, I, M, F>(
    spec: &'static ToolSpec,
    env: &ProcessEnv,
    argv: I,
    metadata_command: M,
    run: F,
) -> i32
where
    T: CommandFactory + FromArgMatches,
    I: IntoIterator,
    I::Item: Into<std::ffi::OsString> + Clone,
    M: FnOnce(&T) -> Option<crate::StandardCommand>,
    F: FnOnce(T) -> Result<i32, FatalCliError>,
{
    match parse_command_with_agent_surface_from::<T, I>(spec, &env.agent, argv) {
        Ok(AgentDispatch::Cli(cli)) => {
            if let Some(command) = metadata_command(&cli) {
                return run_standard_command_no_doctor::<T>(spec, env, &command);
            }
            run_with_fatal_handler(|| run(cli))
        }
        Ok(AgentDispatch::Printed(code)) => code,
        Err(error) => error.exit(),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use clap::{Parser, Subcommand};

    use super::*;
    use crate::{
        AGENT_TOKEN_ENV, AGENT_TOKEN_EXPECTED_ENV, AgentCapability, AgentSurfaceSpec,
        CommandSelector, FatalCliError, FlagSelector, JsonOutput, LicenseType, MetaCommand,
        RepoInfo, StandardCommand, map_standard_command, test_support::env_lock, workspace_tool,
    };

    const QUERY_COMMAND: CommandSelector = CommandSelector::new(&["query"]);
    const QUERY_FAIL_FLAG: FlagSelector = FlagSelector::new(&["query"], "fail");
    const QUERY_CAPABILITY: AgentCapability =
        AgentCapability::minimal("query", &[QUERY_COMMAND], &[QUERY_FAIL_FLAG]);
    const AGENT_SURFACE: AgentSurfaceSpec = AgentSurfaceSpec::new(&[QUERY_CAPABILITY]);
    const TOOL_SPEC: ToolSpec =
        workspace_tool("tool", "Tool", "1.2.3", LicenseType::MIT, true, true)
            .with_agent_surface(&AGENT_SURFACE);

    #[derive(Debug, Parser)]
    #[command(name = "tool")]
    struct TestCli {
        #[command(subcommand)]
        command: TestCommand,
    }

    #[derive(Debug, Subcommand)]
    enum TestCommand {
        Meta {
            #[command(subcommand)]
            command: MetaCommand,
        },
        Query {
            #[arg(long)]
            fail: bool,
        },
    }

    struct TestDoctor;

    impl DoctorChecks for TestDoctor {
        fn repo_info() -> RepoInfo {
            RepoInfo::new("example", "tool")
        }

        fn current_version() -> &'static str {
            "1.2.3"
        }
    }

    fn metadata_command(cli: &TestCli) -> Option<StandardCommand> {
        match &cli.command {
            TestCommand::Meta { command } => Some(map_standard_command(command, JsonOutput::Text)),
            TestCommand::Query { .. } => None,
        }
    }

    #[allow(unsafe_code)]
    fn set_tokens(presented: Option<&str>, expected: Option<&str>) {
        unsafe {
            std::env::remove_var(AGENT_TOKEN_ENV);
            std::env::remove_var(AGENT_TOKEN_EXPECTED_ENV);
            if let Some(presented) = presented {
                std::env::set_var(AGENT_TOKEN_ENV, presented);
            }
            if let Some(expected) = expected {
                std::env::set_var(AGENT_TOKEN_EXPECTED_ENV, expected);
            }
        }
    }

    fn env_from_detected() -> ProcessEnv {
        ProcessEnv {
            agent: crate::AgentModeContext::from_tokens(
                std::env::var(AGENT_TOKEN_ENV).ok(),
                std::env::var(AGENT_TOKEN_EXPECTED_ENV).ok(),
            ),
            home: None,
        }
    }

    fn recording_runner(
        called: &Cell<bool>,
    ) -> impl FnOnce(TestCli) -> Result<i32, FatalCliError> + '_ {
        move |_cli| {
            called.set(true);
            Ok(9)
        }
    }

    #[test]
    fn run_cli_from_routes_metadata_before_domain_runner() {
        let _guard = env_lock();
        set_tokens(None, None);
        let env = env_from_detected();
        let called = Cell::new(false);

        let exit_code = run_cli_from::<TestCli, _, TestDoctor, _, _>(
            &TOOL_SPEC,
            &env,
            ["tool", "meta", "doctor", "--json"],
            &TestDoctor,
            metadata_command,
            recording_runner(&called),
        );

        assert_eq!(exit_code, 0);
        assert!(!called.get());
    }

    #[test]
    fn run_cli_from_short_circuits_agent_help_output() {
        let _guard = env_lock();
        set_tokens(Some("shared-token"), Some("shared-token"));
        let env = env_from_detected();
        let called = Cell::new(false);

        let exit_code = run_cli_from::<TestCli, _, TestDoctor, _, _>(
            &TOOL_SPEC,
            &env,
            ["tool", "--agent-help"],
            &TestDoctor,
            metadata_command,
            recording_runner(&called),
        );

        assert_eq!(exit_code, 0);
        assert!(!called.get());
        set_tokens(None, None);
    }

    #[test]
    fn run_cli_from_wraps_domain_errors_with_shared_fatal_handler() {
        let _guard = env_lock();
        set_tokens(None, None);
        let env = env_from_detected();
        let exit_code = run_cli_from::<TestCli, _, TestDoctor, _, _>(
            &TOOL_SPEC,
            &env,
            ["tool", "query", "--fail"],
            &TestDoctor,
            metadata_command,
            |_cli| {
                Err(FatalCliError::new(
                    "query",
                    JsonOutput::Text,
                    "domain failure",
                ))
            },
        );

        assert_eq!(exit_code, 1);
    }

    #[test]
    fn run_cli_no_doctor_from_routes_metadata_without_domain_runner() {
        let _guard = env_lock();
        set_tokens(None, None);
        let env = env_from_detected();
        let called = Cell::new(false);

        let exit_code = run_cli_no_doctor_from::<TestCli, _, _, _>(
            &TOOL_SPEC,
            &env,
            ["tool", "meta", "license"],
            metadata_command,
            recording_runner(&called),
        );

        assert_eq!(exit_code, 0);
        assert!(!called.get());
    }

    #[test]
    fn run_cli_no_doctor_from_runs_domain_handler_for_non_metadata_command() {
        let _guard = env_lock();
        set_tokens(None, None);
        let env = env_from_detected();
        let called = Cell::new(false);

        let exit_code = run_cli_no_doctor_from::<TestCli, _, _, _>(
            &TOOL_SPEC,
            &env,
            ["tool", "query"],
            metadata_command,
            recording_runner(&called),
        );

        assert_eq!(exit_code, 9);
        assert!(called.get());
    }

    #[test]
    fn run_cli_no_doctor_from_short_circuits_agent_help_output() {
        let _guard = env_lock();
        set_tokens(Some("shared-token"), Some("shared-token"));
        let env = env_from_detected();
        let called = Cell::new(false);

        let exit_code = run_cli_no_doctor_from::<TestCli, _, _, _>(
            &TOOL_SPEC,
            &env,
            ["tool", "--agent-help"],
            metadata_command,
            recording_runner(&called),
        );

        assert_eq!(exit_code, 0);
        assert!(!called.get());
        set_tokens(None, None);
    }
}
