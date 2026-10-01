//! Pure project resolution, and the convenience I/O assembly around it.
//!
//! [`resolve`] is a pure function: given what a caller already knows (a
//! declared slug, a directory, a normalized remote, and a loaded
//! [`Registry`]), it applies the fleet's one derivation order and returns a
//! [`Resolution`] or `None`. It performs no I/O itself.
//!
//! [`discover_inputs`] is the convenience most consumers actually want: it
//! performs the filesystem walk ([`project_root`]), reads a `.clanker`
//! declaration ([`read_declared_project`]), and reads the origin remote
//! ([`read_origin_remote`]) for a directory, returning the
//! [`ResolutionInputs`] that [`resolve`] takes. Consumers that act once per
//! session may cache a [`Resolution`]; consumers that act per command (such
//! as `mnene`) call [`discover_inputs`] and [`resolve`] every time, since
//! neither is a cache of the other.

use std::path::Path;

use super::declare::{DeclareError, ProjectRoot, project_root, read_declared_project};
use super::git::{OriginRemoteError, read_origin_remote};
use super::registry::Registry;
use super::remote::NormalizedRemote;
use super::slug::Slug;

/// Where a [`Resolution`]'s slug came from.
///
/// Ordered as the derivation itself is ordered: a declaration wins over a
/// registered path, which wins over a registered remote, which wins over a
/// slug derived from an unregistered remote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A `.clanker` at the project root declared the slug.
    Declared,
    /// A registry `paths` entry that is the working directory or an
    /// ancestor of it named the slug.
    Path,
    /// The working directory's origin remote was found among a project's
    /// registry `remotes`.
    Remote,
    /// No declaration or registry entry matched; the slug was derived from
    /// the working directory's origin remote.
    Derived,
}

impl Source {
    /// The stable, lowercase label this source is reported and stored
    /// under.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Declared => "declared",
            Self::Path => "path",
            Self::Remote => "remote",
            Self::Derived => "derived",
        }
    }
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// The result of resolving a directory to a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// The resolved project slug.
    pub slug: Slug,
    /// How the slug was resolved.
    pub source: Source,
    /// The directory's normalized origin remote, when one exists,
    /// regardless of `source`.
    pub remote: Option<NormalizedRemote>,
}

/// Resolve `dir` to a project.
///
/// Applies the fleet's one derivation order: a `.clanker` declaration at
/// the project root; then a registry `paths` entry that is `dir` or an
/// ancestor of it (the most specific match wins, compared lexically after
/// `~` expansion — the caller should pass a canonicalized `dir` for this
/// comparison to be meaningful); then the directory's origin remote looked
/// up among registry `remotes`; then a slug derived from an unregistered
/// remote; then `None`.
///
/// This function performs no I/O: `declared`, `dir`, `remote`, and
/// `registry` are exactly what the caller already knows.
/// [`discover_inputs`] assembles `declared` and `remote` for a directory
/// when a caller wants the filesystem walk done for it.
#[must_use]
pub fn resolve(
    declared: Option<&Slug>,
    dir: &Path,
    remote: Option<&NormalizedRemote>,
    registry: &Registry,
) -> Option<Resolution> {
    if let Some(slug) = declared {
        return Some(Resolution {
            slug: slug.clone(),
            source: Source::Declared,
            remote: remote.cloned(),
        });
    }

    if let Some(slug) = best_path_match(dir, registry) {
        return Some(Resolution {
            slug,
            source: Source::Path,
            remote: remote.cloned(),
        });
    }

    let remote = remote?;

    if let Some(slug) = registry_slug_for_remote(remote, registry) {
        return Some(Resolution {
            slug,
            source: Source::Remote,
            remote: Some(remote.clone()),
        });
    }

    derive_slug(remote).map(|slug| Resolution {
        slug,
        source: Source::Derived,
        remote: Some(remote.clone()),
    })
}

/// Find the most specific registry `paths` entry that is `dir` or an
/// ancestor of it, and return the slug it is registered under.
///
/// "Most specific" is the entry with the most path components; a tie is
/// broken by slug ordering, which is deterministic but otherwise
/// unspecified.
fn best_path_match(dir: &Path, registry: &Registry) -> Option<Slug> {
    let mut best: Option<(usize, Slug)> = None;
    for (slug, project) in registry.projects() {
        for path in &project.paths {
            if dir != path.as_path() && !dir.starts_with(path) {
                continue;
            }
            let Ok(candidate) = Slug::new(slug) else {
                continue;
            };
            let depth = path.components().count();
            let replace = best
                .as_ref()
                .is_none_or(|(best_depth, _)| depth > *best_depth);
            if replace {
                best = Some((depth, candidate));
            }
        }
    }
    best.map(|(_, slug)| slug)
}

/// Find a project registered under `remote` in `registry`.
fn registry_slug_for_remote(remote: &NormalizedRemote, registry: &Registry) -> Option<Slug> {
    registry
        .projects()
        .find(|(_, project)| project.remotes.contains(remote.as_str()))
        .and_then(|(slug, _)| Slug::new(slug).ok())
}

/// Derive a slug from an unregistered remote's last path segment:
/// lowercased, every run of non-alphanumeric characters collapsed to one
/// hyphen, leading and trailing hyphens trimmed, truncated to
/// [`super::slug::SLUG_MAX_BYTES`] bytes. Returns `None` when that leaves
/// nothing.
fn derive_slug(remote: &NormalizedRemote) -> Option<Slug> {
    let last_segment = remote.as_str().rsplit('/').next()?;

    let mut collapsed = String::new();
    let mut last_was_separator = true; // suppresses a leading hyphen
    for ch in last_segment.chars() {
        if ch.is_ascii_alphanumeric() {
            collapsed.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if !last_was_separator {
            collapsed.push('-');
            last_was_separator = true;
        }
    }
    while collapsed.ends_with('-') {
        collapsed.pop();
    }
    while collapsed.len() > super::slug::SLUG_MAX_BYTES {
        collapsed.pop();
    }
    let trimmed = collapsed.trim_end_matches('-');
    if trimmed.is_empty() {
        return None;
    }

    Slug::new(trimmed).ok()
}

/// What [`discover_inputs`] found for a directory: exactly what
/// [`resolve`] needs beyond the registry.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolutionInputs {
    /// The directory's declared project, if `.clanker` at its project root
    /// declares one.
    pub declared: Option<Slug>,
    /// The directory's normalized origin remote, if it is inside a
    /// repository with one.
    pub remote: Option<NormalizedRemote>,
}

/// A failure while assembling [`ResolutionInputs`] for a directory.
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    /// The project root's `.clanker` declaration could not be read.
    #[error(transparent)]
    Declare(#[from] DeclareError),
    /// The repository's origin remote could not be read.
    #[error(transparent)]
    Remote(#[from] OriginRemoteError),
}

/// Assemble the [`ResolutionInputs`] for `dir`.
///
/// Finds its project root, reads any `.clanker` declaration there, and
/// (inside a repository) reads its origin remote. This is the I/O
/// consumers no longer need to re-implement themselves; [`resolve`] itself
/// stays pure.
///
/// # Errors
///
/// Returns [`DiscoveryError`] when a `.clanker` declaration or an origin
/// remote is present but cannot be read or parsed.
pub fn discover_inputs(dir: &Path) -> Result<ResolutionInputs, DiscoveryError> {
    let root = project_root(dir);

    let declared = match &root {
        Some(root) => read_declared_project(&root.path)?,
        None => None,
    };

    let remote = match &root {
        Some(ProjectRoot {
            common_dir: Some(common_dir),
            ..
        }) => read_origin_remote(common_dir)?,
        _ => None,
    };

    Ok(ResolutionInputs { declared, remote })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn registry_from_toml(contents: &str) -> Registry {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("projects.toml"), contents).expect("write registry");
        super::super::registry::load_registry(dir.path(), None).expect("valid registry")
    }

    fn remote(text: &str) -> NormalizedRemote {
        super::super::remote::normalize_remote(text).expect("normalizable remote")
    }

    #[test]
    fn declared_wins_over_both_a_registered_path_and_a_registered_remote() {
        let registry = registry_from_toml(
            "[project.by-path]\npaths = [\"/repos/work\"]\n\n[project.by-remote]\nremotes = [\"github.com/tftio/kb\"]\n",
        );
        let declared = Slug::new("declared-project").expect("valid slug");
        let dir = PathBuf::from("/repos/work");
        let origin = remote("git@github.com:tftio/kb.git");

        let resolution =
            resolve(Some(&declared), &dir, Some(&origin), &registry).expect("resolution");
        assert_eq!(declared, resolution.slug);
        assert_eq!(Source::Declared, resolution.source);
        assert_eq!(Some(origin), resolution.remote);
    }

    #[test]
    fn a_registered_path_wins_over_a_disagreeing_registered_remote() {
        let registry = registry_from_toml(
            "[project.by-path]\npaths = [\"/repos/work\"]\n\n[project.by-remote]\nremotes = [\"github.com/tftio/kb\"]\n",
        );
        let dir = PathBuf::from("/repos/work");
        let origin = remote("git@github.com:tftio/kb.git");

        let resolution = resolve(None, &dir, Some(&origin), &registry).expect("resolution");
        assert_eq!("by-path", resolution.slug.as_str());
        assert_eq!(Source::Path, resolution.source);
        assert_eq!(Some(origin), resolution.remote);
    }

    #[test]
    fn a_path_match_from_an_ancestor_directory_still_wins() {
        let registry = registry_from_toml("[project.kb]\npaths = [\"/repos/kb\"]\n");
        let dir = PathBuf::from("/repos/kb/src/inner");

        let resolution = resolve(None, &dir, None, &registry).expect("resolution");
        assert_eq!("kb", resolution.slug.as_str());
        assert_eq!(Source::Path, resolution.source);
    }

    #[test]
    fn the_most_specific_path_match_wins() {
        let registry = registry_from_toml(
            "[project.outer]\npaths = [\"/repos\"]\n\n[project.inner]\npaths = [\"/repos/kb\"]\n",
        );
        let dir = PathBuf::from("/repos/kb/src");

        let resolution = resolve(None, &dir, None, &registry).expect("resolution");
        assert_eq!("inner", resolution.slug.as_str());
    }

    #[test]
    fn a_registered_remote_resolves_when_no_path_matches() {
        let registry = registry_from_toml("[project.kb]\nremotes = [\"github.com/tftio/kb\"]\n");
        let dir = PathBuf::from("/elsewhere");
        let origin = remote("https://github.com/tftio/kb");

        let resolution = resolve(None, &dir, Some(&origin), &registry).expect("resolution");
        assert_eq!("kb", resolution.slug.as_str());
        assert_eq!(Source::Remote, resolution.source);
    }

    #[test]
    fn an_unregistered_remote_derives_a_slug() {
        let registry = Registry::default();
        let dir = PathBuf::from("/elsewhere");
        let origin = remote("https://github.com/tftio/My_New.Repo");

        let resolution = resolve(None, &dir, Some(&origin), &registry).expect("resolution");
        assert_eq!("my-new-repo", resolution.slug.as_str());
        assert_eq!(Source::Derived, resolution.source);
    }

    #[test]
    fn nothing_resolves_with_no_declaration_no_path_and_no_remote() {
        let registry = Registry::default();
        let dir = PathBuf::from("/elsewhere");

        assert_eq!(None, resolve(None, &dir, None, &registry));
    }

    #[test]
    fn a_derived_slug_is_none_when_the_remote_path_has_no_alphanumerics() {
        let registry = Registry::default();
        let dir = PathBuf::from("/elsewhere");
        let origin = remote("https://example.com/---");

        assert_eq!(None, resolve(None, &dir, Some(&origin), &registry));
    }

    #[test]
    fn source_labels_are_stable_and_lowercase() {
        assert_eq!("declared", Source::Declared.label());
        assert_eq!("path", Source::Path.label());
        assert_eq!("remote", Source::Remote.label());
        assert_eq!("derived", Source::Derived.label());
        assert_eq!("declared", Source::Declared.to_string());
    }

    #[test]
    fn discover_inputs_finds_no_declaration_or_remote_outside_a_repository()
    -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        let inputs = discover_inputs(dir.path()).expect("no repository");
        assert_eq!(ResolutionInputs::default(), inputs);
        Ok(())
    }

    #[test]
    fn discover_inputs_reads_a_declaration_outside_a_repository() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        fs::write(dir.path().join(".clanker"), "project = \"notes\"\n")?;

        let inputs = discover_inputs(dir.path()).expect("declaration present");
        assert_eq!(
            Some(Slug::new("notes").expect("valid slug")),
            inputs.declared
        );
        assert_eq!(None, inputs.remote);
        Ok(())
    }

    #[test]
    fn discover_inputs_reads_the_origin_remote_inside_a_repository() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        fs::create_dir_all(dir.path().join(".git"))?;
        fs::write(
            dir.path().join(".git").join("HEAD"),
            "ref: refs/heads/main\n",
        )?;
        fs::write(
            dir.path().join(".git").join("config"),
            "[remote \"origin\"]\n\turl = git@github.com:tftio/kb.git\n",
        )?;

        let inputs = discover_inputs(dir.path()).expect("repository present");
        assert_eq!(None, inputs.declared);
        assert_eq!(Some(remote("git@github.com:tftio/kb.git")), inputs.remote);
        Ok(())
    }

    #[test]
    fn discover_inputs_prefers_the_worktree_declaration_over_none() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        fs::create_dir_all(dir.path().join(".git"))?;
        fs::write(
            dir.path().join(".git").join("HEAD"),
            "ref: refs/heads/main\n",
        )?;
        fs::write(dir.path().join(".clanker"), "project = \"kb\"\n")?;

        let inputs = discover_inputs(dir.path()).expect("repository present");
        assert_eq!(Some(Slug::new("kb").expect("valid slug")), inputs.declared);
        Ok(())
    }
}
