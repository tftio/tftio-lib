//! Shell completion generation module.
//!
//! This module provides generic shell completion generation for CLI tools using clap.
//! It works with any clap `CommandFactory` and generates completions for all major shells.

use clap::CommandFactory;
use clap_complete::Shell;
use std::io::{self, Write};

/// Completion content rendered in-memory before it is written anywhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionOutput {
    /// Installation instructions for the selected shell.
    pub instructions: String,
    /// Completion script emitted by `clap_complete`.
    pub script: String,
}

/// Render completion installation instructions for a clap-based CLI.
#[must_use]
pub fn render_completion_instructions(shell: Shell, bin_name: &str) -> String {
    match shell {
        Shell::Bash => format!(
            "# Shell completion for {bin_name}\n#\n# To enable completions, add this to your shell config:\n#\n#   source <({bin_name} completions bash)\n\n"
        ),
        Shell::Zsh => format!(
            "# Shell completion for {bin_name}\n#\n# To enable completions, add this to your shell config:\n#\n#   {bin_name} completions zsh > ~/.zsh/completions/_{bin_name}\n#   # Ensure fpath includes ~/.zsh/completions\n\n"
        ),
        Shell::Fish => format!(
            "# Shell completion for {bin_name}\n#\n# To enable completions, add this to your shell config:\n#\n#   {bin_name} completions fish | source\n\n"
        ),
        Shell::PowerShell => format!(
            "# Shell completion for {bin_name}\n#\n# To enable completions, add this to your shell config:\n#\n#   {bin_name} completions powershell | Out-String | Invoke-Expression\n\n"
        ),
        Shell::Elvish => format!(
            "# Shell completion for {bin_name}\n#\n# To enable completions, add this to your shell config:\n#\n#   {bin_name} completions elvish | eval\n\n"
        ),
        other => format!(
            "# Shell completion for {bin_name}\n#\n# To enable completions, add this to your shell config:\n#\n#   {bin_name} completions {other}\n\n"
        ),
    }
}

/// Render shell completions fully in memory.
#[must_use]
pub fn render_completion<T: CommandFactory>(shell: Shell) -> CompletionOutput {
    render_completion_from_command(shell, T::command())
}

/// Render shell completions from a pre-built clap command tree.
#[must_use]
pub fn render_completion_from_command(
    shell: Shell,
    mut command: clap::Command,
) -> CompletionOutput {
    let bin_name = command.get_name().to_string();
    let mut buffer = Vec::new();

    clap_complete::generate(shell, &mut command, bin_name.clone(), &mut buffer);

    CompletionOutput {
        instructions: render_completion_instructions(shell, &bin_name),
        script: String::from_utf8_lossy(&buffer).into_owned(),
    }
}

/// Write a previously rendered completion output to a writer.
///
/// # Errors
///
/// Returns an error if writing fails.
pub fn write_completion(mut writer: impl Write, output: &CompletionOutput) -> io::Result<()> {
    writer.write_all(output.instructions.as_bytes())?;
    writer.write_all(output.script.as_bytes())
}

/// Generate shell completion scripts for a clap-based CLI.
///
/// This function generates shell completions and prints both installation instructions
/// and the completion script to stdout. It supports bash, zsh, fish, elvish, and `PowerShell`.
///
/// # Type Parameters
/// * `T` - A type that implements `CommandFactory` (typically your clap `Cli` struct)
///
/// # Arguments
/// * `shell` - The shell type to generate completions for
///
/// # Examples
/// ```no_run
/// use clap::Parser;
/// use tftio_lib::completions::generate_completions;
///
/// #[derive(Parser)]
/// struct Cli {
///     // your CLI definition
/// }
///
/// let _ = generate_completions::<Cli>(clap_complete::Shell::Bash);
/// ```
///
/// # Errors
///
/// Returns an I/O error when writing instructions or the generated completion
/// script to standard output fails.
pub fn generate_completions<T: CommandFactory>(shell: Shell) -> io::Result<()> {
    generate_completions_from_command(shell, T::command())
}

/// Generate shell completion scripts from a pre-built clap command tree.
///
/// # Errors
///
/// Returns an I/O error when writing instructions or the generated completion
/// script to standard output fails.
pub fn generate_completions_from_command(shell: Shell, command: clap::Command) -> io::Result<()> {
    let output = render_completion_from_command(shell, command);
    write_completion(io::stdout(), &output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::apply_agent_surface;
    use crate::{
        AgentCapability, AgentModeContext, AgentSurfaceSpec, CommandSelector, FlagSelector,
        LicenseType, RepoInfo, ToolSpec,
    };
    use clap::{Arg, Parser, Subcommand};

    #[derive(Parser)]
    #[command(name = "test-cli")]
    struct TestCli {
        #[command(subcommand)]
        command: TestCommands,
    }

    #[derive(Subcommand)]
    enum TestCommands {
        Version,
        Test { arg: String },
    }

    const QUERY_COMMAND: CommandSelector = CommandSelector::new(&["query"]);
    const QUERY_LIMIT_FLAG: FlagSelector = FlagSelector::new(&["query"], "limit");
    const QUERY_CAPABILITY: AgentCapability = AgentCapability::new(
        "query-posts",
        "Read paginated post records",
        &[QUERY_COMMAND],
        &[QUERY_LIMIT_FLAG],
    );
    const AGENT_SURFACE: AgentSurfaceSpec = AgentSurfaceSpec::new(&[QUERY_CAPABILITY]);

    fn agent_spec() -> ToolSpec {
        ToolSpec::new(
            "test-cli",
            "Test CLI",
            "1.2.3",
            LicenseType::MIT,
            RepoInfo::new("owner", "repo"),
            true,
            false,
        )
        .with_agent_surface(&AGENT_SURFACE)
    }

    #[test]
    fn test_generate_completions_bash() {
        // Just verify it doesn't panic
        let _ = generate_completions::<TestCli>(Shell::Bash);
    }

    #[test]
    fn test_generate_completions_zsh() {
        let _ = generate_completions::<TestCli>(Shell::Zsh);
    }

    #[test]
    fn test_generate_completions_fish() {
        let _ = generate_completions::<TestCli>(Shell::Fish);
    }

    #[test]
    fn test_generate_completions_elvish() {
        let _ = generate_completions::<TestCli>(Shell::Elvish);
    }

    #[test]
    fn test_generate_completions_powershell() {
        let _ = generate_completions::<TestCli>(Shell::PowerShell);
    }

    #[test]
    fn test_all_shells_generate_without_panic() {
        let shells = vec![
            Shell::Bash,
            Shell::Zsh,
            Shell::Fish,
            Shell::Elvish,
            Shell::PowerShell,
        ];

        for shell in shells {
            let _ = generate_completions::<TestCli>(shell);
        }
    }

    #[test]
    fn render_completion_separates_instructions_from_script() {
        let output = render_completion::<TestCli>(Shell::Bash);

        assert!(
            output
                .instructions
                .contains("source <(test-cli completions bash)")
        );
        assert!(output.script.contains("complete"));
    }

    #[test]
    fn agent_surface_redaction_completion_helper_omits_hidden_entries() {
        let mut command = clap::Command::new("test-cli")
            .subcommand(
                clap::Command::new("query")
                    .arg(Arg::new("limit").long("limit"))
                    .arg(Arg::new("secret").long("secret")),
            )
            .subcommand(clap::Command::new("admin"));

        apply_agent_surface(
            &mut command,
            &agent_spec(),
            &AgentModeContext { active: true },
        );

        let output = render_completion_from_command(Shell::Bash, command);

        assert!(output.script.contains("query"));
        assert!(!output.script.contains("admin"));
        assert!(!output.script.contains("--secret"));
    }
}
