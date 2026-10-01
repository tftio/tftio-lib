#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)
)]
// Unit-test modules in this crate set/remove the agent-token and HOME env vars
// for sanctioned test setup; the env ban applies only to non-test library code
// (REPO_INVARIANTS.md ENG-008).
#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unwrap_used,
        reason = "test fixtures use fail-fast assertions and sanctioned env setup"
    )
)]
//! Common CLI and agent-mode plumbing for tftio tools.
//!
//! This library provides shared functionality for CLI tools including:
//! - Shell completion generation
//! - Health check framework
//! - License display
//! - Terminal output utilities
//!
//! # Example Usage
//!
//! ```no_run
//! use tftio_lib::{
//!     RepoInfo, DoctorChecks, DoctorCheck,
//!     completions, doctor, license,
//! };
//! use clap::Parser;
//!
//! #[derive(Parser)]
//! struct Cli {
//!     // your CLI definition
//! }
//!
//! struct MyTool;
//!
//! impl DoctorChecks for MyTool {
//!     fn repo_info() -> RepoInfo {
//!         RepoInfo::new("myorg", "mytool")
//!     }
//!
//!     fn current_version() -> &'static str {
//!         env!("CARGO_PKG_VERSION")
//!     }
//!
//!     fn tool_checks(&self) -> Vec<DoctorCheck> {
//!         vec![
//!             DoctorCheck::file_exists("~/.config/mytool/config.toml"),
//!         ]
//!     }
//! }
//!
//! // Generate completions
//! completions::generate_completions::<Cli>(clap_complete::Shell::Bash);
//!
//! // Run health check
//! let tool = MyTool;
//! let exit_code = doctor::run_doctor(&tool);
//! ```

// Re-export main types and traits
pub use doctor::DoctorChecks;
pub use license::LicenseType;
pub use types::{DoctorCheck, RepoInfo};

// Public modules
pub mod agent;
pub mod agent_hook;
pub mod agent_skill;
pub mod app;
pub mod binary;
pub mod command;
pub mod completions;
pub mod credential;
pub mod doctor;
pub mod error;
pub mod json;
pub mod license;
pub mod meta;
pub mod output;
pub mod progress;
pub mod project;
pub mod runner;
pub mod types;

// Re-export commonly used items
pub use agent::{
    AGENT_TOKEN_ENV, AGENT_TOKEN_EXPECTED_ENV, AgentCapability, AgentDispatch, AgentHook,
    AgentModeContext, AgentSkillError, AgentSurfaceSpec, CommandSelector, FlagSelector, HookEvent,
    ProcessEnv, apply_agent_surface, parse_with_agent_surface_from, render_agent_help,
    render_agent_skill,
};
pub use agent::{AgentSubcommand, DescribeFormat, EmitScope, EmitTarget, ListFormat};
pub use agent_hook::{event_key as hook_event_key, hook_file_name, render_hooks_fragment};
pub use agent_skill::{
    EmitDirError, SKILL_DESCRIPTION_BUDGET, render_describe, render_list, render_skill_md,
    resolve_emit_dir, run_agent_subcommand, skill_name,
};
pub use app::{ContractError, ToolContract, ToolSpec, workspace_tool};
pub use binary::{run_cli_from, run_cli_no_doctor_from};
pub use command::{
    NoDoctor, StandardCommand, StandardCommandMap, map_standard_command,
    maybe_run_standard_command, maybe_run_standard_command_no_doctor,
    parse_command_ref_with_agent_surface_from, parse_command_with_agent_surface_from,
    run_standard_command_no_doctor,
};
pub use completions::{
    CompletionOutput, generate_completions, generate_completions_from_command, render_completion,
    render_completion_from_command, render_completion_instructions, write_completion,
};
pub use credential::{
    CredentialError, CredentialInvocation, CredentialSource, OnePasswordRef,
    resolve as resolve_credential,
};
pub use doctor::{
    DoctorReport, print_doctor_report_json, print_doctor_report_text, run_doctor,
    run_doctor_with_output,
};
pub use error::{fatal_error, print_error};
pub use json::{
    JsonOutput, err_response, ok_response, render_response, render_response_parts,
    render_response_with,
};
pub use license::display_license;
pub use meta::MetaCommand;
pub use progress::make_spinner;
pub use project::{
    NormalizedRemote, ProjectRoot, Registry, RegistryProject, Resolution, ResolutionInputs, Slug,
    Source, discover_inputs, load_registry, normalize_remote, project_root, read_declared_project,
    resolve,
};
pub use runner::{
    FatalCliError, parse_and_exit, parse_and_run, run_with_display_error_handler,
    run_with_fatal_handler,
};

#[cfg(test)]
/// Shared test utilities for workspace integration tests.
pub mod test_support {
    use std::sync::{Mutex, MutexGuard, OnceLock};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    /// Acquire a process-wide lock for tests that mutate environment variables.
    pub fn env_lock() -> MutexGuard<'static, ()> {
        ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repo_info_creation() {
        let repo = RepoInfo::new("example", "test");
        assert_eq!(repo.owner, "example");
        assert_eq!(repo.name, "test");
    }

    #[test]
    fn test_doctor_check_creation() {
        let check = DoctorCheck::pass("test");
        assert!(check.passed);

        let check = DoctorCheck::fail("test", "failed");
        assert!(!check.passed);
    }

    #[test]
    fn test_license_type() {
        assert_eq!(LicenseType::MIT.name(), "MIT");
        assert_eq!(LicenseType::Apache2.name(), "Apache-2.0");
        assert_eq!(LicenseType::CC0.name(), "CC0-1.0");
    }
}
