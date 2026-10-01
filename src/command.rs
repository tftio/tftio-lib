//! Shared standard CLI commands.

use clap::{CommandFactory, FromArgMatches};
use clap_complete::Shell;

use crate::{
    AgentDispatch, AgentModeContext, DoctorChecks, JsonOutput, ProcessEnv, ToolSpec,
    agent::AgentSubcommand, agent_skill::run_agent_subcommand, apply_agent_surface,
    display_license, doctor::run_doctor_with_output, generate_completions_from_command,
    parse_with_agent_surface_from,
};
#[cfg(test)]
use crate::{CompletionOutput, render_completion_from_command};

/// Shared doctorless adapter for tools that do not expose a doctor command.
pub struct NoDoctor;

impl DoctorChecks for NoDoctor {
    fn repo_info() -> crate::RepoInfo {
        crate::app::WORKSPACE_REPO
    }

    fn current_version() -> &'static str {
        "unknown"
    }
}

/// Shared metadata commands exposed by workspace binaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StandardCommand {
    /// Show version information.
    Version {
        /// Output format to render.
        output: JsonOutput,
    },
    /// Show license text.
    License,
    /// Generate shell completion scripts.
    Completions {
        /// Shell to generate completions for.
        shell: Shell,
    },
    /// Run health checks.
    Doctor {
        /// Output format to render.
        output: JsonOutput,
    },
    /// Inspect or emit agent-skill artifacts.
    Agent {
        /// Agent-skill subcommand to dispatch.
        command: AgentSubcommand,
    },
}

/// Map crate-local metadata commands onto the shared [`StandardCommand`] surface.
pub trait StandardCommandMap {
    /// Convert a local metadata command into a shared command.
    fn to_standard_command(&self, output: JsonOutput) -> StandardCommand;
}

/// Convert a crate-local metadata command into a shared [`StandardCommand`].
#[must_use]
pub fn map_standard_command<C>(command: &C, output: JsonOutput) -> StandardCommand
where
    C: StandardCommandMap + ?Sized,
{
    command.to_standard_command(output)
}

/// Run a mapped standard command when a tool exposes one.
#[must_use]
#[allow(
    clippy::single_option_map,
    reason = "ergonomic entry point: callers pass Option<command> and get the mapped run result; pushing the map to callers would duplicate it across every binary"
)]
pub fn maybe_run_standard_command<T, D, C>(
    spec: &ToolSpec,
    env: &ProcessEnv,
    command: Option<&C>,
    output: JsonOutput,
    doctor: Option<&D>,
) -> Option<i32>
where
    T: CommandFactory,
    D: DoctorChecks,
    C: StandardCommandMap + ?Sized,
{
    command.map(|command| {
        run_standard_command::<T, D>(spec, env, &map_standard_command(command, output), doctor)
    })
}

/// Run a mapped standard command for a tool with no doctor support.
#[must_use]
#[allow(
    clippy::single_option_map,
    reason = "ergonomic entry point: callers pass Option<command> and get the mapped run result; pushing the map to callers would duplicate it across every binary"
)]
pub fn maybe_run_standard_command_no_doctor<T, C>(
    spec: &ToolSpec,
    env: &ProcessEnv,
    command: Option<&C>,
    output: JsonOutput,
) -> Option<i32>
where
    T: CommandFactory,
    C: StandardCommandMap + ?Sized,
{
    command.map(|command| {
        run_standard_command_no_doctor::<T>(spec, env, &map_standard_command(command, output))
    })
}

/// Parse argv against the current normal or agent-filtered surface.
///
/// # Errors
///
/// Returns a `clap` error when parsing fails.
pub fn parse_command_with_agent_surface_from<T, I>(
    spec: &ToolSpec,
    ctx: &AgentModeContext,
    argv: I,
) -> Result<AgentDispatch<T>, clap::Error>
where
    T: CommandFactory + FromArgMatches,
    I: IntoIterator,
    I::Item: Into<std::ffi::OsString> + Clone,
{
    parse_with_agent_surface_from(spec, ctx, argv)
}

/// Parse argv and hand the typed CLI to a borrowed callback when parsing succeeds.
///
/// # Errors
///
/// Returns a `clap` error when parsing fails.
pub fn parse_command_ref_with_agent_surface_from<T, I, R, F>(
    spec: &ToolSpec,
    ctx: &AgentModeContext,
    argv: I,
    run: F,
) -> Result<AgentDispatch<R>, clap::Error>
where
    T: CommandFactory + FromArgMatches,
    I: IntoIterator,
    I::Item: Into<std::ffi::OsString> + Clone,
    F: FnOnce(&T) -> R,
{
    match parse_with_agent_surface_from(spec, ctx, argv)? {
        AgentDispatch::Cli(cli) => Ok(AgentDispatch::Cli(run(&cli))),
        AgentDispatch::Printed(code) => Ok(AgentDispatch::Printed(code)),
    }
}

fn render_version(spec: &ToolSpec, output: JsonOutput) -> String {
    if output.is_json() {
        format!(r#"{{"version":"{}"}}"#, spec.version)
    } else {
        format!("{} {}", spec.bin_name, spec.version)
    }
}

fn render_license(spec: &ToolSpec) -> String {
    display_license(spec.bin_name, spec.license)
}

fn completion_command_for_spec<T>(spec: &ToolSpec, ctx: AgentModeContext) -> clap::Command
where
    T: CommandFactory,
{
    let mut command = T::command();
    if ctx.active {
        apply_agent_surface(&mut command, spec, &ctx);
    }
    command
}

#[cfg(test)]
fn render_standard_completion_for_command<T>(
    spec: &ToolSpec,
    ctx: AgentModeContext,
    shell: Shell,
) -> CompletionOutput
where
    T: CommandFactory,
{
    render_completion_from_command(shell, completion_command_for_spec::<T>(spec, ctx))
}

fn generate_standard_completion_for_command<T>(
    spec: &ToolSpec,
    ctx: AgentModeContext,
    shell: Shell,
) -> std::io::Result<()>
where
    T: CommandFactory,
{
    generate_completions_from_command(shell, completion_command_for_spec::<T>(spec, ctx))
}

/// Execute a shared standard command.
#[must_use]
pub fn run_standard_command<T, D>(
    spec: &ToolSpec,
    env: &ProcessEnv,
    command: &StandardCommand,
    doctor: Option<&D>,
) -> i32
where
    T: CommandFactory,
    D: DoctorChecks,
{
    match command {
        StandardCommand::Version { output } => {
            println!("{}", render_version(spec, *output));
            0
        }
        StandardCommand::License => {
            println!("{}", render_license(spec));
            0
        }
        StandardCommand::Completions { shell } => {
            match generate_standard_completion_for_command::<T>(spec, env.agent, *shell) {
                Ok(()) => 0,
                // A closed stdout pipe is the only realistic failure; report it
                // via exit code rather than panicking.
                Err(_) => 1,
            }
        }
        StandardCommand::Doctor { output } => {
            let Some(tool) = doctor else {
                eprintln!("doctor support not configured");
                return 1;
            };
            run_doctor_with_output(tool, *output)
        }
        StandardCommand::Agent { command } => run_agent_subcommand(spec, env, command),
    }
}

/// Execute a shared standard command for a tool with no doctor support.
#[must_use]
pub fn run_standard_command_no_doctor<T>(
    spec: &ToolSpec,
    env: &ProcessEnv,
    command: &StandardCommand,
) -> i32
where
    T: CommandFactory,
{
    run_standard_command::<T, NoDoctor>(spec, env, command, None)
}

/// Implement [`StandardCommandMap`] for a crate-local metadata enum that uses the
/// workspace-standard variant names.
#[macro_export]
macro_rules! impl_standard_command_map {
    ($type:ty, global_json $(,)?) => {
        impl $crate::command::StandardCommandMap for $type {
            fn to_standard_command(&self, output: $crate::JsonOutput) -> $crate::StandardCommand {
                match self {
                    Self::Version => $crate::StandardCommand::Version { output },
                    Self::License => $crate::StandardCommand::License,
                    Self::Completions { shell } => {
                        $crate::StandardCommand::Completions { shell: *shell }
                    }
                }
            }
        }
    };
    ($type:ty, global_json, doctor $(,)?) => {
        impl $crate::command::StandardCommandMap for $type {
            fn to_standard_command(&self, output: $crate::JsonOutput) -> $crate::StandardCommand {
                match self {
                    Self::Version => $crate::StandardCommand::Version { output },
                    Self::License => $crate::StandardCommand::License,
                    Self::Completions { shell } => {
                        $crate::StandardCommand::Completions { shell: *shell }
                    }
                    Self::Doctor => $crate::StandardCommand::Doctor { output },
                }
            }
        }
    };
    ($type:ty, field_json $(,)?) => {
        impl $crate::command::StandardCommandMap for $type {
            fn to_standard_command(&self, _output: $crate::JsonOutput) -> $crate::StandardCommand {
                match self {
                    Self::Version { json } => $crate::StandardCommand::Version {
                        output: $crate::JsonOutput::from_flag(*json),
                    },
                    Self::License => $crate::StandardCommand::License,
                    Self::Completions { shell } => {
                        $crate::StandardCommand::Completions { shell: *shell }
                    }
                }
            }
        }
    };
    ($type:ty, field_json, doctor $(,)?) => {
        impl $crate::command::StandardCommandMap for $type {
            fn to_standard_command(&self, _output: $crate::JsonOutput) -> $crate::StandardCommand {
                match self {
                    Self::Version { json } => $crate::StandardCommand::Version {
                        output: $crate::JsonOutput::from_flag(*json),
                    },
                    Self::License => $crate::StandardCommand::License,
                    Self::Completions { shell } => {
                        $crate::StandardCommand::Completions { shell: *shell }
                    }
                    Self::Doctor { json } => $crate::StandardCommand::Doctor {
                        output: $crate::JsonOutput::from_flag(*json),
                    },
                }
            }
        }
    };
    ($type:ty, fixed_json = $json:expr $(,)?) => {
        impl $crate::command::StandardCommandMap for $type {
            fn to_standard_command(&self, _output: $crate::JsonOutput) -> $crate::StandardCommand {
                match self {
                    Self::Version => $crate::StandardCommand::Version {
                        output: $crate::JsonOutput::from_flag($json),
                    },
                    Self::License => $crate::StandardCommand::License,
                    Self::Completions { shell } => {
                        $crate::StandardCommand::Completions { shell: *shell }
                    }
                }
            }
        }
    };
    ($type:ty, fixed_json = $json:expr, doctor $(,)?) => {
        impl $crate::command::StandardCommandMap for $type {
            fn to_standard_command(&self, _output: $crate::JsonOutput) -> $crate::StandardCommand {
                match self {
                    Self::Version => $crate::StandardCommand::Version {
                        output: $crate::JsonOutput::from_flag($json),
                    },
                    Self::License => $crate::StandardCommand::License,
                    Self::Completions { shell } => {
                        $crate::StandardCommand::Completions { shell: *shell }
                    }
                    Self::Doctor => $crate::StandardCommand::Doctor {
                        output: $crate::JsonOutput::from_flag($json),
                    },
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use clap::{Parser, Subcommand};

    use super::*;
    use crate::{
        AGENT_TOKEN_ENV, AGENT_TOKEN_EXPECTED_ENV, AgentCapability, AgentDispatch,
        AgentSurfaceSpec, CommandSelector, FlagSelector, LicenseType, ListFormat, RepoInfo,
        ToolContract, test_support::env_lock, workspace_tool,
    };

    const QUERY_COMMAND: CommandSelector = CommandSelector::new(&["query"]);
    const QUERY_LIMIT_FLAG: FlagSelector = FlagSelector::new(&["query"], "limit");
    const QUERY_CAPABILITY: AgentCapability = AgentCapability::new(
        "query-posts",
        "Read paginated post records",
        &[QUERY_COMMAND],
        &[QUERY_LIMIT_FLAG],
    );
    const AGENT_SURFACE: AgentSurfaceSpec = AgentSurfaceSpec::new(&[QUERY_CAPABILITY]);

    #[derive(Parser)]
    struct TestCli;

    #[derive(Debug, Parser, PartialEq, Eq)]
    #[command(name = "tool")]
    struct ParseTestCli {
        #[command(subcommand)]
        command: ParseTestCommand,
    }

    #[derive(Debug, Subcommand, PartialEq, Eq)]
    enum ParseTestCommand {
        Query {
            #[arg(long)]
            limit: u32,
        },
        Admin,
    }

    #[derive(Debug, Parser, PartialEq, Eq)]
    #[command(name = "tool")]
    struct CompletionTestCli {
        #[command(subcommand)]
        command: CompletionTestCommand,
    }

    #[derive(Debug, Subcommand, PartialEq, Eq)]
    enum CompletionTestCommand {
        Query {
            #[arg(long)]
            limit: Option<u32>,
            #[arg(long)]
            secret: bool,
        },
        Admin,
    }

    struct TestDoctor;

    impl DoctorChecks for TestDoctor {
        fn repo_info() -> RepoInfo {
            RepoInfo::new("owner", "doctor-tool")
        }

        fn current_version() -> &'static str {
            "1.0.0"
        }
    }

    fn spec() -> ToolSpec {
        ToolSpec::new(
            "tool",
            "Tool",
            "1.2.3",
            LicenseType::MIT,
            RepoInfo::new("owner", "repo"),
            true,
            true,
        )
    }

    fn agent_spec() -> ToolSpec {
        workspace_tool("tool", "Tool", "1.2.3", LicenseType::MIT, true, true)
            .with_agent_surface(&AGENT_SURFACE)
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

    fn detect_from_env() -> AgentModeContext {
        AgentModeContext::from_tokens(
            std::env::var(AGENT_TOKEN_ENV).ok(),
            std::env::var(AGENT_TOKEN_EXPECTED_ENV).ok(),
        )
    }

    fn env_from_detected() -> ProcessEnv {
        ProcessEnv {
            agent: detect_from_env(),
            home: None,
        }
    }

    fn inactive_env() -> ProcessEnv {
        ProcessEnv::default()
    }

    #[test]
    fn version_json_contains_version_key() {
        let rendered = render_version(&spec(), JsonOutput::Json);
        assert!(rendered.contains("\"version\""));
    }

    #[test]
    fn spec_with_all_capabilities_is_marked_authoritative() {
        assert_eq!(agent_spec().contract, ToolContract::CliCommonBase);
        assert!(agent_spec().has_authoritative_contract());
    }

    #[test]
    fn license_render_uses_display_license_text() {
        let rendered = render_license(&spec());
        assert!(rendered.contains("MIT License"));
    }

    #[test]
    fn run_standard_command_version_returns_success() {
        let exit_code = run_standard_command::<TestCli, TestDoctor>(
            &spec(),
            &inactive_env(),
            &StandardCommand::Version {
                output: JsonOutput::Text,
            },
            Some(&TestDoctor),
        );
        assert_eq!(exit_code, 0);
    }

    #[test]
    fn run_standard_command_no_doctor_version_returns_success() {
        let exit_code = run_standard_command_no_doctor::<TestCli>(
            &spec(),
            &inactive_env(),
            &StandardCommand::Version {
                output: JsonOutput::Json,
            },
        );
        assert_eq!(exit_code, 0);
    }

    #[allow(dead_code)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum GlobalJsonMetaCommand {
        Version,
        License,
        Completions { shell: Shell },
    }

    impl_standard_command_map!(GlobalJsonMetaCommand, global_json);

    #[allow(dead_code)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum FixedJsonMetaCommand {
        Version,
        License,
        Completions { shell: Shell },
        Doctor,
    }

    impl_standard_command_map!(FixedJsonMetaCommand, fixed_json = false, doctor);

    #[allow(dead_code)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum VersionFieldMetaCommand {
        Version { json: bool },
        License,
        Completions { shell: Shell },
    }

    impl_standard_command_map!(VersionFieldMetaCommand, field_json);

    #[test]
    fn impl_standard_command_map_uses_global_json_flag() {
        let command = map_standard_command(&GlobalJsonMetaCommand::Version, JsonOutput::Json);
        assert_eq!(
            command,
            StandardCommand::Version {
                output: JsonOutput::Json
            }
        );
    }

    #[test]
    fn impl_standard_command_map_supports_fixed_json_and_doctor_variants() {
        let command = map_standard_command(&FixedJsonMetaCommand::Doctor, JsonOutput::Json);
        assert_eq!(
            command,
            StandardCommand::Doctor {
                output: JsonOutput::Text
            }
        );
    }

    #[allow(dead_code)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum FieldJsonDoctorMetaCommand {
        Version { json: bool },
        License,
        Completions { shell: Shell },
        Doctor { json: bool },
    }

    impl_standard_command_map!(FieldJsonDoctorMetaCommand, field_json, doctor);

    #[test]
    fn impl_standard_command_map_reads_json_from_version_field() {
        let command = map_standard_command(
            &VersionFieldMetaCommand::Version { json: true },
            JsonOutput::Text,
        );
        assert_eq!(
            command,
            StandardCommand::Version {
                output: JsonOutput::Json
            }
        );
    }

    #[test]
    fn impl_standard_command_map_reads_json_from_doctor_field() {
        let command = map_standard_command(
            &FieldJsonDoctorMetaCommand::Doctor { json: true },
            JsonOutput::Text,
        );
        assert_eq!(
            command,
            StandardCommand::Doctor {
                output: JsonOutput::Json
            }
        );
    }

    #[test]
    fn maybe_run_standard_command_no_doctor_executes_mapped_metadata_command() {
        let exit_code = maybe_run_standard_command_no_doctor::<TestCli, _>(
            &spec(),
            &inactive_env(),
            Some(&GlobalJsonMetaCommand::License),
            JsonOutput::Text,
        );
        assert_eq!(exit_code, Some(0));
    }

    #[test]
    fn maybe_run_standard_command_returns_none_without_metadata_command() {
        let exit_code = maybe_run_standard_command_no_doctor::<TestCli, GlobalJsonMetaCommand>(
            &spec(),
            &inactive_env(),
            None,
            JsonOutput::Text,
        );
        assert_eq!(exit_code, None);
    }

    #[test]
    fn parse_command_with_agent_surface_from_returns_owned_cli() {
        let _guard = env_lock();
        set_tokens(None, None);
        let ctx = detect_from_env();

        let parsed = parse_command_with_agent_surface_from::<ParseTestCli, _>(
            &agent_spec(),
            &ctx,
            ["tool", "query", "--limit", "5"],
        )
        .expect("parse should succeed");

        assert_eq!(
            parsed,
            AgentDispatch::Cli(ParseTestCli {
                command: ParseTestCommand::Query { limit: 5 },
            })
        );
    }

    fn parse_test_limit(cli: &ParseTestCli) -> u32 {
        match cli.command {
            ParseTestCommand::Query { limit } => limit,
            ParseTestCommand::Admin => 0,
        }
    }

    #[test]
    fn parse_command_ref_with_agent_surface_from_borrows_cli() {
        let _guard = env_lock();
        set_tokens(Some("shared-token"), Some("shared-token"));
        let ctx = detect_from_env();

        let parsed = parse_command_ref_with_agent_surface_from::<ParseTestCli, _, _, _>(
            &agent_spec(),
            &ctx,
            ["tool", "query", "--limit", "7"],
            parse_test_limit,
        )
        .expect("parse should succeed");

        assert_eq!(parsed, AgentDispatch::Cli(7));
    }

    #[test]
    fn parse_command_ref_with_agent_surface_from_maps_admin_branch() {
        let ctx = AgentModeContext::default();

        let parsed = parse_command_ref_with_agent_surface_from::<ParseTestCli, _, u32, _>(
            &agent_spec(),
            &ctx,
            ["tool", "admin"],
            parse_test_limit,
        )
        .expect("parse should succeed");

        assert_eq!(parsed, AgentDispatch::Cli(0));
    }

    #[test]
    fn parse_command_ref_with_agent_surface_from_returns_printed_for_agent_help() {
        let _guard = env_lock();
        set_tokens(Some("shared-token"), Some("shared-token"));
        let ctx = detect_from_env();

        let parsed = parse_command_ref_with_agent_surface_from::<ParseTestCli, _, u32, _>(
            &agent_spec(),
            &ctx,
            ["tool", "--agent-help"],
            parse_test_limit,
        )
        .expect("parse should succeed");

        assert_eq!(parsed, AgentDispatch::Printed(0));
        set_tokens(None, None);
    }

    #[test]
    fn agent_surface_redaction_completion_metadata_path_omits_hidden_entries() {
        let _guard = env_lock();
        set_tokens(Some("shared-token"), Some("shared-token"));
        let env = env_from_detected();

        let output = render_standard_completion_for_command::<CompletionTestCli>(
            &agent_spec(),
            env.agent,
            Shell::Bash,
        );

        assert!(output.script.contains("query"));
        assert!(!output.script.contains("admin"));
        assert!(!output.script.contains("--secret"));
    }

    #[test]
    fn no_doctor_reports_workspace_repo_and_unknown_version() {
        let repo = NoDoctor::repo_info();
        assert_eq!(repo.owner, "tftio");
        assert_eq!(repo.name, "tftio-lib");
        assert_eq!(NoDoctor::current_version(), "unknown");
    }

    #[test]
    fn maybe_run_standard_command_executes_mapped_metadata_command_with_doctor() {
        let exit_code = maybe_run_standard_command::<TestCli, TestDoctor, _>(
            &spec(),
            &inactive_env(),
            Some(&FixedJsonMetaCommand::License),
            JsonOutput::Text,
            Some(&TestDoctor),
        );
        assert_eq!(exit_code, Some(0));
    }

    #[test]
    fn run_standard_command_completions_returns_success() {
        let exit_code = run_standard_command::<TestCli, TestDoctor>(
            &spec(),
            &inactive_env(),
            &StandardCommand::Completions { shell: Shell::Bash },
            Some(&TestDoctor),
        );
        assert_eq!(exit_code, 0);
    }

    #[test]
    fn run_standard_command_no_doctor_doctor_reports_missing_support() {
        let exit_code = run_standard_command_no_doctor::<TestCli>(
            &spec(),
            &inactive_env(),
            &StandardCommand::Doctor {
                output: JsonOutput::Text,
            },
        );
        assert_eq!(exit_code, 1);
    }

    #[test]
    fn run_standard_command_doctor_invokes_configured_checks() {
        let exit_code = run_standard_command::<TestCli, TestDoctor>(
            &spec(),
            &inactive_env(),
            &StandardCommand::Doctor {
                output: JsonOutput::Text,
            },
            Some(&TestDoctor),
        );
        assert_eq!(exit_code, 0);
    }

    #[test]
    fn run_standard_command_agent_lists_capabilities() {
        let exit_code = run_standard_command::<TestCli, TestDoctor>(
            &agent_spec(),
            &inactive_env(),
            &StandardCommand::Agent {
                command: AgentSubcommand::List {
                    format: ListFormat::Text,
                },
            },
            Some(&TestDoctor),
        );
        assert_eq!(exit_code, 0);
    }

    #[test]
    fn global_json_map_supports_completions_variant() {
        let command = map_standard_command(
            &GlobalJsonMetaCommand::Completions { shell: Shell::Bash },
            JsonOutput::Json,
        );
        assert_eq!(command, StandardCommand::Completions { shell: Shell::Bash });
    }

    #[test]
    fn field_json_map_supports_license_and_completions_variants() {
        assert_eq!(
            map_standard_command(&VersionFieldMetaCommand::License, JsonOutput::Json),
            StandardCommand::License
        );
        assert_eq!(
            map_standard_command(
                &VersionFieldMetaCommand::Completions { shell: Shell::Zsh },
                JsonOutput::Json
            ),
            StandardCommand::Completions { shell: Shell::Zsh }
        );
    }

    #[test]
    fn field_json_doctor_map_supports_non_doctor_variants() {
        assert_eq!(
            map_standard_command(
                &FieldJsonDoctorMetaCommand::Version { json: false },
                JsonOutput::Json
            ),
            StandardCommand::Version {
                output: JsonOutput::Text
            }
        );
        assert_eq!(
            map_standard_command(&FieldJsonDoctorMetaCommand::License, JsonOutput::Text),
            StandardCommand::License
        );
        assert_eq!(
            map_standard_command(
                &FieldJsonDoctorMetaCommand::Completions { shell: Shell::Fish },
                JsonOutput::Text
            ),
            StandardCommand::Completions { shell: Shell::Fish }
        );
    }

    #[test]
    fn fixed_json_doctor_map_supports_non_doctor_variants() {
        assert_eq!(
            map_standard_command(&FixedJsonMetaCommand::Version, JsonOutput::Json),
            StandardCommand::Version {
                output: JsonOutput::Text
            }
        );
        assert_eq!(
            map_standard_command(&FixedJsonMetaCommand::License, JsonOutput::Json),
            StandardCommand::License
        );
        assert_eq!(
            map_standard_command(
                &FixedJsonMetaCommand::Completions { shell: Shell::Bash },
                JsonOutput::Json
            ),
            StandardCommand::Completions { shell: Shell::Bash }
        );
    }
}
