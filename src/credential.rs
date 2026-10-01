//! Credential resolution at the process edge.
//!
//! Wrapper CLIs in this workspace must not read a credential's *value* from the
//! ambient environment. A tool whose token is also present in its caller's
//! environment narrows what is convenient, not what is possible: the same
//! upstream API stays reachable with the same credential, so the tool's command
//! surface bounds nothing. This module exists so a tool is configured with a
//! *reference* to a secret — a file path, or a 1Password `op://` reference —
//! and resolves it at call time.
//!
//! Two rules follow from that and are enforced here rather than left to callers:
//!
//! 1. **No silent fallback.** A source that is configured but unavailable — a
//!    missing file, a locked 1Password agent — is an error naming the source,
//!    never a quiet degradation to some other credential path. A locked keyring
//!    is a stop condition, not a problem to route around.
//! 2. **No secret in an error.** Every variant of [`CredentialError`] carries
//!    the *source* it failed on and never the value it was reading. Errors are
//!    printed, logged, and pasted into issues; a resolver that leaks its secret
//!    into a message has defeated its own purpose.
//!
//! Like the rest of this crate, nothing here reads [`mod@std::env`]
//! (`REPO_INVARIANTS.md` ENG-008). The `PATH` and `HOME` a helper subprocess
//! needs arrive as a caller-supplied [`CredentialInvocation`], and that
//! subprocess runs under a cleared environment carrying only those two values,
//! so resolution cannot be steered by whatever the calling process happened to
//! inherit.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

use secrecy::SecretString;

/// The 1Password CLI invoked to resolve an `op://` reference.
const OP_PROGRAM: &str = "op";

/// The scheme marking a configured reference as a 1Password secret reference.
const OP_SCHEME: &str = "op://";

/// A failure to resolve a configured credential.
///
/// No variant carries the credential's value.
#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    /// A configured `op://` reference is not well formed.
    #[error("malformed 1Password reference {raw:?}: {reason}")]
    MalformedReference {
        /// The reference as configured. A reference is a locator, not a secret.
        raw: String,
        /// Why the reference could not be parsed.
        reason: &'static str,
    },

    /// A credential file could not be read.
    #[error("cannot read credential file {path}: {source}")]
    FileUnreadable {
        /// The configured path.
        path: PathBuf,
        /// The underlying I/O failure.
        source: std::io::Error,
    },

    /// The helper program that resolves `op://` references could not be run.
    ///
    /// Typically the 1Password CLI is not installed or is not on the supplied
    /// `PATH`.
    #[error("cannot run `{program}` to resolve {reference}: {source}")]
    HelperUnavailable {
        /// The helper program that could not be spawned.
        program: String,
        /// The reference being resolved.
        reference: String,
        /// The underlying spawn failure.
        source: std::io::Error,
    },

    /// The helper program ran and reported failure.
    ///
    /// A locked 1Password agent arrives here, and its own message is the useful
    /// part, so it is carried through verbatim.
    #[error("`{program}` failed to resolve {reference} ({status}): {stderr}")]
    HelperFailed {
        /// The helper program that failed.
        program: String,
        /// The reference being resolved.
        reference: String,
        /// How the helper exited.
        status: String,
        /// What the helper wrote to stderr, trimmed.
        stderr: String,
    },

    /// A source resolved to bytes that are not valid UTF-8.
    #[error("credential from {source_description} is not valid UTF-8")]
    NotUtf8 {
        /// A description of the source, never its contents.
        source_description: String,
    },

    /// A source resolved successfully but held nothing.
    ///
    /// An empty credential is a misconfiguration. Failing here beats presenting
    /// an empty bearer token to an upstream API and reporting whatever
    /// authentication error comes back.
    #[error("credential from {source_description} is empty")]
    Empty {
        /// A description of the source, never its contents.
        source_description: String,
    },
}

/// A validated 1Password secret reference (`op://vault/item/field`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnePasswordRef(String);

impl OnePasswordRef {
    /// Parse and validate an `op://vault/item/field` reference.
    ///
    /// Validation is deliberately shallow: the scheme must be present and the
    /// path after it must have at least two non-empty segments. Whether the
    /// vault, item, and field exist is the 1Password CLI's judgment, not this
    /// crate's, and a wrong answer there arrives as
    /// [`CredentialError::HelperFailed`] with the CLI's own message.
    ///
    /// # Errors
    ///
    /// Returns [`CredentialError::MalformedReference`] when `raw` does not
    /// begin with `op://` or does not carry enough non-empty path segments.
    pub fn parse(raw: &str) -> Result<Self, CredentialError> {
        let malformed = |reason: &'static str| CredentialError::MalformedReference {
            raw: raw.to_owned(),
            reason,
        };

        let rest = raw
            .strip_prefix(OP_SCHEME)
            .ok_or_else(|| malformed("expected an op:// prefix"))?;

        let segments: Vec<&str> = rest.split('/').collect();
        if segments.len() < 2 {
            return Err(malformed(
                "expected at least op://<vault>/<item>, with an optional /<field>",
            ));
        }
        if segments.iter().any(|segment| segment.is_empty()) {
            return Err(malformed("reference has an empty path segment"));
        }

        Ok(Self(raw.to_owned()))
    }

    /// The reference as configured.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Where a credential's value is read from.
///
/// A tool is configured with one of these rather than with a secret, so the
/// secret itself never has to live in a config file, a shell profile, or the
/// environment of whatever launched the process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialSource {
    /// A filesystem path whose contents are the credential.
    File(PathBuf),
    /// A 1Password secret reference resolved through the `op` CLI.
    OnePassword(OnePasswordRef),
}

impl CredentialSource {
    /// Interpret a configured string as a credential source.
    ///
    /// A value beginning with `op://` is a 1Password reference; anything else
    /// is a filesystem path. The path is not required to exist yet — a missing
    /// file is reported at resolution time, where the error can name what was
    /// being resolved.
    ///
    /// # Errors
    ///
    /// Returns [`CredentialError::MalformedReference`] when the value begins
    /// with `op://` but is not a well-formed reference.
    pub fn parse(raw: &str) -> Result<Self, CredentialError> {
        if raw.starts_with(OP_SCHEME) {
            return Ok(Self::OnePassword(OnePasswordRef::parse(raw)?));
        }
        Ok(Self::File(PathBuf::from(raw)))
    }

    /// A human-readable description of where the credential comes from.
    ///
    /// Safe to log: it describes the source, never the value.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::File(path) => format!("file {}", path.display()),
            Self::OnePassword(reference) => format!("1Password reference {}", reference.as_str()),
        }
    }
}

/// The explicitly constructed environment a helper subprocess runs under.
///
/// Mirrors the discipline the rest of the workspace applies to subprocesses:
/// the child's environment is cleared and rebuilt from these values, so
/// credential resolution cannot be steered by the ambient environment. Both
/// fields are read at the binary's edge and passed inward
/// (`REPO_INVARIANTS.md` ENG-008).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialInvocation {
    path: OsString,
    home: PathBuf,
}

impl CredentialInvocation {
    /// Construct an invocation from edge-read `PATH` and `HOME` values.
    #[must_use]
    pub const fn new(path: OsString, home: PathBuf) -> Self {
        Self { path, home }
    }

    /// The `PATH` a helper subprocess is given.
    #[must_use]
    pub fn path(&self) -> &OsStr {
        &self.path
    }

    /// The `HOME` a helper subprocess is given.
    #[must_use]
    pub fn home(&self) -> &Path {
        &self.home
    }
}

/// Resolve a configured source to its credential value.
///
/// # Errors
///
/// Returns [`CredentialError`] when the source cannot be read, a helper cannot
/// be run or reports failure, or the resolved value is empty or not UTF-8. No
/// error carries the credential's value, and no failure falls back to another
/// source.
pub fn resolve(
    source: &CredentialSource,
    invocation: &CredentialInvocation,
) -> Result<SecretString, CredentialError> {
    let raw = match source {
        CredentialSource::File(path) => read_file(path)?,
        CredentialSource::OnePassword(reference) => read_one_password(reference, invocation)?,
    };

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(CredentialError::Empty {
            source_description: source.describe(),
        });
    }

    Ok(SecretString::from(trimmed.to_owned()))
}

/// Read a credential file's contents as UTF-8.
fn read_file(path: &Path) -> Result<String, CredentialError> {
    let bytes = std::fs::read(path).map_err(|source| CredentialError::FileUnreadable {
        path: path.to_path_buf(),
        source,
    })?;

    String::from_utf8(bytes).map_err(|_| CredentialError::NotUtf8 {
        source_description: format!("file {}", path.display()),
    })
}

/// Resolve an `op://` reference through the 1Password CLI.
///
/// The child runs under a cleared environment carrying only the supplied `PATH`
/// and `HOME`. A locked 1Password agent surfaces as
/// [`CredentialError::HelperFailed`] carrying the CLI's own stderr, which is
/// the part that tells the operator what to unlock.
fn read_one_password(
    reference: &OnePasswordRef,
    invocation: &CredentialInvocation,
) -> Result<String, CredentialError> {
    let mut command = Command::new(OP_PROGRAM);
    command.env_clear();
    command.env("PATH", invocation.path());
    command.env("HOME", invocation.home());
    command.arg("read");
    command.arg(reference.as_str());

    let output = command
        .output()
        .map_err(|source| CredentialError::HelperUnavailable {
            program: OP_PROGRAM.to_owned(),
            reference: reference.as_str().to_owned(),
            source,
        })?;

    if !output.status.success() {
        return Err(CredentialError::HelperFailed {
            program: OP_PROGRAM.to_owned(),
            reference: reference.as_str().to_owned(),
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    String::from_utf8(output.stdout).map_err(|_| CredentialError::NotUtf8 {
        source_description: format!("1Password reference {}", reference.as_str()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret as _;
    use std::io::Write as _;

    /// A `CredentialInvocation` whose `PATH` is `dir` alone, so a stub helper
    /// placed there is the only `op` the resolver can find.
    fn invocation_with_path(dir: &Path) -> CredentialInvocation {
        CredentialInvocation::new(OsString::from(dir), dir.to_path_buf())
    }

    /// Write an executable stub named `op` into `dir`.
    #[cfg(unix)]
    fn write_op_stub(dir: &Path, body: &str) {
        use std::os::unix::fs::PermissionsExt as _;

        let path = dir.join(OP_PROGRAM);
        let mut file = std::fs::File::create(&path).expect("create stub");
        file.write_all(body.as_bytes()).expect("write stub");
        drop(file);
        let mut permissions = std::fs::metadata(&path).expect("stat stub").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).expect("chmod stub");
    }

    #[test]
    fn parses_a_filesystem_path_as_a_file_source() {
        let source = CredentialSource::parse("/run/secrets/api-token").expect("parse");
        assert_eq!(
            source,
            CredentialSource::File(PathBuf::from("/run/secrets/api-token"))
        );
    }

    #[test]
    fn parses_an_op_reference_as_a_one_password_source() {
        let source = CredentialSource::parse("op://vault/item/field").expect("parse");
        assert_eq!(
            source,
            CredentialSource::OnePassword(
                OnePasswordRef::parse("op://vault/item/field").expect("parse ref")
            )
        );
    }

    #[test]
    fn rejects_an_op_reference_with_too_few_segments() {
        let error = OnePasswordRef::parse("op://vault").expect_err("must reject");
        assert!(matches!(error, CredentialError::MalformedReference { .. }));
    }

    #[test]
    fn rejects_an_op_reference_with_an_empty_segment() {
        let error = OnePasswordRef::parse("op://vault//field").expect_err("must reject");
        assert!(matches!(error, CredentialError::MalformedReference { .. }));
    }

    #[test]
    fn resolves_a_credential_file_and_trims_its_trailing_newline() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("token");
        std::fs::write(&path, "s3cr3t-value\n").expect("write");

        let secret = resolve(
            &CredentialSource::File(path),
            &invocation_with_path(dir.path()),
        )
        .expect("resolve");

        assert_eq!(secret.expose_secret(), "s3cr3t-value");
    }

    #[test]
    fn reports_a_missing_credential_file_by_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("absent");

        let error = resolve(
            &CredentialSource::File(path.clone()),
            &invocation_with_path(dir.path()),
        )
        .expect_err("must fail");

        assert!(matches!(error, CredentialError::FileUnreadable { .. }));
        assert!(error.to_string().contains(&path.display().to_string()));
    }

    #[test]
    fn rejects_an_empty_credential_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("token");
        std::fs::write(&path, "   \n").expect("write");

        let error = resolve(
            &CredentialSource::File(path),
            &invocation_with_path(dir.path()),
        )
        .expect_err("must fail");

        assert!(matches!(error, CredentialError::Empty { .. }));
    }

    #[test]
    #[cfg(unix)]
    fn resolves_an_op_reference_through_the_helper() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_op_stub(dir.path(), "#!/bin/sh\nprintf 'from-op\\n'\n");

        let source = CredentialSource::parse("op://vault/item/field").expect("parse");
        let secret = resolve(&source, &invocation_with_path(dir.path())).expect("resolve");

        assert_eq!(secret.expose_secret(), "from-op");
    }

    #[test]
    #[cfg(unix)]
    fn surfaces_a_locked_helper_message_verbatim() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_op_stub(
            dir.path(),
            "#!/bin/sh\necho 'error: 1Password is locked' >&2\nexit 1\n",
        );

        let source = CredentialSource::parse("op://vault/item/field").expect("parse");
        let error = resolve(&source, &invocation_with_path(dir.path())).expect_err("must fail");

        assert!(matches!(error, CredentialError::HelperFailed { .. }));
        let rendered = error.to_string();
        assert!(rendered.contains("1Password is locked"));
        assert!(rendered.contains("op://vault/item/field"));
    }

    #[test]
    #[cfg(unix)]
    fn reports_an_absent_helper_rather_than_falling_back() {
        let dir = tempfile::tempdir().expect("tempdir");

        let source = CredentialSource::parse("op://vault/item/field").expect("parse");
        let error = resolve(&source, &invocation_with_path(dir.path())).expect_err("must fail");

        assert!(matches!(error, CredentialError::HelperUnavailable { .. }));
    }

    #[test]
    #[cfg(unix)]
    fn runs_the_helper_under_a_cleared_environment() {
        // `cargo test` sets CARGO_PKG_NAME in this process. A child built from
        // a cleared environment must not see it, and must see exactly the HOME
        // the invocation supplied rather than the caller's.
        assert!(
            std::env::var("CARGO_PKG_NAME").is_ok(),
            "precondition: the test process must carry CARGO_PKG_NAME"
        );

        let dir = tempfile::tempdir().expect("tempdir");
        write_op_stub(
            dir.path(),
            "#!/bin/sh\nprintf '%s %s' \"${CARGO_PKG_NAME-unset}\" \"$HOME\"\n",
        );

        let source = CredentialSource::parse("op://vault/item/field").expect("parse");
        let secret = resolve(&source, &invocation_with_path(dir.path())).expect("resolve");

        assert_eq!(
            secret.expose_secret(),
            format!("unset {}", dir.path().display())
        );
    }

    #[test]
    fn describes_sources_without_exposing_values() {
        let file = CredentialSource::File(PathBuf::from("/run/secrets/api-token"));
        assert_eq!(file.describe(), "file /run/secrets/api-token");

        let reference = CredentialSource::parse("op://vault/item/field").expect("parse");
        assert_eq!(
            reference.describe(),
            "1Password reference op://vault/item/field"
        );
    }
}
