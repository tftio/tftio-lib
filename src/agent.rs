//! Shared agent-mode capability declarations and filtering helpers.

use std::{collections::BTreeSet, ffi::OsString};

use clap::{Arg, ArgAction, Command, CommandFactory, FromArgMatches, error::ErrorKind};

use crate::ToolSpec;

/// Environment variable containing the presented agent token.
pub const AGENT_TOKEN_ENV: &str = "TFTIO_AGENT_TOKEN";

/// Environment variable containing the expected agent token.
pub const AGENT_TOKEN_EXPECTED_ENV: &str = "TFTIO_AGENT_TOKEN_EXPECTED";

/// Shared process-level agent-mode context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AgentModeContext {
    /// Whether agent mode is active for the current process.
    pub active: bool,
}

impl AgentModeContext {
    /// Construct an agent-mode context from edge-read token values.
    ///
    /// Agent mode is active only when both the presented and expected tokens
    /// are present and equal. Reading the tokens from the environment is the
    /// binary edge's responsibility (`REPO_INVARIANTS.md` ENG-008).
    #[must_use]
    pub fn from_tokens(presented: Option<String>, expected: Option<String>) -> Self {
        Self {
            active: matches!((presented, expected), (Some(presented), Some(expected)) if presented == expected),
        }
    }
}

/// Process-edge environment values threaded inward from each binary's `main`.
///
/// Per `REPO_INVARIANTS.md` ENG-008 the environment is read once at the binary edge
/// and passed inward as typed values. This bundle carries those reads so the
/// shared library code never touches `std::env`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProcessEnv {
    /// Agent-mode context derived from the agent token environment variables.
    pub agent: AgentModeContext,
    /// The process `HOME` directory, if set.
    pub home: Option<std::path::PathBuf>,
}

/// Declarative capability surface for a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentSurfaceSpec {
    /// Named capabilities visible to the agent surface.
    pub(crate) capabilities: &'static [AgentCapability],
    /// Lifecycle hooks declared alongside the capabilities.
    pub(crate) hooks: &'static [AgentHook],
}

impl AgentSurfaceSpec {
    /// Create a new [`AgentSurfaceSpec`] with no declared hooks.
    #[must_use]
    pub const fn new(capabilities: &'static [AgentCapability]) -> Self {
        Self {
            capabilities,
            hooks: &[],
        }
    }

    /// Attach lifecycle hooks to the agent surface.
    #[must_use]
    pub const fn with_hooks(self, hooks: &'static [AgentHook]) -> Self {
        Self { hooks, ..self }
    }

    /// Return the capabilities in this surface.
    #[must_use]
    pub const fn capabilities(&self) -> &'static [AgentCapability] {
        self.capabilities
    }

    /// Return the lifecycle hooks in this surface.
    #[must_use]
    pub const fn hooks(&self) -> &'static [AgentHook] {
        self.hooks
    }
}

/// A harness lifecycle event a hook can be registered against.
///
/// Closed by design: adding a new harness event a tool can hook into is a
/// deliberate act, not an incidental one, so every consumer that dispatches
/// on [`HookEvent`] is forced to decide what the new variant means for it
/// (in particular [`crate::agent_hook`]'s per-target event-key mapping,
/// which decides whether a harness has an equivalent for the event at all).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookEvent {
    /// The harness session has ended.
    SessionEnd,
    /// The harness turn is about to stop.
    Stop,
}

/// A declared lifecycle hook: a script a tool wants a harness to run on one
/// of its lifecycle events.
///
/// Mirrors [`AgentCapability`]'s role for skills: a tool declares hooks
/// alongside its capabilities on its [`AgentSurfaceSpec`], and `meta agent
/// emit-hooks` (see [`crate::agent_hook`]) writes the embedded `script` to
/// disk and prints the target harness's registration fragment. `script` is
/// typically produced with `include_str!` so the file a tool tests is the
/// file the binary emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentHook {
    /// Stable hook name; also the emitted script's file stem (`<name>.sh`).
    pub(crate) name: &'static str,
    /// Harness lifecycle event that triggers the hook.
    pub(crate) event: HookEvent,
    /// Embedded script contents, typically produced with `include_str!`.
    pub(crate) script: &'static str,
    /// Harness-side timeout, in seconds, for running the hook.
    pub(crate) timeout: u32,
    /// Optional status message shown while the hook runs (Claude Code only).
    pub(crate) status_message: Option<&'static str>,
}

impl AgentHook {
    /// Create a new [`AgentHook`] with no status message.
    #[must_use]
    pub const fn new(
        name: &'static str,
        event: HookEvent,
        script: &'static str,
        timeout: u32,
    ) -> Self {
        Self {
            name,
            event,
            script,
            timeout,
            status_message: None,
        }
    }

    /// Attach a status message shown while the hook runs.
    #[must_use]
    pub const fn with_status_message(self, status_message: &'static str) -> Self {
        Self {
            status_message: Some(status_message),
            ..self
        }
    }

    /// Return the hook name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Return the harness lifecycle event that triggers this hook.
    #[must_use]
    pub const fn event(&self) -> HookEvent {
        self.event
    }

    /// Return the embedded script contents.
    #[must_use]
    pub const fn script(&self) -> &'static str {
        self.script
    }

    /// Return the harness-side timeout, in seconds.
    #[must_use]
    pub const fn timeout(&self) -> u32 {
        self.timeout
    }

    /// Return the status message, if any.
    #[must_use]
    pub const fn status_message(&self) -> Option<&'static str> {
        self.status_message
    }
}

/// Declarative capability group for agent-mode visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentCapability {
    /// Stable capability name.
    pub(crate) name: &'static str,
    /// Optional human-readable summary for the capability.
    pub(crate) summary: Option<&'static str>,
    /// Command paths exposed by this capability.
    pub(crate) commands: &'static [CommandSelector],
    /// Long flags exposed by this capability.
    pub(crate) flags: &'static [FlagSelector],
    /// Optional example invocations.
    pub(crate) examples: Option<&'static [&'static str]>,
    /// Optional output contract prose.
    pub(crate) output: Option<&'static str>,
    /// Optional constraints prose.
    pub(crate) constraints: Option<&'static str>,
    /// Optional prose describing when an agent should reach for this capability.
    pub(crate) when_to_use: Option<&'static str>,
    /// Optional prose describing when an agent should NOT reach for this capability.
    pub(crate) when_not_to_use: Option<&'static str>,
}

impl AgentCapability {
    /// Return the capability name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Return the capability summary, if any.
    #[must_use]
    pub const fn summary(&self) -> Option<&'static str> {
        self.summary
    }

    /// Return the command selectors for this capability.
    #[must_use]
    pub const fn commands(&self) -> &'static [CommandSelector] {
        self.commands
    }

    /// Return the flag selectors for this capability.
    #[must_use]
    pub const fn flags(&self) -> &'static [FlagSelector] {
        self.flags
    }

    /// Return example invocations, if any.
    #[must_use]
    pub const fn examples(&self) -> Option<&'static [&'static str]> {
        self.examples
    }

    /// Return the output contract prose, if any.
    #[must_use]
    pub const fn output(&self) -> Option<&'static str> {
        self.output
    }

    /// Return the constraints prose, if any.
    #[must_use]
    pub const fn constraints(&self) -> Option<&'static str> {
        self.constraints
    }

    /// Return the when-to-use prose, if any.
    #[must_use]
    pub const fn when_to_use(&self) -> Option<&'static str> {
        self.when_to_use
    }

    /// Return the when-not-to-use prose, if any.
    #[must_use]
    pub const fn when_not_to_use(&self) -> Option<&'static str> {
        self.when_not_to_use
    }

    /// Create a new [`AgentCapability`].
    #[must_use]
    pub const fn new(
        name: &'static str,
        summary: &'static str,
        commands: &'static [CommandSelector],
        flags: &'static [FlagSelector],
    ) -> Self {
        Self {
            name,
            summary: Some(summary),
            commands,
            flags,
            examples: None,
            output: None,
            constraints: None,
            when_to_use: None,
            when_not_to_use: None,
        }
    }

    /// Create a new [`AgentCapability`] without optional prose metadata.
    #[must_use]
    pub const fn minimal(
        name: &'static str,
        commands: &'static [CommandSelector],
        flags: &'static [FlagSelector],
    ) -> Self {
        Self {
            name,
            summary: None,
            commands,
            flags,
            examples: None,
            output: None,
            constraints: None,
            when_to_use: None,
            when_not_to_use: None,
        }
    }

    /// Attach example invocations.
    #[must_use]
    pub const fn with_examples(self, examples: &'static [&'static str]) -> Self {
        Self {
            examples: Some(examples),
            ..self
        }
    }

    /// Attach output contract prose.
    #[must_use]
    pub const fn with_output(self, output: &'static str) -> Self {
        Self {
            output: Some(output),
            ..self
        }
    }

    /// Attach constraints prose.
    #[must_use]
    pub const fn with_constraints(self, constraints: &'static str) -> Self {
        Self {
            constraints: Some(constraints),
            ..self
        }
    }

    /// Attach when-to-use prose.
    #[must_use]
    pub const fn with_when_to_use(self, when_to_use: &'static str) -> Self {
        Self {
            when_to_use: Some(when_to_use),
            ..self
        }
    }

    /// Attach when-not-to-use prose.
    #[must_use]
    pub const fn with_when_not_to_use(self, when_not_to_use: &'static str) -> Self {
        Self {
            when_not_to_use: Some(when_not_to_use),
            ..self
        }
    }
}

/// Declarative selector for a command path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSelector {
    /// Path segments for the selected command.
    pub(crate) path: &'static [&'static str],
}

impl CommandSelector {
    /// Create a new [`CommandSelector`].
    #[must_use]
    pub const fn new(path: &'static [&'static str]) -> Self {
        Self { path }
    }

    /// Return the path segments.
    #[must_use]
    pub const fn path(&self) -> &'static [&'static str] {
        self.path
    }
}

/// Declarative selector for a long flag on a command path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlagSelector {
    /// Command path that owns the flag.
    pub(crate) command_path: &'static [&'static str],
    /// Long flag name without the leading `--`.
    pub(crate) long: &'static str,
}

impl FlagSelector {
    /// Create a new [`FlagSelector`].
    #[must_use]
    pub const fn new(command_path: &'static [&'static str], long: &'static str) -> Self {
        Self { command_path, long }
    }

    /// Return the command path that owns this flag.
    #[must_use]
    pub const fn command_path(&self) -> &'static [&'static str] {
        self.command_path
    }

    /// Return the long flag name without the leading `--`.
    #[must_use]
    pub const fn long(&self) -> &'static str {
        self.long
    }
}

/// Shared parse result for agent-aware entrypoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentDispatch<T> {
    /// Continue with the parsed CLI value.
    Cli(T),
    /// A shared agent inspection path printed output and chose an exit code.
    Printed(i32),
}

/// Error returned when rendering a single agent capability view fails.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum AgentSkillError {
    /// The requested capability name is not visible in the current context.
    #[error("unknown agent capability: {0}")]
    UnknownCapability(String),
}

/// Return the capabilities visible in the current agent-mode context.
#[must_use]
pub fn visible_capabilities<'a>(
    spec: &'a ToolSpec,
    ctx: &AgentModeContext,
) -> &'a [AgentCapability] {
    if ctx.active {
        spec.agent_surface
            .map_or(&[], |surface| surface.capabilities)
    } else {
        &[]
    }
}

/// Apply the visible agent surface to a `clap` command tree.
pub fn apply_agent_surface(command: &mut Command, spec: &ToolSpec, ctx: &AgentModeContext) {
    if !ctx.active {
        return;
    }

    ensure_agent_inspection_args(command);

    let filtered = filter_command(command, spec.version, visible_capabilities(spec, ctx), &[]);
    *command = filtered;
}

fn filter_command(
    command: &Command,
    version: &'static str,
    capabilities: &[AgentCapability],
    current_path: &[&str],
) -> Command {
    let keep_full_subtree =
        is_within_explicit_command_subtree(capabilities, current_path, command.has_subcommands());
    let allowed_flags = allowed_flags(capabilities, current_path);
    let mut filtered = clone_command_metadata(command, version, current_path.is_empty());

    for arg in command
        .get_arguments()
        .filter(|arg| {
            should_keep_arg(
                arg,
                capabilities,
                current_path,
                &allowed_flags,
                keep_full_subtree,
            )
        })
        .cloned()
    {
        filtered = filtered.arg(arg);
    }

    if keep_full_subtree {
        for subcommand in command.get_subcommands() {
            let subcommand_name = subcommand.get_name();
            let next_path = extend_path_owned(current_path, subcommand_name);
            let next_path_refs = next_path.iter().map(String::as_str).collect::<Vec<_>>();
            filtered = filtered.subcommand(filter_command(
                subcommand,
                version,
                capabilities,
                &next_path_refs,
            ));
        }
        return filtered;
    }

    for subcommand_name in allowed_subcommands(capabilities, current_path) {
        if let Some(subcommand) = command.find_subcommand(subcommand_name) {
            let next_path = extend_path_owned(current_path, subcommand_name);
            let next_path_refs = next_path.iter().map(String::as_str).collect::<Vec<_>>();
            filtered = filtered.subcommand(filter_command(
                subcommand,
                version,
                capabilities,
                &next_path_refs,
            ));
        }
    }

    filtered
}

fn clone_command_metadata(
    command: &Command,
    version: &'static str,
    include_version: bool,
) -> Command {
    let mut filtered = Command::new(command.get_name().to_owned());

    if let Some(display_name) = command.get_display_name() {
        filtered = filtered.display_name(display_name.to_owned());
    }
    if include_version {
        filtered = filtered.version(version);
    }
    if let Some(about) = command.get_about() {
        filtered = filtered.about(about.clone());
    }
    if let Some(long_about) = command.get_long_about() {
        filtered = filtered.long_about(long_about.clone());
    }
    if let Some(before_help) = command.get_before_help() {
        filtered = filtered.before_help(before_help.clone());
    }
    if let Some(after_help) = command.get_after_help() {
        filtered = filtered.after_help(after_help.clone());
    }
    if command.is_disable_help_flag_set() {
        filtered = filtered.disable_help_flag(true);
    }
    if command.is_disable_help_subcommand_set() {
        filtered = filtered.disable_help_subcommand(true);
    }
    if command.is_disable_colored_help_set() {
        filtered = filtered.disable_colored_help(true);
    }
    if command.is_flatten_help_set() {
        filtered = filtered.flatten_help(true);
    }
    if let Some(bin_name) = command.get_bin_name() {
        filtered.set_bin_name(bin_name.to_owned());
    }

    filtered
}

fn allowed_flags(
    capabilities: &[AgentCapability],
    current_path: &[&str],
) -> BTreeSet<&'static str> {
    let mut flags = BTreeSet::new();

    for capability in capabilities {
        for selector in capability.flags {
            if selector.command_path == current_path {
                flags.insert(selector.long);
            }
        }
    }

    flags
}

fn allowed_subcommands(
    capabilities: &[AgentCapability],
    current_path: &[&str],
) -> BTreeSet<&'static str> {
    let mut subcommands = BTreeSet::new();

    for capability in capabilities {
        for selector in capability.commands {
            if selector.path.starts_with(current_path)
                && let Some(&segment) = selector.path.get(current_path.len())
            {
                subcommands.insert(segment);
            }
        }
    }

    subcommands
}

fn is_within_explicit_command_subtree(
    capabilities: &[AgentCapability],
    current_path: &[&str],
    command_has_subcommands: bool,
) -> bool {
    capabilities.iter().any(|capability| {
        capability.commands.iter().any(|selector| {
            !selector.path.is_empty()
                && current_path.starts_with(selector.path)
                && (selector.path.len() < current_path.len()
                    || (selector.path.len() == current_path.len() && command_has_subcommands))
        })
    })
}

fn should_keep_arg(
    arg: &Arg,
    capabilities: &[AgentCapability],
    current_path: &[&str],
    allowed_flags: &BTreeSet<&str>,
    keep_full_subtree: bool,
) -> bool {
    if is_shared_agent_flag(arg) {
        return true;
    }

    if arg.is_positional() {
        return capability_includes_command_path(capabilities, current_path);
    }

    if keep_full_subtree {
        return true;
    }

    arg.get_long()
        .is_some_and(|long| allowed_flags.contains(long))
}

fn capability_includes_command_path(
    capabilities: &[AgentCapability],
    current_path: &[&str],
) -> bool {
    capabilities
        .iter()
        .flat_map(|capability| capability.commands.iter())
        .any(|selector| selector.path == current_path)
}

fn is_shared_agent_flag(arg: &Arg) -> bool {
    matches!(arg.get_long(), Some("agent-help" | "agent-skill"))
}

fn ensure_agent_inspection_args(command: &mut Command) {
    if !command
        .get_arguments()
        .any(|arg| arg.get_long() == Some("agent-help"))
    {
        *command = command.clone().arg(
            Arg::new("agent-help")
                .long("agent-help")
                .help("Print the visible agent command surface")
                .hide(true)
                .global(true)
                .action(ArgAction::SetTrue),
        );
    }

    if !command
        .get_arguments()
        .any(|arg| arg.get_long() == Some("agent-skill"))
    {
        *command = command.clone().arg(
            Arg::new("agent-skill")
                .long("agent-skill")
                .help("Print the visible agent capability contract")
                .hide(true)
                .global(true)
                .value_name("NAME"),
        );
    }
}

fn extend_path_owned(current_path: &[&str], segment: &str) -> Vec<String> {
    let mut next_path = current_path
        .iter()
        .map(|part| (*part).to_owned())
        .collect::<Vec<_>>();
    next_path.push(segment.to_owned());
    next_path
}

/// Parse argv against the normal or agent-filtered surface for a clap CLI.
///
/// # Errors
///
/// Returns a `clap` error when parsing fails or when an unknown agent capability is requested.
pub fn parse_with_agent_surface_from<T, I>(
    spec: &ToolSpec,
    ctx: &AgentModeContext,
    argv: I,
) -> Result<AgentDispatch<T>, clap::Error>
where
    T: CommandFactory + FromArgMatches,
    I: IntoIterator,
    I::Item: Into<OsString> + Clone,
{
    let argv = rewrite_trailing_help_subcommand(argv.into_iter().map(Into::into).collect());
    let mut command = T::command();
    ensure_agent_inspection_args(&mut command);

    if ctx.active {
        apply_agent_surface(&mut command, spec, ctx);
    }

    match command.try_get_matches_from_mut(argv) {
        Ok(mut matches) => {
            if matches.get_flag("agent-help") {
                println!("{}", render_agent_help(spec, ctx));
                return Ok(AgentDispatch::Printed(0));
            }

            if let Some(name) = matches.get_one::<String>("agent-skill") {
                let text = render_agent_skill(spec, ctx, name)
                    .map_err(|error| command.error(ErrorKind::InvalidValue, error.to_string()))?;
                println!("{text}");
                return Ok(AgentDispatch::Printed(0));
            }

            T::from_arg_matches_mut(&mut matches).map(AgentDispatch::Cli)
        }
        Err(error) => Err(sanitize_agent_parse_error(error)),
    }
}

fn rewrite_trailing_help_subcommand(mut argv: Vec<OsString>) -> Vec<OsString> {
    if argv.last().is_some_and(|arg| arg == "help") {
        argv.pop();
        argv.push(OsString::from("--help"));
    }

    argv
}

fn sanitize_agent_parse_error(mut error: clap::Error) -> clap::Error {
    let rendered = error.to_string();
    if rendered.contains("Did you mean") {
        error = clap::Error::raw(error.kind(), strip_suggestion_lines(&rendered));
    }
    error
}

fn strip_suggestion_lines(rendered: &str) -> String {
    rendered
        .lines()
        .filter(|line| !line.contains("Did you mean"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Render the visible agent capability surface as structured text.
#[must_use]
pub fn render_agent_help(spec: &ToolSpec, ctx: &AgentModeContext) -> String {
    let capabilities = visible_capabilities(spec, ctx);
    let capability_lines = if capabilities.is_empty() {
        String::from("- none")
    } else {
        capabilities
            .iter()
            .map(|capability| format!("- {}: {}", capability.name, capability_summary(capability)))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let argument_lines = render_surface_arguments(capabilities);

    format!(
        "tool:\n- {}\nmode:\n- {}\ncapabilities:\n{}\narguments:\n{}\noutput:\n- structured plain text for the visible command surface\nconstraints:\n- output is limited to the currently visible surface",
        spec.bin_name,
        if ctx.active { "agent" } else { "human" },
        capability_lines,
        argument_lines,
    )
}

/// Render the visible contract for one agent capability.
///
/// # Errors
///
/// Returns [`AgentSkillError`] when the capability name is not visible in the current context.
pub fn render_agent_skill(
    spec: &ToolSpec,
    ctx: &AgentModeContext,
    name: &str,
) -> Result<String, AgentSkillError> {
    let capability = visible_capabilities(spec, ctx)
        .iter()
        .find(|capability| capability.name == name)
        .ok_or_else(|| AgentSkillError::UnknownCapability(name.to_owned()))?;

    Ok(format!(
        "tool:\n- {}\ncapability:\n- {}\nsummary:\n- {}\ncommands:\n{}\nflags:\n{}\nexamples:\n{}\noutput:\n- {}\nconstraints:\n- {}",
        spec.bin_name,
        capability.name,
        capability_summary(capability),
        render_command_lines(capability),
        render_flag_lines(capability),
        render_example_lines(capability),
        capability_output(capability),
        capability_constraints(capability),
    ))
}

fn capability_summary(capability: &AgentCapability) -> String {
    if let Some(summary) = capability.summary {
        return String::from(summary);
    }

    if let Some(primary_command) = capability.commands.first() {
        return format!(
            "Use {} via {}",
            capability.name.replace('-', " "),
            primary_command.path.join(" ")
        );
    }

    format!("Use {}", capability.name.replace('-', " "))
}

fn capability_output(capability: &AgentCapability) -> String {
    capability.output.map_or_else(
        || {
            capability.commands.first().map_or_else(
                || String::from("output follows the existing CLI contract"),
                |primary_command| {
                    format!(
                        "output follows the existing CLI contract for {}",
                        primary_command.path.join(" ")
                    )
                },
            )
        },
        String::from,
    )
}

fn capability_constraints(capability: &AgentCapability) -> String {
    capability.constraints.map_or_else(
        || String::from("existing command validation and auth rules still apply"),
        String::from,
    )
}

fn render_example_lines(capability: &AgentCapability) -> String {
    capability.examples.map_or_else(
        || String::from("- none declared"),
        |examples| {
            if examples.is_empty() {
                String::from("- none declared")
            } else {
                examples
                    .iter()
                    .map(|example| format!("- {example}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        },
    )
}

fn render_surface_arguments(capabilities: &[AgentCapability]) -> String {
    let mut lines = vec![
        String::from("- --agent-help"),
        String::from("- --agent-skill <NAME>"),
    ];

    for capability in capabilities {
        for command in capability.commands {
            lines.push(format!("- command {}", command.path.join(" ")));
        }
        for flag in capability.flags {
            let prefix = if flag.command_path.is_empty() {
                String::new()
            } else {
                format!("{} ", flag.command_path.join(" "))
            };
            lines.push(format!("- {prefix}--{}", flag.long));
        }
    }

    lines.join("\n")
}

fn render_command_lines(capability: &AgentCapability) -> String {
    if capability.commands.is_empty() {
        String::from("- none declared")
    } else {
        capability
            .commands
            .iter()
            .map(|selector| format!("- {}", selector.path.join(" ")))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn render_flag_lines(capability: &AgentCapability) -> String {
    if capability.flags.is_empty() {
        String::from("- none declared")
    } else {
        capability
            .flags
            .iter()
            .map(|selector| {
                if selector.command_path.is_empty() {
                    format!("- --{}", selector.long)
                } else {
                    format!("- {} --{}", selector.command_path.join(" "), selector.long)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

// ---- Agent subcommand types (moved from agent_skill.rs) ----

use clap::{Subcommand, ValueEnum};
use std::path::PathBuf;

/// Output format for `agent describe`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DescribeFormat {
    /// Structured plain text.
    Text,
    /// Canonical JSON.
    Json,
    /// Markdown skill artifact (`SKILL.md` body) with frontmatter.
    SkillMd,
}

/// Output format for `agent list`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ListFormat {
    /// Structured plain text.
    Text,
    /// Canonical JSON.
    Json,
}

/// Target agent runtime for `agent emit-skills`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum EmitTarget {
    /// `Claude Code` skill directory layout.
    Claude,
    /// `OpenAI Codex CLI` skill directory layout.
    Codex,
}

/// Installation scope for `agent emit-skills --install`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum EmitScope {
    /// User-wide skill directory (e.g. `~/.claude/skills`).
    User,
    /// Project-local skill directory (e.g. `.claude/skills`).
    Project,
}

/// Ungated agent-skill subcommand surface.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum AgentSubcommand {
    /// List declared agent capabilities for this tool.
    List {
        /// Output format.
        #[arg(long, value_enum, default_value_t = ListFormat::Text)]
        format: ListFormat,
    },
    /// Describe one declared agent capability.
    Describe {
        /// Capability name as declared by the tool.
        name: String,
        /// Output format.
        #[arg(long, value_enum, default_value_t = DescribeFormat::Text)]
        format: DescribeFormat,
    },
    /// Emit one skill artifact per declared capability.
    EmitSkills {
        /// Target agent runtime.
        #[arg(long, value_enum)]
        target: EmitTarget,
        /// Installation scope (used with `--install`).
        #[arg(long, value_enum, default_value_t = EmitScope::User)]
        scope: EmitScope,
        /// Write artifacts under this directory instead of the runtime default.
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,
        /// Install artifacts into the resolved runtime skill directory.
        #[arg(long)]
        install: bool,
    },
    /// Emit one script per declared lifecycle hook, plus its registration fragment.
    EmitHooks {
        /// Target agent runtime.
        #[arg(long, value_enum)]
        target: EmitTarget,
        /// Write hook scripts under this directory.
        #[arg(long, value_name = "DIR")]
        out: PathBuf,
    },
}

#[cfg(test)]
mod tests {
    use clap::{Arg, Args, Command, Parser, Subcommand};

    use super::*;
    use crate::{LicenseType, RepoInfo, ToolSpec, test_support::env_lock};

    const QUERY_COMMAND: CommandSelector = CommandSelector::new(&["query"]);
    const STATUS_COMMAND: CommandSelector = CommandSelector::new(&["status"]);
    const QUERY_LIMIT_FLAG: FlagSelector = FlagSelector::new(&["query"], "limit");
    const QUERY_OFFSET_FLAG: FlagSelector = FlagSelector::new(&["query"], "offset");

    const QUERY_CAPABILITY: AgentCapability = AgentCapability::new(
        "query-posts",
        "Read paginated post records",
        &[QUERY_COMMAND],
        &[QUERY_LIMIT_FLAG, QUERY_OFFSET_FLAG],
    );

    const STATUS_CAPABILITY: AgentCapability = AgentCapability::new(
        "inspect-status",
        "Inspect current status",
        &[STATUS_COMMAND],
        &[],
    );
    const QUERY_MINIMAL_CAPABILITY: AgentCapability =
        AgentCapability::minimal("query-minimal", &[QUERY_COMMAND], &[QUERY_LIMIT_FLAG]);

    const AGENT_SURFACE: AgentSurfaceSpec =
        AgentSurfaceSpec::new(&[QUERY_CAPABILITY, STATUS_CAPABILITY]);
    const MINIMAL_AGENT_SURFACE: AgentSurfaceSpec =
        AgentSurfaceSpec::new(&[QUERY_MINIMAL_CAPABILITY]);

    fn spec() -> ToolSpec {
        ToolSpec::new(
            "tool",
            "Tool",
            "1.2.3",
            LicenseType::MIT,
            RepoInfo::new("owner", "repo"),
            true,
            false,
        )
        .with_agent_surface(&AGENT_SURFACE)
    }

    fn minimal_spec() -> ToolSpec {
        ToolSpec::new(
            "tool",
            "Tool",
            "1.2.3",
            LicenseType::MIT,
            RepoInfo::new("owner", "repo"),
            true,
            false,
        )
        .with_agent_surface(&MINIMAL_AGENT_SURFACE)
    }

    fn detect_from_env() -> AgentModeContext {
        AgentModeContext::from_tokens(
            std::env::var(AGENT_TOKEN_ENV).ok(),
            std::env::var(AGENT_TOKEN_EXPECTED_ENV).ok(),
        )
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

    #[test]
    fn agent_mode_activation_is_inactive_without_presented_token() {
        let ctx = AgentModeContext::from_tokens(None, Some("expected".into()));

        assert!(!ctx.active);
    }

    #[test]
    fn agent_mode_activation_is_inactive_without_expected_token() {
        let ctx = AgentModeContext::from_tokens(Some("presented".into()), None);

        assert!(!ctx.active);
    }

    #[test]
    fn agent_mode_activation_is_inactive_on_exact_string_mismatch() {
        let ctx = AgentModeContext::from_tokens(Some("presented".into()), Some("expected".into()));

        assert!(!ctx.active);
    }

    #[test]
    fn agent_mode_activation_is_active_on_exact_string_match() {
        let ctx =
            AgentModeContext::from_tokens(Some("shared-token".into()), Some("shared-token".into()));

        assert!(ctx.active);
    }

    #[test]
    fn agent_mode_activation_preserves_capability_declarations() {
        let spec = spec();
        let capability = spec
            .agent_surface
            .expect("agent surface present")
            .capabilities
            .first()
            .expect("capability present");

        assert_eq!(capability.name, "query-posts");
        assert_eq!(capability.commands[0].path, ["query"]);
        assert_eq!(capability.flags[0].command_path, ["query"]);
        assert_eq!(capability.flags[0].long, "limit");
        assert_eq!(capability.flags[1].long, "offset");
    }

    #[test]
    fn capability_policy_removes_undeclared_subcommand() {
        let mut command = sample_command();

        apply_agent_surface(&mut command, &spec(), &AgentModeContext { active: true });

        assert!(command.find_subcommand("query").is_some());
        assert!(command.find_subcommand("status").is_some());
        assert!(command.find_subcommand("admin").is_none());
    }

    #[test]
    fn capability_policy_removes_undeclared_flag() {
        let mut command = sample_command();

        apply_agent_surface(&mut command, &spec(), &AgentModeContext { active: true });

        let query = command.find_subcommand("query").expect("query present");
        assert!(
            query
                .get_arguments()
                .any(|arg| arg.get_long() == Some("limit"))
        );
        assert!(
            query
                .get_arguments()
                .any(|arg| arg.get_long() == Some("offset"))
        );
        assert!(
            !query
                .get_arguments()
                .any(|arg| arg.get_long() == Some("secret"))
        );
    }

    #[test]
    fn capability_policy_returns_empty_surface_without_declared_capabilities() {
        let spec = ToolSpec::new(
            "tool",
            "Tool",
            "1.2.3",
            LicenseType::MIT,
            RepoInfo::new("owner", "repo"),
            true,
            false,
        );
        let mut command = sample_command();

        apply_agent_surface(&mut command, &spec, &AgentModeContext { active: true });

        assert!(visible_capabilities(&spec, &AgentModeContext { active: true }).is_empty());
        assert!(command.find_subcommand("query").is_none());
        assert!(command.find_subcommand("status").is_none());
        assert!(command.find_subcommand("admin").is_none());
        assert!(
            command
                .get_arguments()
                .any(|arg| arg.get_long() == Some("agent-help"))
        );
        assert!(
            command
                .get_arguments()
                .any(|arg| arg.get_long() == Some("agent-skill"))
        );
    }

    fn sample_command() -> Command {
        Command::new("tool")
            .arg(Arg::new("agent-help").long("agent-help"))
            .arg(
                Arg::new("agent-skill")
                    .long("agent-skill")
                    .value_name("NAME"),
            )
            .subcommand(
                Command::new("query")
                    .arg(Arg::new("limit").long("limit"))
                    .arg(Arg::new("offset").long("offset"))
                    .arg(Arg::new("secret").long("secret")),
            )
            .subcommand(Command::new("status"))
            .subcommand(Command::new("admin").arg(Arg::new("danger").long("danger")))
    }

    #[derive(Debug, Parser, PartialEq, Eq)]
    #[command(name = "tool")]
    struct AgentTestCli {
        #[command(subcommand)]
        command: Option<AgentTestCommand>,
    }

    #[derive(Debug, Subcommand, PartialEq, Eq)]
    enum AgentTestCommand {
        Query(QueryArgs),
        Status,
        Admin(AdminArgs),
    }

    #[derive(Debug, Args, PartialEq, Eq)]
    struct QueryArgs {
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        offset: Option<u32>,
        #[arg(long)]
        secret: bool,
    }

    #[derive(Debug, Args, PartialEq, Eq)]
    struct AdminArgs {
        #[arg(long)]
        danger: bool,
    }

    #[test]
    fn agent_surface_redaction_rejects_hidden_command_and_flag() {
        let _guard = env_lock();
        set_tokens(Some("shared-token"), Some("shared-token"));
        let ctx = detect_from_env();
        let spec = spec();

        let hidden_command_error =
            parse_with_agent_surface_from::<AgentTestCli, _>(&spec, &ctx, ["tool", "admin"])
                .expect_err("hidden subcommand should be rejected")
                .to_string();
        assert!(hidden_command_error.contains("unrecognized subcommand"));

        let hidden_command_typo_error =
            parse_with_agent_surface_from::<AgentTestCli, _>(&spec, &ctx, ["tool", "admni"])
                .expect_err("hidden subcommand typo should not leak suggestions")
                .to_string();
        assert!(hidden_command_typo_error.contains("unrecognized subcommand"));
        assert!(!hidden_command_typo_error.contains("Did you mean"));

        let hidden_flag_error = parse_with_agent_surface_from::<AgentTestCli, _>(
            &spec,
            &ctx,
            ["tool", "query", "--secre"],
        )
        .expect_err("hidden flag typo should be rejected")
        .to_string();
        assert!(hidden_flag_error.contains("unexpected argument"));
        assert!(!hidden_flag_error.contains("--secret"));
        assert!(!hidden_flag_error.contains("Did you mean"));
    }

    #[test]
    fn agent_surface_redaction_help_omits_hidden_entries() {
        let _guard = env_lock();
        set_tokens(Some("shared-token"), Some("shared-token"));
        let ctx = detect_from_env();
        let spec = spec();

        let long_help =
            parse_with_agent_surface_from::<AgentTestCli, _>(&spec, &ctx, ["tool", "--help"])
                .expect_err("help should short-circuit through clap")
                .to_string();
        assert!(long_help.contains("query"));
        assert!(long_help.contains("status"));
        assert!(!long_help.contains("admin"));
        assert!(!long_help.contains("--secret"));

        let help_subcommand =
            parse_with_agent_surface_from::<AgentTestCli, _>(&spec, &ctx, ["tool", "help"])
                .expect_err("help subcommand should short-circuit through clap")
                .to_string();
        assert!(help_subcommand.contains("query"));
        assert!(help_subcommand.contains("status"));
        assert!(!help_subcommand.contains("admin"));
        assert!(!help_subcommand.contains("--secret"));
    }

    #[test]
    fn agent_surface_redaction_preserves_human_mode_surface() {
        let _guard = env_lock();
        set_tokens(None, None);
        let ctx = detect_from_env();
        let spec = spec();

        let admin = parse_with_agent_surface_from::<AgentTestCli, _>(
            &spec,
            &ctx,
            ["tool", "admin", "--danger"],
        )
        .expect("human mode should keep the full command tree");
        assert_eq!(
            admin,
            AgentDispatch::Cli(AgentTestCli {
                command: Some(AgentTestCommand::Admin(AdminArgs { danger: true })),
            })
        );

        let query = parse_with_agent_surface_from::<AgentTestCli, _>(
            &spec,
            &ctx,
            ["tool", "query", "--secret"],
        )
        .expect("human mode should keep hidden flags available");
        assert_eq!(
            query,
            AgentDispatch::Cli(AgentTestCli {
                command: Some(AgentTestCommand::Query(QueryArgs {
                    limit: None,
                    offset: None,
                    secret: true,
                })),
            })
        );
    }

    #[test]
    fn agent_surface_redaction_agent_flags_short_circuit() {
        let _guard = env_lock();
        set_tokens(Some("shared-token"), Some("shared-token"));
        let ctx = detect_from_env();
        let spec = spec();

        let help =
            parse_with_agent_surface_from::<AgentTestCli, _>(&spec, &ctx, ["tool", "--agent-help"])
                .expect("agent help should print and exit");
        assert_eq!(help, AgentDispatch::Printed(0));

        let skill = parse_with_agent_surface_from::<AgentTestCli, _>(
            &spec,
            &ctx,
            ["tool", "--agent-skill", "query-posts"],
        )
        .expect("agent skill should print and exit");
        assert_eq!(skill, AgentDispatch::Printed(0));
    }

    #[test]
    fn agent_help_render_sections_are_structured_and_redacted() {
        let rendered = render_agent_help(&spec(), &AgentModeContext { active: true });

        let section_positions = [
            rendered.find("tool:\n").expect("tool section"),
            rendered.find("mode:\n").expect("mode section"),
            rendered
                .find("capabilities:\n")
                .expect("capabilities section"),
            rendered.find("arguments:\n").expect("arguments section"),
            rendered.find("output:\n").expect("output section"),
            rendered
                .find("constraints:\n")
                .expect("constraints section"),
        ];
        assert!(section_positions.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(rendered.contains("query-posts"));
        assert!(rendered.contains("inspect-status"));
        assert!(!rendered.contains("admin"));
        assert!(!rendered.contains("--secret"));
    }

    #[test]
    fn agent_help_render_skill_output_is_single_capability_only() {
        let rendered =
            render_agent_skill(&spec(), &AgentModeContext { active: true }, "query-posts")
                .expect("capability should render");

        let section_positions = [
            rendered.find("tool:\n").expect("tool section"),
            rendered.find("capability:\n").expect("capability section"),
            rendered.find("summary:\n").expect("summary section"),
            rendered.find("commands:\n").expect("commands section"),
            rendered.find("flags:\n").expect("flags section"),
            rendered.find("examples:\n").expect("examples section"),
            rendered.find("output:\n").expect("output section"),
            rendered
                .find("constraints:\n")
                .expect("constraints section"),
        ];
        assert!(section_positions.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(rendered.contains("query-posts"));
        assert!(!rendered.contains("inspect-status"));
        assert!(rendered.contains("query"));
        assert!(rendered.contains("--limit"));
        assert!(rendered.contains("--offset"));
    }

    #[test]
    fn agent_help_render_unknown_skill_is_bounded() {
        let error = render_agent_skill(&spec(), &AgentModeContext { active: true }, "missing")
            .expect_err("unknown capability should fail");

        assert_eq!(error.to_string(), "unknown agent capability: missing");
        assert!(!error.to_string().contains("query-posts"));
        assert!(!error.to_string().contains("inspect-status"));
    }

    #[test]
    fn agent_help_render_fills_missing_prose_metadata() {
        let rendered = render_agent_skill(
            &minimal_spec(),
            &AgentModeContext { active: true },
            "query-minimal",
        )
        .expect("minimal capability should render");

        assert!(rendered.contains("capability:\n- query-minimal"));
        assert!(rendered.contains("summary:\n-"));
        assert!(rendered.contains("commands:\n- query"));
        assert!(rendered.contains("flags:\n- query --limit"));
        assert!(rendered.contains("examples:\n- none declared"));
        assert!(rendered.contains("output:\n-"));
        assert!(rendered.contains("constraints:\n-"));
    }

    // ---- Additional surfaces: builders, accessors, filtering, rendering ----

    const SUBTREE_CAPABILITY: AgentCapability =
        AgentCapability::minimal("query-subtree", &[QUERY_COMMAND], &[]);
    const SUBTREE_SURFACE: AgentSurfaceSpec = AgentSurfaceSpec::new(&[SUBTREE_CAPABILITY]);

    const EMPTY_CAPABILITY: AgentCapability = AgentCapability::minimal("empty-cap", &[], &[]);
    const EMPTY_SURFACE: AgentSurfaceSpec = AgentSurfaceSpec::new(&[EMPTY_CAPABILITY]);

    const NONEMPTY_EXAMPLES: &[&str] = &["tool query --limit 5", "tool query --offset 2"];
    const EXAMPLE_CAPABILITY: AgentCapability = AgentCapability::new(
        "with-examples",
        "Summary",
        &[QUERY_COMMAND],
        &[QUERY_LIMIT_FLAG],
    )
    .with_examples(NONEMPTY_EXAMPLES);
    const EXAMPLE_SURFACE: AgentSurfaceSpec = AgentSurfaceSpec::new(&[EXAMPLE_CAPABILITY]);

    const EMPTY_EXAMPLES: &[&str] = &[];
    const EMPTY_EXAMPLES_CAPABILITY: AgentCapability =
        AgentCapability::new("empty-examples", "Summary", &[QUERY_COMMAND], &[])
            .with_examples(EMPTY_EXAMPLES);
    const EMPTY_EXAMPLES_SURFACE: AgentSurfaceSpec =
        AgentSurfaceSpec::new(&[EMPTY_EXAMPLES_CAPABILITY]);

    const ROOT_FLAG: FlagSelector = FlagSelector::new(&[], "verbose");
    const ROOT_FLAG_CAPABILITY: AgentCapability =
        AgentCapability::new("root-flag", "Summary", &[], &[ROOT_FLAG]);
    const ROOT_FLAG_SURFACE: AgentSurfaceSpec = AgentSurfaceSpec::new(&[ROOT_FLAG_CAPABILITY]);

    fn spec_with_surface(surface: &'static AgentSurfaceSpec) -> ToolSpec {
        ToolSpec::new(
            "tool",
            "Tool",
            "1.2.3",
            LicenseType::MIT,
            RepoInfo::new("owner", "repo"),
            true,
            false,
        )
        .with_agent_surface(surface)
    }

    fn nested_command() -> Command {
        Command::new("tool").subcommand(
            Command::new("query")
                .arg(Arg::new("term"))
                .subcommand(Command::new("recent").arg(Arg::new("limit").long("limit")))
                .subcommand(Command::new("all")),
        )
    }

    #[derive(Debug, Parser)]
    #[command(name = "agent-tool")]
    struct AgentSubcommandCli {
        #[command(subcommand)]
        command: AgentSubcommand,
    }

    #[test]
    fn agent_surface_spec_exposes_capabilities_at_runtime() {
        const CAPS: &[AgentCapability] = &[QUERY_CAPABILITY, STATUS_CAPABILITY];

        let surface = AgentSurfaceSpec::new(CAPS);
        let capabilities = surface.capabilities();

        assert_eq!(capabilities.len(), 2);
        assert_eq!(capabilities[0].name(), "query-posts");
        assert_eq!(capabilities[1].name(), "inspect-status");
    }

    #[test]
    fn agent_capability_builder_sets_all_optional_fields() {
        const CMDS: &[CommandSelector] = &[QUERY_COMMAND];
        const FLAGS: &[FlagSelector] = &[QUERY_LIMIT_FLAG];
        const EXAMPLES: &[&str] = &["tool query --limit 5"];

        let capability = AgentCapability::new("query-posts", "Read posts", CMDS, FLAGS)
            .with_examples(EXAMPLES)
            .with_output("json lines")
            .with_constraints("auth required")
            .with_when_to_use("when reading")
            .with_when_not_to_use("when writing");

        assert_eq!(capability.name(), "query-posts");
        assert_eq!(capability.summary(), Some("Read posts"));
        assert_eq!(capability.commands().len(), 1);
        assert_eq!(capability.commands()[0].path(), ["query"]);
        assert_eq!(capability.flags().len(), 1);
        assert_eq!(capability.flags()[0].long(), "limit");
        assert_eq!(capability.examples(), Some(EXAMPLES));
        assert_eq!(capability.output(), Some("json lines"));
        assert_eq!(capability.constraints(), Some("auth required"));
        assert_eq!(capability.when_to_use(), Some("when reading"));
        assert_eq!(capability.when_not_to_use(), Some("when writing"));
    }

    #[test]
    fn agent_capability_minimal_leaves_optional_fields_none() {
        const CMDS: &[CommandSelector] = &[QUERY_COMMAND];
        const FLAGS: &[FlagSelector] = &[QUERY_LIMIT_FLAG];

        let capability = AgentCapability::minimal("query-minimal", CMDS, FLAGS);

        assert_eq!(capability.name(), "query-minimal");
        assert_eq!(capability.summary(), None);
        assert_eq!(capability.examples(), None);
        assert_eq!(capability.output(), None);
        assert_eq!(capability.constraints(), None);
        assert_eq!(capability.when_to_use(), None);
        assert_eq!(capability.when_not_to_use(), None);
        assert_eq!(capability.commands().len(), 1);
        assert_eq!(capability.flags().len(), 1);
    }

    #[test]
    fn command_and_flag_selectors_expose_paths_at_runtime() {
        const PATH: &[&str] = &["query", "recent"];

        let command = CommandSelector::new(PATH);
        assert_eq!(command.path(), ["query", "recent"]);

        let flag = FlagSelector::new(PATH, "limit");
        assert_eq!(flag.command_path(), ["query", "recent"]);
        assert_eq!(flag.long(), "limit");
    }

    #[test]
    fn visible_capabilities_empty_when_inactive() {
        let spec = spec();

        let capabilities = visible_capabilities(&spec, &AgentModeContext { active: false });

        assert!(capabilities.is_empty());
    }

    #[test]
    fn apply_agent_surface_inactive_leaves_command_unchanged() {
        let mut command = sample_command();

        apply_agent_surface(&mut command, &spec(), &AgentModeContext { active: false });

        assert!(command.find_subcommand("admin").is_some());
        let query = command.find_subcommand("query").expect("query present");
        assert!(
            query
                .get_arguments()
                .any(|arg| arg.get_long() == Some("secret"))
        );
    }

    #[test]
    fn filter_command_keeps_full_subtree_for_declared_parent_command() {
        let spec = spec_with_surface(&SUBTREE_SURFACE);
        let mut command = nested_command();

        apply_agent_surface(&mut command, &spec, &AgentModeContext { active: true });

        let query = command.find_subcommand("query").expect("query present");
        assert!(query.find_subcommand("recent").is_some());
        assert!(query.find_subcommand("all").is_some());
        assert!(query.get_arguments().any(Arg::is_positional));

        let recent = query.find_subcommand("recent").expect("recent present");
        assert!(
            recent
                .get_arguments()
                .any(|arg| arg.get_long() == Some("limit"))
        );
    }

    #[test]
    fn clone_command_metadata_preserves_all_optional_fields() {
        let mut command = Command::new("tool")
            .display_name("Tool Display")
            .about("about text")
            .long_about("long about text")
            .before_help("before help text")
            .after_help("after help text")
            .disable_help_flag(true)
            .disable_help_subcommand(true)
            .disable_colored_help(true)
            .flatten_help(true)
            .subcommand(Command::new("query").arg(Arg::new("limit").long("limit")));
        command.set_bin_name("toolbin");

        apply_agent_surface(&mut command, &spec(), &AgentModeContext { active: true });

        assert_eq!(command.get_display_name(), Some("Tool Display"));
        assert_eq!(
            command.get_about().map(ToString::to_string),
            Some(String::from("about text"))
        );
        assert_eq!(
            command.get_long_about().map(ToString::to_string),
            Some(String::from("long about text"))
        );
        assert_eq!(
            command.get_before_help().map(ToString::to_string),
            Some(String::from("before help text"))
        );
        assert_eq!(
            command.get_after_help().map(ToString::to_string),
            Some(String::from("after help text"))
        );
        assert!(command.is_disable_help_flag_set());
        assert!(command.is_disable_help_subcommand_set());
        assert!(command.is_disable_colored_help_set());
        assert!(command.is_flatten_help_set());
        assert_eq!(command.get_bin_name(), Some("toolbin"));
    }

    #[test]
    fn sanitize_agent_parse_error_strips_suggestions() {
        let error = clap::Error::raw(
            clap::error::ErrorKind::UnknownArgument,
            "unexpected argument '--secre'\n\tDid you mean '--secret'?\n",
        );

        let sanitized = sanitize_agent_parse_error(error).to_string();

        assert!(sanitized.contains("unexpected argument"));
        assert!(!sanitized.contains("Did you mean"));
    }

    #[test]
    fn strip_suggestion_lines_removes_did_you_mean_lines() {
        let rendered = "error: unexpected argument\n\tDid you mean '--limit'?\ntip: keep this line";

        let stripped = strip_suggestion_lines(rendered);

        assert!(stripped.contains("error: unexpected argument"));
        assert!(stripped.contains("tip: keep this line"));
        assert!(!stripped.contains("Did you mean"));
    }

    #[test]
    fn render_agent_help_lists_none_when_no_capabilities() {
        let spec = ToolSpec::new(
            "tool",
            "Tool",
            "1.2.3",
            LicenseType::MIT,
            RepoInfo::new("owner", "repo"),
            true,
            false,
        );

        let rendered = render_agent_help(&spec, &AgentModeContext { active: true });

        assert!(rendered.contains("capabilities:\n- none"));
        assert!(rendered.contains("mode:\n- agent"));
    }

    #[test]
    fn render_agent_help_renders_root_level_flag_without_command_prefix() {
        let spec = spec_with_surface(&ROOT_FLAG_SURFACE);

        let rendered = render_agent_help(&spec, &AgentModeContext { active: true });

        assert!(rendered.contains("- --verbose"));
        assert!(!rendered.contains("query --verbose"));
    }

    #[test]
    fn render_agent_skill_summary_falls_back_to_command_path() {
        let rendered = render_agent_skill(
            &minimal_spec(),
            &AgentModeContext { active: true },
            "query-minimal",
        )
        .expect("minimal capability should render");

        assert!(rendered.contains("summary:\n- Use query minimal via query"));
    }

    #[test]
    fn render_agent_skill_fallbacks_for_capability_without_commands() {
        let spec = spec_with_surface(&EMPTY_SURFACE);

        let rendered = render_agent_skill(&spec, &AgentModeContext { active: true }, "empty-cap")
            .expect("empty capability should render");

        assert!(rendered.contains("summary:\n- Use empty cap"));
        assert!(rendered.contains("commands:\n- none declared"));
        assert!(rendered.contains("flags:\n- none declared"));
        assert!(rendered.contains("examples:\n- none declared"));
        assert!(rendered.contains("output:\n- output follows the existing CLI contract"));
        assert!(
            rendered
                .contains("constraints:\n- existing command validation and auth rules still apply")
        );
    }

    #[test]
    fn render_agent_skill_lists_declared_examples() {
        let spec = spec_with_surface(&EXAMPLE_SURFACE);

        let rendered =
            render_agent_skill(&spec, &AgentModeContext { active: true }, "with-examples")
                .expect("capability should render");

        assert!(rendered.contains("examples:\n- tool query --limit 5\n- tool query --offset 2"));
    }

    #[test]
    fn render_agent_skill_reports_none_for_empty_examples() {
        let spec = spec_with_surface(&EMPTY_EXAMPLES_SURFACE);

        let rendered =
            render_agent_skill(&spec, &AgentModeContext { active: true }, "empty-examples")
                .expect("capability should render");

        assert!(rendered.contains("examples:\n- none declared"));
    }

    #[test]
    fn render_agent_skill_renders_root_level_flag() {
        let spec = spec_with_surface(&ROOT_FLAG_SURFACE);

        let rendered = render_agent_skill(&spec, &AgentModeContext { active: true }, "root-flag")
            .expect("capability should render");

        assert!(rendered.contains("flags:\n- --verbose"));
        assert!(rendered.contains("commands:\n- none declared"));
    }

    #[test]
    fn agent_subcommand_parses_list_with_default_format() {
        let cli =
            AgentSubcommandCli::try_parse_from(["agent-tool", "list"]).expect("list should parse");

        assert_eq!(
            cli.command,
            AgentSubcommand::List {
                format: ListFormat::Text
            }
        );
    }

    #[test]
    fn agent_subcommand_parses_describe_with_json_format() {
        let cli = AgentSubcommandCli::try_parse_from([
            "agent-tool",
            "describe",
            "query-posts",
            "--format",
            "json",
        ])
        .expect("describe should parse");

        assert_eq!(
            cli.command,
            AgentSubcommand::Describe {
                name: String::from("query-posts"),
                format: DescribeFormat::Json,
            }
        );
    }

    #[test]
    fn agent_subcommand_parses_describe_skill_md_and_emit_defaults() {
        let describe = AgentSubcommandCli::try_parse_from([
            "agent-tool",
            "describe",
            "cap",
            "--format",
            "skill-md",
        ])
        .expect("describe should parse");
        assert_eq!(
            describe.command,
            AgentSubcommand::Describe {
                name: String::from("cap"),
                format: DescribeFormat::SkillMd,
            }
        );

        let emit =
            AgentSubcommandCli::try_parse_from(["agent-tool", "emit-skills", "--target", "claude"])
                .expect("emit-skills should parse");
        assert_eq!(
            emit.command,
            AgentSubcommand::EmitSkills {
                target: EmitTarget::Claude,
                scope: EmitScope::User,
                out: None,
                install: false,
            }
        );
    }

    #[test]
    fn agent_subcommand_parses_emit_skills_variants() {
        let cli = AgentSubcommandCli::try_parse_from([
            "agent-tool",
            "emit-skills",
            "--target",
            "codex",
            "--scope",
            "project",
            "--out",
            "/tmp/skills",
            "--install",
        ])
        .expect("emit-skills should parse");

        assert_eq!(
            cli.command,
            AgentSubcommand::EmitSkills {
                target: EmitTarget::Codex,
                scope: EmitScope::Project,
                out: Some(std::path::PathBuf::from("/tmp/skills")),
                install: true,
            }
        );
    }

    #[test]
    fn agent_subcommand_parses_emit_hooks() {
        let cli = AgentSubcommandCli::try_parse_from([
            "agent-tool",
            "emit-hooks",
            "--target",
            "claude",
            "--out",
            "/tmp/hooks",
        ])
        .expect("emit-hooks should parse");

        assert_eq!(
            cli.command,
            AgentSubcommand::EmitHooks {
                target: EmitTarget::Claude,
                out: std::path::PathBuf::from("/tmp/hooks"),
            }
        );
    }

    #[test]
    fn agent_subcommand_emit_hooks_requires_out() {
        let result =
            AgentSubcommandCli::try_parse_from(["agent-tool", "emit-hooks", "--target", "claude"]);

        assert!(result.is_err(), "--out should be required for emit-hooks");
    }

    #[test]
    fn agent_hook_builder_sets_status_message() {
        const HOOK: AgentHook = AgentHook::new(
            "session-end-kb",
            HookEvent::SessionEnd,
            "#!/usr/bin/env bash\n",
            30,
        )
        .with_status_message("Distilling session into kb");

        assert_eq!(HOOK.name(), "session-end-kb");
        assert_eq!(HOOK.event(), HookEvent::SessionEnd);
        assert_eq!(HOOK.script(), "#!/usr/bin/env bash\n");
        assert_eq!(HOOK.timeout(), 30);
        assert_eq!(HOOK.status_message(), Some("Distilling session into kb"));
    }

    #[test]
    fn agent_hook_new_leaves_status_message_none() {
        const HOOK: AgentHook = AgentHook::new(
            "plan-validate",
            HookEvent::Stop,
            "#!/usr/bin/env bash\n",
            60,
        );

        assert_eq!(HOOK.status_message(), None);
    }

    #[test]
    fn agent_surface_spec_exposes_hooks_at_runtime() {
        const HOOK: AgentHook = AgentHook::new(
            "plan-validate",
            HookEvent::Stop,
            "#!/usr/bin/env bash\n",
            60,
        );
        const SURFACE: AgentSurfaceSpec = AgentSurfaceSpec::new(&[]).with_hooks(&[HOOK]);

        assert_eq!(SURFACE.hooks().len(), 1);
        assert_eq!(SURFACE.hooks()[0].name(), "plan-validate");
        assert!(AgentSurfaceSpec::new(&[]).hooks().is_empty());
    }
}
