//! Canonical metadata command surface shared across workspace binaries.

use clap::Subcommand;
use clap_complete::Shell;

use crate::{JsonOutput, StandardCommand, StandardCommandMap, agent::AgentSubcommand};

/// Canonical metadata commands exposed by the shared CLI contract.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum MetaCommand {
    /// Show version information.
    Version {
        /// Emit JSON instead of plain text.
        #[arg(long)]
        json: bool,
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
        /// Emit JSON instead of plain text.
        #[arg(long)]
        json: bool,
    },
    /// Inspect or emit agent-skill artifacts for this tool.
    Agent {
        /// Agent-skill subcommand.
        #[command(subcommand)]
        command: AgentSubcommand,
    },
}

impl StandardCommandMap for MetaCommand {
    fn to_standard_command(&self, _output: JsonOutput) -> StandardCommand {
        match self {
            Self::Version { json } => StandardCommand::Version {
                output: JsonOutput::from_flag(*json),
            },
            Self::License => StandardCommand::License,
            Self::Completions { shell } => StandardCommand::Completions { shell: *shell },
            Self::Doctor { json } => StandardCommand::Doctor {
                output: JsonOutput::from_flag(*json),
            },
            Self::Agent { command } => StandardCommand::Agent {
                command: command.clone(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::{Parser, Subcommand};

    use super::*;
    use crate::{JsonOutput, StandardCommand, map_standard_command};

    #[derive(Debug, Parser)]
    #[command(name = "tool")]
    struct Cli {
        #[command(subcommand)]
        command: Command,
    }

    #[derive(Debug, Subcommand)]
    enum Command {
        /// Canonical metadata commands.
        Meta {
            #[command(subcommand)]
            command: MetaCommand,
        },
    }

    fn parse_meta(argv: &[&str]) -> MetaCommand {
        match Cli::parse_from(argv).command {
            Command::Meta { command } => command,
        }
    }

    #[test]
    fn parses_meta_version_json() {
        let command = parse_meta(&["tool", "meta", "version", "--json"]);
        assert_eq!(command, MetaCommand::Version { json: true });
        assert_eq!(
            map_standard_command(&command, JsonOutput::Text),
            StandardCommand::Version {
                output: JsonOutput::Json
            }
        );
    }

    #[test]
    fn parses_meta_license() {
        let command = parse_meta(&["tool", "meta", "license"]);
        assert_eq!(command, MetaCommand::License);
        assert_eq!(
            map_standard_command(&command, JsonOutput::Text),
            StandardCommand::License
        );
    }

    #[test]
    fn parses_meta_completions() {
        let command = parse_meta(&["tool", "meta", "completions", "bash"]);
        assert_eq!(command, MetaCommand::Completions { shell: Shell::Bash });
        assert_eq!(
            map_standard_command(&command, JsonOutput::Text),
            StandardCommand::Completions { shell: Shell::Bash }
        );
    }

    #[test]
    fn parses_meta_doctor_json() {
        let command = parse_meta(&["tool", "meta", "doctor", "--json"]);
        assert_eq!(command, MetaCommand::Doctor { json: true });
        assert_eq!(
            map_standard_command(&command, JsonOutput::Text),
            StandardCommand::Doctor {
                output: JsonOutput::Json
            }
        );
    }

    #[test]
    fn parses_meta_agent_and_maps_to_standard_command() {
        use crate::ListFormat;

        let command = parse_meta(&["tool", "meta", "agent", "list"]);
        assert_eq!(
            command,
            MetaCommand::Agent {
                command: AgentSubcommand::List {
                    format: ListFormat::Text,
                },
            }
        );
        assert_eq!(
            map_standard_command(&command, JsonOutput::Text),
            StandardCommand::Agent {
                command: AgentSubcommand::List {
                    format: ListFormat::Text,
                },
            }
        );
    }

    #[test]
    fn rejects_meta_update() {
        let result = Cli::try_parse_from(["tool", "meta", "update"]);
        assert!(result.is_err(), "meta update must no longer parse");
    }
}
