//! Shared types for tftio CLI tools.

/// Repository information for CLI tools.
///
/// This structure holds basic repository metadata for identification purposes.
#[derive(Debug, Clone)]
pub struct RepoInfo {
    /// Repository owner (e.g., "tftio")
    pub owner: &'static str,
    /// Repository name (e.g., "prompter")
    pub name: &'static str,
}

impl RepoInfo {
    /// Create a new `RepoInfo` instance.
    #[must_use]
    pub const fn new(owner: &'static str, name: &'static str) -> Self {
        Self { owner, name }
    }
}

/// Health check result for doctor command.
#[derive(Debug, Clone)]
pub struct DoctorCheck {
    /// Name of the check
    pub name: String,
    /// Whether the check passed
    pub passed: bool,
    /// Optional message
    pub message: Option<String>,
}

impl DoctorCheck {
    /// Create a new passing check.
    #[must_use]
    pub fn pass(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            passed: true,
            message: None,
        }
    }

    /// Create a new failing check with a message.
    #[must_use]
    pub fn fail(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            passed: false,
            message: Some(message.into()),
        }
    }

    /// Create a file existence check.
    ///
    /// # Errors
    /// Returns a failing check if the file doesn't exist.
    pub fn file_exists(path: impl AsRef<std::path::Path>) -> Self {
        let path_ref = path.as_ref();
        if path_ref.exists() && path_ref.is_file() {
            Self::pass(format!("File exists: {}", path_ref.display()))
        } else {
            Self::fail(
                format!("File check: {}", path_ref.display()),
                format!("File not found: {}", path_ref.display()),
            )
        }
    }

    /// Create a directory existence check.
    ///
    /// # Errors
    /// Returns a failing check if the directory doesn't exist.
    pub fn dir_exists(path: impl AsRef<std::path::Path>) -> Self {
        let path_ref = path.as_ref();
        if path_ref.exists() && path_ref.is_dir() {
            Self::pass(format!("Directory exists: {}", path_ref.display()))
        } else {
            Self::fail(
                format!("Directory check: {}", path_ref.display()),
                format!("Directory not found: {}", path_ref.display()),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repo_info_creation() {
        let repo = RepoInfo::new("example", "prompter");
        assert_eq!(repo.owner, "example");
        assert_eq!(repo.name, "prompter");
    }

    #[test]
    fn test_doctor_check_pass() {
        let check = DoctorCheck::pass("test check");
        assert!(check.passed);
        assert_eq!(check.name, "test check");
        assert!(check.message.is_none());
    }

    #[test]
    fn test_doctor_check_fail() {
        let check = DoctorCheck::fail("test check", "error message");
        assert!(!check.passed);
        assert_eq!(check.name, "test check");
        assert_eq!(check.message, Some("error message".to_string()));
    }

    fn unique_temp_path(label: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "tftio-lib-types-{label}-{}-{n}",
            std::process::id()
        ))
    }

    #[test]
    fn file_exists_passes_for_a_real_file() {
        let path = unique_temp_path("file-pass");
        std::fs::write(&path, b"data").unwrap();
        let check = DoctorCheck::file_exists(&path);
        assert!(check.passed);
        assert!(check.name.starts_with("File exists:"));
        assert!(check.message.is_none());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn file_exists_fails_when_the_path_is_missing() {
        let path = unique_temp_path("file-missing");
        let check = DoctorCheck::file_exists(&path);
        assert!(!check.passed);
        assert_eq!(
            check.message,
            Some(format!("File not found: {}", path.display()))
        );
    }

    #[test]
    fn file_exists_fails_when_the_path_is_a_directory() {
        let path = unique_temp_path("file-is-dir");
        std::fs::create_dir(&path).unwrap();
        let check = DoctorCheck::file_exists(&path);
        assert!(!check.passed);
        assert!(check.name.starts_with("File check:"));
        std::fs::remove_dir(&path).unwrap();
    }

    #[test]
    fn dir_exists_passes_for_a_real_directory() {
        let path = unique_temp_path("dir-pass");
        std::fs::create_dir(&path).unwrap();
        let check = DoctorCheck::dir_exists(&path);
        assert!(check.passed);
        assert!(check.name.starts_with("Directory exists:"));
        assert!(check.message.is_none());
        std::fs::remove_dir(&path).unwrap();
    }

    #[test]
    fn dir_exists_fails_when_the_path_is_missing() {
        let path = unique_temp_path("dir-missing");
        let check = DoctorCheck::dir_exists(&path);
        assert!(!check.passed);
        assert_eq!(
            check.message,
            Some(format!("Directory not found: {}", path.display()))
        );
    }

    #[test]
    fn dir_exists_fails_when_the_path_is_a_file() {
        let path = unique_temp_path("dir-is-file");
        std::fs::write(&path, b"data").unwrap();
        let check = DoctorCheck::dir_exists(&path);
        assert!(!check.passed);
        assert!(check.name.starts_with("Directory check:"));
        std::fs::remove_file(&path).unwrap();
    }
}
