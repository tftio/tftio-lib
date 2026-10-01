//! The project registry: `projects.toml` plus a machine-local overlay.
//!
//! The registry is a file the operator edits directly, normally at
//! `${XDG_CONFIG_HOME:-~/.config}/tftio/projects.toml`
//! ([`default_registry_dir`] computes that directory from explicit
//! arguments; this module never reads the environment or the current
//! directory itself). A sibling `projects.local.toml` overlays it for
//! paths that exist on one machine only, merged per slug: the union of
//! `remotes` and `paths`, deduplicated.
//!
//! ```toml
//! [project.kb]
//! remotes = ["github.com/tftio/kb"]
//! paths = ["~/Documents/Repositories/kb/main"]
//!
//! [project.notes]
//! paths = ["~/Documents/notes"]
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::remote::{RemoteError, normalize_remote};
use super::slug::{Slug, SlugError};

/// The base registry file name, within the registry directory.
const REGISTRY_FILE: &str = "projects.toml";

/// The machine-local overlay file name, within the registry directory.
const OVERLAY_FILE: &str = "projects.local.toml";

/// A loaded, merged project registry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Registry {
    projects: BTreeMap<String, RegistryProject>,
}

/// One project's entries in the registry: its known remotes and known
/// paths, each a deduplicated set after the base file and its overlay are
/// merged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegistryProject {
    /// Normalized remote URLs registered for this project.
    pub remotes: std::collections::BTreeSet<String>,
    /// Filesystem paths registered for this project, with `~` already
    /// expanded against the `home` the loader was given.
    pub paths: std::collections::BTreeSet<PathBuf>,
}

/// A failure loading or parsing a registry file.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    /// A registry file exists but could not be read.
    #[error("cannot read {path}: {source}")]
    Io {
        /// The file that could not be read.
        path: PathBuf,
        /// The underlying I/O failure.
        source: std::io::Error,
    },
    /// A registry file exists but is not valid TOML matching the registry
    /// schema.
    #[error("cannot parse {path}: {source}")]
    Parse {
        /// The file that failed to parse.
        path: PathBuf,
        /// The TOML diagnostic.
        source: toml::de::Error,
    },
}

/// One issue found by [`Registry::validate`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ValidationIssue {
    /// A `[project.<slug>]` table key is not a valid slug.
    #[error("{slug:?} is not a valid project slug: {reason}")]
    InvalidSlug {
        /// The table key as written in the registry.
        slug: String,
        /// Why it is not a valid slug.
        reason: SlugError,
    },
    /// A registered remote does not normalize.
    #[error("project {slug:?} has an unnormalizable remote {remote:?}: {reason}")]
    RemoteNotNormalizable {
        /// The project the remote is registered under.
        slug: String,
        /// The remote text as written in the registry.
        remote: String,
        /// Why it does not normalize.
        reason: RemoteError,
    },
    /// A registered remote normalizes, but not to itself: the registry
    /// should store the normalized form.
    #[error(
        "project {slug:?} has a non-normalized remote {remote:?}; the registry should store {normalized:?}"
    )]
    RemoteNotNormalized {
        /// The project the remote is registered under.
        slug: String,
        /// The remote text as written in the registry.
        remote: String,
        /// The normalized form the registry should hold instead.
        normalized: String,
    },
    /// One normalized remote is registered under two different slugs.
    #[error("remote {remote:?} is registered under both {first:?} and {second:?}")]
    DuplicateRemote {
        /// The remote registered twice.
        remote: String,
        /// The first slug found holding it.
        first: String,
        /// The second slug found holding it.
        second: String,
    },
    /// One path is registered under two different slugs.
    #[error("path {path:?} is registered under both {first:?} and {second:?}")]
    DuplicatePath {
        /// The path registered twice.
        path: PathBuf,
        /// The first slug found holding it.
        first: String,
        /// The second slug found holding it.
        second: String,
    },
    /// A registered path is not absolute.
    #[error(
        "project {slug:?} has a relative path {path:?}; registry paths must be absolute or ~-prefixed"
    )]
    RelativePath {
        /// The project the path is registered under.
        slug: String,
        /// The relative path as loaded (after `~` expansion).
        path: PathBuf,
    },
}

/// The raw shape of one registry file, before merging.
#[derive(Debug, Default, Deserialize)]
struct RawRegistryFile {
    #[serde(default)]
    project: BTreeMap<String, RawProjectEntry>,
}

/// The raw shape of one `[project.<slug>]` table, before path expansion.
#[derive(Debug, Default, Deserialize)]
struct RawProjectEntry {
    #[serde(default)]
    remotes: Vec<String>,
    #[serde(default)]
    paths: Vec<String>,
}

impl Registry {
    /// Whether the registry holds no projects at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.projects.is_empty()
    }

    /// The number of projects in the registry.
    #[must_use]
    pub fn len(&self) -> usize {
        self.projects.len()
    }

    /// Iterate over every project in the registry, keyed by its slug text
    /// as written in the registry (not yet validated).
    pub fn projects(&self) -> impl Iterator<Item = (&str, &RegistryProject)> {
        self.projects
            .iter()
            .map(|(slug, project)| (slug.as_str(), project))
    }

    /// Look up a project's entry by slug text.
    #[must_use]
    pub fn get(&self, slug: &str) -> Option<&RegistryProject> {
        self.projects.get(slug)
    }

    /// Validate the registry, returning every issue found.
    ///
    /// An empty result means the registry is internally consistent: every
    /// slug is well formed, every remote is already normalized, and no
    /// remote or path is claimed by two projects. This does not check that
    /// a registered path exists on the current machine; that is a
    /// machine-local concern for the caller (`clanker doctor`, in this
    /// fleet) to report as a warning rather than a validation failure.
    #[must_use]
    pub fn validate(&self) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        let mut remote_owner: BTreeMap<&str, &str> = BTreeMap::new();
        let mut path_owner: BTreeMap<&Path, &str> = BTreeMap::new();

        for (slug, project) in &self.projects {
            if let Err(reason) = Slug::new(slug.clone()) {
                issues.push(ValidationIssue::InvalidSlug {
                    slug: slug.clone(),
                    reason,
                });
            }

            for remote in &project.remotes {
                match normalize_remote(remote) {
                    Err(reason) => issues.push(ValidationIssue::RemoteNotNormalizable {
                        slug: slug.clone(),
                        remote: remote.clone(),
                        reason,
                    }),
                    Ok(normalized) if normalized.as_str() != remote => {
                        issues.push(ValidationIssue::RemoteNotNormalized {
                            slug: slug.clone(),
                            remote: remote.clone(),
                            normalized: normalized.into_string(),
                        });
                    }
                    Ok(_) => {}
                }

                match remote_owner.get(remote.as_str()) {
                    Some(&first) if first != slug => {
                        issues.push(ValidationIssue::DuplicateRemote {
                            remote: remote.clone(),
                            first: first.to_string(),
                            second: slug.clone(),
                        });
                    }
                    Some(_) => {}
                    None => {
                        remote_owner.insert(remote.as_str(), slug.as_str());
                    }
                }
            }

            for path in &project.paths {
                if !path.is_absolute() {
                    issues.push(ValidationIssue::RelativePath {
                        slug: slug.clone(),
                        path: path.clone(),
                    });
                }

                match path_owner.get(path.as_path()) {
                    Some(&first) if first != slug => issues.push(ValidationIssue::DuplicatePath {
                        path: path.clone(),
                        first: first.to_string(),
                        second: slug.clone(),
                    }),
                    Some(_) => {}
                    None => {
                        path_owner.insert(path.as_path(), slug.as_str());
                    }
                }
            }
        }

        issues
    }
}

/// Compute the default registry directory from explicit arguments.
///
/// Mirrors `${XDG_CONFIG_HOME:-~/.config}/tftio` without reading either
/// variable itself: the caller reads `XDG_CONFIG_HOME` and `HOME` at the
/// process edge and passes the results in. Returns `None` when neither is
/// given, since no default directory can be computed.
#[must_use]
pub fn default_registry_dir(
    xdg_config_home: Option<&Path>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(xdg_config_home) = xdg_config_home {
        return Some(xdg_config_home.join("tftio"));
    }
    home.map(|home| home.join(".config").join("tftio"))
}

/// Load and merge the registry from `dir`: `projects.toml` plus the
/// `projects.local.toml` overlay, if present.
///
/// A missing base file is an empty registry; a missing overlay file is
/// simply not merged. Entries merge per slug: the union of `remotes` and
/// of `paths`, deduplicated. `~`-prefixed paths are expanded against
/// `home`; a path with no `home` given is left as written (verbatim), which
/// [`Registry::validate`] then reports as [`ValidationIssue::RelativePath`]
/// if it is not absolute.
///
/// # Errors
///
/// Returns [`RegistryError::Io`] when a present file cannot be read for a
/// reason other than not existing, and [`RegistryError::Parse`] when a
/// present file is not valid TOML matching the registry schema.
pub fn load_registry(dir: &Path, home: Option<&Path>) -> Result<Registry, RegistryError> {
    let base = load_registry_file(&dir.join(REGISTRY_FILE))?.unwrap_or_default();
    let overlay = load_registry_file(&dir.join(OVERLAY_FILE))?.unwrap_or_default();

    let mut projects: BTreeMap<String, RegistryProject> = BTreeMap::new();
    for (slug, entry) in base.project.into_iter().chain(overlay.project) {
        let target = projects.entry(slug).or_default();
        for remote in entry.remotes {
            target.remotes.insert(remote);
        }
        for path in entry.paths {
            target.paths.insert(expand_tilde(&path, home));
        }
    }

    Ok(Registry { projects })
}

/// Load and parse one registry file, returning `None` when it does not
/// exist.
fn load_registry_file(path: &Path) -> Result<Option<RawRegistryFile>, RegistryError> {
    match fs::read_to_string(path) {
        Ok(contents) => {
            let parsed = toml::from_str(&contents).map_err(|source| RegistryError::Parse {
                path: path.to_path_buf(),
                source,
            })?;
            Ok(Some(parsed))
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(RegistryError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Expand a leading `~` in `raw` against `home`.
///
/// `raw` is returned unchanged when it has no leading `~`, or when it does
/// but no `home` was given; the latter case leaves an unresolved path for
/// [`Registry::validate`] to flag as relative.
fn expand_tilde(raw: &str, home: Option<&Path>) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = home {
            return home.join(rest);
        }
        return PathBuf::from(raw);
    }
    if raw == "~"
        && let Some(home) = home
    {
        return home.to_path_buf();
    }
    PathBuf::from(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_registry_dir_prefers_xdg_config_home() {
        let dir = default_registry_dir(Some(Path::new("/xdg")), Some(Path::new("/home/j")));
        assert_eq!(Some(PathBuf::from("/xdg/tftio")), dir);
    }

    #[test]
    fn default_registry_dir_falls_back_to_home_dot_config() {
        let dir = default_registry_dir(None, Some(Path::new("/home/j")));
        assert_eq!(Some(PathBuf::from("/home/j/.config/tftio")), dir);
    }

    #[test]
    fn default_registry_dir_is_none_with_neither_input() {
        assert_eq!(None, default_registry_dir(None, None));
    }

    #[test]
    fn load_registry_is_empty_when_no_files_exist() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        let registry = load_registry(dir.path(), None).expect("empty registry");
        assert!(registry.is_empty());
        Ok(())
    }

    #[test]
    fn load_registry_reads_the_base_file() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        fs::write(
            dir.path().join(REGISTRY_FILE),
            "[project.kb]\nremotes = [\"github.com/tftio/kb\"]\npaths = [\"/repos/kb\"]\n",
        )?;

        let registry = load_registry(dir.path(), None).expect("valid registry");
        let kb = registry.get("kb").expect("kb project present");
        assert!(kb.remotes.contains("github.com/tftio/kb"));
        assert!(kb.paths.contains(&PathBuf::from("/repos/kb")));
        Ok(())
    }

    #[test]
    fn load_registry_expands_tilde_paths_against_home() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        fs::write(
            dir.path().join(REGISTRY_FILE),
            "[project.kb]\npaths = [\"~/Documents/kb\"]\n",
        )?;

        let registry =
            load_registry(dir.path(), Some(Path::new("/home/j"))).expect("valid registry");
        let kb = registry.get("kb").expect("kb project present");
        assert!(kb.paths.contains(&PathBuf::from("/home/j/Documents/kb")));
        Ok(())
    }

    #[test]
    fn load_registry_merges_the_overlay_per_slug() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        fs::write(
            dir.path().join(REGISTRY_FILE),
            "[project.kb]\nremotes = [\"github.com/tftio/kb\"]\npaths = [\"/repos/kb\"]\n",
        )?;
        fs::write(
            dir.path().join(OVERLAY_FILE),
            "[project.kb]\npaths = [\"/other/kb\"]\n\n[project.notes]\npaths = [\"/repos/notes\"]\n",
        )?;

        let registry = load_registry(dir.path(), None).expect("valid registry");
        let kb = registry.get("kb").expect("kb project present");
        assert_eq!(1, kb.remotes.len());
        assert_eq!(2, kb.paths.len());
        assert!(kb.paths.contains(&PathBuf::from("/repos/kb")));
        assert!(kb.paths.contains(&PathBuf::from("/other/kb")));
        assert!(registry.get("notes").is_some());
        Ok(())
    }

    #[test]
    fn load_registry_is_fine_with_a_missing_overlay() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        fs::write(
            dir.path().join(REGISTRY_FILE),
            "[project.kb]\nremotes = [\"github.com/tftio/kb\"]\n",
        )?;

        let registry = load_registry(dir.path(), None).expect("valid registry");
        assert_eq!(1, registry.len());
        Ok(())
    }

    #[test]
    fn load_registry_reports_the_path_and_diagnostic_for_a_malformed_base_file()
    -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join(REGISTRY_FILE);
        fs::write(&path, "not valid toml =")?;

        let err = load_registry(dir.path(), None).expect_err("malformed base file");
        match err {
            RegistryError::Parse { path: err_path, .. } => assert_eq!(path, err_path),
            RegistryError::Io { .. } => panic!("expected a parse error"),
        }
        Ok(())
    }

    #[test]
    fn load_registry_reports_a_malformed_overlay_file() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        fs::write(dir.path().join(OVERLAY_FILE), "not valid toml =")?;

        let err = load_registry(dir.path(), None).expect_err("malformed overlay file");
        assert!(matches!(err, RegistryError::Parse { .. }));
        Ok(())
    }

    #[test]
    fn validate_reports_no_issues_for_a_clean_registry() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        fs::write(
            dir.path().join(REGISTRY_FILE),
            "[project.kb]\nremotes = [\"github.com/tftio/kb\"]\npaths = [\"/repos/kb\"]\n",
        )?;

        let registry = load_registry(dir.path(), None).expect("valid registry");
        assert_eq!(Vec::<ValidationIssue>::new(), registry.validate());
        Ok(())
    }

    #[test]
    fn validate_reports_an_invalid_slug() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        fs::write(dir.path().join(REGISTRY_FILE), "[project.\"Not Valid\"]\n")?;

        let registry = load_registry(dir.path(), None).expect("valid registry");
        let issues = registry.validate();
        assert!(matches!(
            issues.as_slice(),
            [ValidationIssue::InvalidSlug { .. }]
        ));
        Ok(())
    }

    #[test]
    fn validate_reports_an_unnormalizable_remote() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        fs::write(
            dir.path().join(REGISTRY_FILE),
            "[project.kb]\nremotes = [\"\"]\n",
        )?;

        let registry = load_registry(dir.path(), None).expect("valid registry");
        let issues = registry.validate();
        assert!(matches!(
            issues.as_slice(),
            [ValidationIssue::RemoteNotNormalizable { .. }]
        ));
        Ok(())
    }

    #[test]
    fn validate_reports_a_non_normalized_remote() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        fs::write(
            dir.path().join(REGISTRY_FILE),
            "[project.kb]\nremotes = [\"https://github.com/tftio/kb.git\"]\n",
        )?;

        let registry = load_registry(dir.path(), None).expect("valid registry");
        let issues = registry.validate();
        assert!(matches!(
            issues.as_slice(),
            [ValidationIssue::RemoteNotNormalized { .. }]
        ));
        Ok(())
    }

    #[test]
    fn validate_reports_a_remote_registered_under_two_slugs() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        fs::write(
            dir.path().join(REGISTRY_FILE),
            "[project.kb]\nremotes = [\"github.com/tftio/kb\"]\n\n[project.kb-fork]\nremotes = [\"github.com/tftio/kb\"]\n",
        )?;

        let registry = load_registry(dir.path(), None).expect("valid registry");
        let issues = registry.validate();
        assert!(
            issues
                .iter()
                .any(|issue| matches!(issue, ValidationIssue::DuplicateRemote { .. }))
        );
        Ok(())
    }

    #[test]
    fn validate_reports_a_path_registered_under_two_slugs() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        fs::write(
            dir.path().join(REGISTRY_FILE),
            "[project.kb]\npaths = [\"/repos/kb\"]\n\n[project.kb-fork]\npaths = [\"/repos/kb\"]\n",
        )?;

        let registry = load_registry(dir.path(), None).expect("valid registry");
        let issues = registry.validate();
        assert!(
            issues
                .iter()
                .any(|issue| matches!(issue, ValidationIssue::DuplicatePath { .. }))
        );
        Ok(())
    }

    #[test]
    fn validate_reports_a_relative_path() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        fs::write(
            dir.path().join(REGISTRY_FILE),
            "[project.kb]\npaths = [\"repos/kb\"]\n",
        )?;

        let registry = load_registry(dir.path(), None).expect("valid registry");
        let issues = registry.validate();
        assert!(matches!(
            issues.as_slice(),
            [ValidationIssue::RelativePath { .. }]
        ));
        Ok(())
    }

    #[test]
    fn projects_iterates_every_slug() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        fs::write(
            dir.path().join(REGISTRY_FILE),
            "[project.kb]\n[project.mnene]\n",
        )?;

        let registry = load_registry(dir.path(), None).expect("valid registry");
        let slugs: Vec<&str> = registry.projects().map(|(slug, _)| slug).collect();
        assert_eq!(vec!["kb", "mnene"], slugs);
        Ok(())
    }
}
