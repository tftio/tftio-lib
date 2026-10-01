//! Shared CLI application metadata.

use crate::{AgentSurfaceSpec, LicenseType, RepoInfo};

/// Default repository metadata for tools that do not declare their own repository.
pub const WORKSPACE_REPO: RepoInfo = RepoInfo::new("tftio", "tftio-lib");

/// Errors raised when validating a tool's adoption of the shared CLI contract.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum ContractError {
    /// The tool still relies on legacy advisory capability flags.
    #[error("tool spec does not opt into the authoritative cli-common base contract")]
    NotAuthoritative,
}

/// Declares whether a tool has opted into the authoritative shared CLI contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolContract {
    /// The tool exposes the full shared CLI contract.
    CliCommonBase,
    /// The tool still uses legacy advisory capability flags.
    Legacy {
        /// Whether the tool supports JSON output in base commands.
        supports_json: bool,
        /// Whether the tool exposes doctor checks.
        supports_doctor: bool,
    },
}

impl ToolContract {
    /// Build a contract marker from legacy capability flags.
    #[must_use]
    pub const fn from_support_flags(supports_json: bool, supports_doctor: bool) -> Self {
        if supports_json && supports_doctor {
            Self::CliCommonBase
        } else {
            Self::Legacy {
                supports_json,
                supports_doctor,
            }
        }
    }

    /// Whether the tool supports JSON output in base commands.
    #[must_use]
    pub const fn supports_json(self) -> bool {
        match self {
            Self::CliCommonBase => true,
            Self::Legacy { supports_json, .. } => supports_json,
        }
    }

    /// Whether the tool exposes doctor checks.
    #[must_use]
    pub const fn supports_doctor(self) -> bool {
        match self {
            Self::CliCommonBase => true,
            Self::Legacy {
                supports_doctor, ..
            } => supports_doctor,
        }
    }

    /// Whether the tool has opted into the authoritative shared CLI contract.
    #[must_use]
    pub const fn is_authoritative(self) -> bool {
        matches!(self, Self::CliCommonBase)
    }

    /// Validate that the tool has opted into the authoritative shared CLI contract.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError::NotAuthoritative`] when the tool still relies on
    /// legacy advisory capability flags.
    pub const fn validate(self) -> Result<(), ContractError> {
        match self {
            Self::CliCommonBase => Ok(()),
            Self::Legacy { .. } => Err(ContractError::NotAuthoritative),
        }
    }
}

/// Shared metadata for a CLI binary.
#[derive(Debug, Clone)]
pub struct ToolSpec {
    /// Binary name shown in version output and help text.
    pub(crate) bin_name: &'static str,
    /// Human-readable tool name.
    #[allow(
        dead_code,
        reason = "set by the public ToolSpec constructor and read via the display_name() accessor; dead_code fires only in binaries that never call that accessor"
    )]
    pub(crate) display_name: &'static str,
    /// Binary version.
    pub(crate) version: &'static str,
    /// License rendered by the shared license command.
    pub(crate) license: LicenseType,
    /// Repository metadata used by workspace tooling.
    #[allow(
        dead_code,
        reason = "set by the public ToolSpec constructor and read via its accessor; dead_code fires only in binaries that never read it"
    )]
    pub(crate) repo: RepoInfo,
    /// Shared CLI contract adoption marker.
    pub(crate) contract: ToolContract,
    /// Declarative agent-mode surface for the tool.
    pub(crate) agent_surface: Option<&'static AgentSurfaceSpec>,
}

impl ToolSpec {
    /// Create a new [`ToolSpec`].
    #[must_use]
    pub const fn new(
        bin_name: &'static str,
        display_name: &'static str,
        version: &'static str,
        license: LicenseType,
        repo: RepoInfo,
        supports_json: bool,
        supports_doctor: bool,
    ) -> Self {
        Self {
            bin_name,
            display_name,
            version,
            license,
            repo,
            contract: ToolContract::from_support_flags(supports_json, supports_doctor),
            agent_surface: None,
        }
    }

    /// Create a new [`ToolSpec`] using [`WORKSPACE_REPO`].
    #[must_use]
    pub const fn workspace(
        bin_name: &'static str,
        display_name: &'static str,
        version: &'static str,
        license: LicenseType,
        supports_json: bool,
        supports_doctor: bool,
    ) -> Self {
        Self::new(
            bin_name,
            display_name,
            version,
            license,
            WORKSPACE_REPO,
            supports_json,
            supports_doctor,
        )
    }

    /// Attach an agent surface to the tool specification.
    #[must_use]
    pub const fn with_agent_surface(self, agent_surface: &'static AgentSurfaceSpec) -> Self {
        Self {
            agent_surface: Some(agent_surface),
            ..self
        }
    }

    /// Return the binary name.
    #[must_use]
    pub const fn bin_name(&self) -> &'static str {
        self.bin_name
    }

    /// Return the human-readable tool name.
    #[must_use]
    pub const fn display_name(&self) -> &'static str {
        self.display_name
    }

    /// Return the tool version.
    #[must_use]
    pub const fn version(&self) -> &'static str {
        self.version
    }

    /// Return the agent surface, if configured.
    #[must_use]
    pub const fn agent_surface(&self) -> Option<&'static AgentSurfaceSpec> {
        self.agent_surface
    }

    /// Whether the tool supports JSON output in base commands.
    #[must_use]
    pub const fn supports_json(&self) -> bool {
        self.contract.supports_json()
    }

    /// Whether the tool exposes doctor checks.
    #[must_use]
    pub const fn supports_doctor(&self) -> bool {
        self.contract.supports_doctor()
    }

    /// Whether the tool has opted into the authoritative shared CLI contract.
    #[must_use]
    pub const fn has_authoritative_contract(&self) -> bool {
        self.contract.is_authoritative()
    }

    /// Validate that the tool uses the authoritative shared CLI contract.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError::NotAuthoritative`] when the tool still relies on
    /// legacy advisory flags.
    pub const fn validate_authoritative_contract(&self) -> Result<(), ContractError> {
        self.contract.validate()
    }
}

/// Create a [`ToolSpec`] for a binary shipped from the shared tools workspace.
#[must_use]
pub const fn workspace_tool(
    bin_name: &'static str,
    display_name: &'static str,
    version: &'static str,
    license: LicenseType,
    supports_json: bool,
    supports_doctor: bool,
) -> ToolSpec {
    ToolSpec::workspace(
        bin_name,
        display_name,
        version,
        license,
        supports_json,
        supports_doctor,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_spec_new_preserves_fields() {
        let spec = ToolSpec::new(
            "tool",
            "Tool",
            "1.2.3",
            LicenseType::MIT,
            RepoInfo::new("owner", "repo"),
            true,
            false,
        );

        assert_eq!(spec.bin_name, "tool");
        assert_eq!(spec.display_name, "Tool");
        assert_eq!(spec.version, "1.2.3");
        assert_eq!(spec.bin_name(), "tool");
        assert_eq!(spec.display_name(), "Tool");
        assert_eq!(spec.version(), "1.2.3");
        assert_eq!(spec.license, LicenseType::MIT);
        assert_eq!(spec.repo.owner, "owner");
        assert_eq!(spec.repo.name, "repo");
        assert_eq!(
            spec.contract,
            ToolContract::Legacy {
                supports_json: true,
                supports_doctor: false,
            }
        );
        assert!(spec.supports_json());
        assert!(!spec.supports_doctor());
        assert!(!spec.has_authoritative_contract());
        assert!(spec.validate_authoritative_contract().is_err());
        assert!(spec.agent_surface().is_none());
    }

    #[test]
    fn workspace_tool_uses_workspace_repo_defaults() {
        let spec = workspace_tool("tool", "Tool", "1.2.3", LicenseType::MIT, true, false);

        assert_eq!(spec.repo.owner, WORKSPACE_REPO.owner);
        assert_eq!(spec.repo.name, WORKSPACE_REPO.name);
        assert_eq!(spec.bin_name, "tool");
        assert_eq!(spec.display_name, "Tool");
    }

    #[test]
    fn tool_spec_with_all_mandatory_capabilities_is_authoritative() {
        let spec = workspace_tool("tool", "Tool", "1.2.3", LicenseType::MIT, true, true);

        assert_eq!(spec.contract, ToolContract::CliCommonBase);
        assert!(spec.has_authoritative_contract());
        assert!(spec.validate_authoritative_contract().is_ok());
        assert!(spec.supports_json());
        assert!(spec.supports_doctor());
    }
}
