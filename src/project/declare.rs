//! Root-anchored discovery of a declared project.
//!
//! [`project_root`] finds the one directory whose `.clanker` may declare a
//! project: inside a git repository that is the repository's top level (a
//! linked worktree's own root, never the common directory shared with its
//! siblings); outside any repository it is the nearest ancestor whose
//! `.clanker` declares a `project` key, found by the same upward search git
//! performs for `.git`. No ancestor above the root is ever read, and inside
//! a repository no ancestor above the repository's own top level is
//! examined either — an umbrella directory's `.clanker` is never read for a
//! repository nested beneath it, and a subdirectory of a repository cannot
//! declare a different project than its root.

use std::fs;
use std::path::{Path, PathBuf};

use super::git::{find_git_entry, resolve_common_dir, resolve_head_dir};
use super::slug::{Slug, SlugError};

/// The file, relative to a project root, whose `project` key may declare
/// the root's project.
const DECLARATION_FILE: &str = ".clanker";

/// The root directory at which a project may be declared, plus (inside a
/// repository) the common git directory shared by every worktree of that
/// repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRoot {
    /// The root directory whose `.clanker` is the only file consulted for a
    /// declaration. Inside a repository this is the repository's top level
    /// (a linked worktree's own root); outside one it is the nearest
    /// ancestor whose `.clanker` declares a project.
    pub path: PathBuf,
    /// The common git directory shared by every worktree of the
    /// repository, when `path` was found by locating a repository. `None`
    /// when `path` was found by the outside-a-repository ancestor search.
    pub common_dir: Option<PathBuf>,
}

/// Find the root directory at which `dir`'s project may be declared.
///
/// Returns `None` when `dir` is inside a `.git` entry that could not be
/// resolved to a git directory (a malformed linked-worktree pointer, for
/// example) and no repository root can be trusted, and also when no
/// ancestor of `dir` up to the filesystem root declares a project.
#[must_use]
pub fn project_root(dir: &Path) -> Option<ProjectRoot> {
    if let Some((toplevel, git_path)) = find_git_entry(dir) {
        let head_dir = resolve_head_dir(&toplevel, &git_path)?;
        let common_dir = resolve_common_dir(&head_dir);
        return Some(ProjectRoot {
            path: toplevel,
            common_dir: Some(common_dir),
        });
    }

    dir.ancestors()
        .find(|ancestor| matches!(read_declared_project(ancestor), Ok(Some(_))))
        .map(|path| ProjectRoot {
            path: path.to_path_buf(),
            common_dir: None,
        })
}

/// A failure reading or interpreting a root's `.clanker` declaration.
#[derive(Debug, thiserror::Error)]
pub enum DeclareError {
    /// The `.clanker` file exists but could not be read.
    #[error("cannot read {path}: {source}")]
    Io {
        /// The `.clanker` file that could not be read.
        path: PathBuf,
        /// The underlying I/O failure.
        source: std::io::Error,
    },
    /// The `.clanker` file exists but is not valid TOML.
    #[error("cannot parse {path}: {source}")]
    Parse {
        /// The `.clanker` file that failed to parse.
        path: PathBuf,
        /// The TOML diagnostic.
        source: toml::de::Error,
    },
    /// The `.clanker` file's `project` key is not a valid slug.
    #[error("{path}: invalid project key: {source}")]
    InvalidSlug {
        /// The `.clanker` file carrying the invalid key.
        path: PathBuf,
        /// Why the key is not a valid slug.
        source: SlugError,
    },
}

/// Parse `<root>/.clanker` as a generic TOML table and return its
/// `project` key, validated as a [`Slug`].
///
/// Every key other than `project` is ignored, not rejected: this function
/// exists so that a per-command consumer such as `mnene` can honour the
/// declaration without depending on `clanker`'s own strict configuration
/// type. No file other than `<root>/.clanker` is ever read.
///
/// # Errors
///
/// Returns [`DeclareError::Io`] when `.clanker` exists but cannot be read,
/// [`DeclareError::Parse`] when it is not valid TOML, and
/// [`DeclareError::InvalidSlug`] when its `project` key is present but is
/// not a string matching the slug grammar.
pub fn read_declared_project(root: &Path) -> Result<Option<Slug>, DeclareError> {
    let path = root.join(DECLARATION_FILE);
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(DeclareError::Io { path, source }),
    };

    let table: toml::Table = toml::from_str(&contents).map_err(|source| DeclareError::Parse {
        path: path.clone(),
        source,
    })?;

    let Some(value) = table.get("project") else {
        return Ok(None);
    };
    let Some(raw) = value.as_str() else {
        return Err(DeclareError::InvalidSlug {
            path,
            source: SlugError::InvalidGrammar {
                raw: value.to_string(),
                reason: "the project key must be a string",
            },
        });
    };

    Slug::new(raw)
        .map(Some)
        .map_err(|source| DeclareError::InvalidSlug { path, source })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn init_repo(repo_dir: &Path) -> Result<(), std::io::Error> {
        fs::create_dir_all(repo_dir.join(".git"))?;
        fs::write(repo_dir.join(".git").join("HEAD"), "ref: refs/heads/main\n")
    }

    #[test]
    fn read_declared_project_returns_none_with_no_file() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        assert_eq!(None, read_declared_project(dir.path()).expect("no file"));
        Ok(())
    }

    #[test]
    fn read_declared_project_returns_none_without_a_project_key() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        fs::write(dir.path().join(".clanker"), "context = \"work\"\n")?;
        assert_eq!(
            None,
            read_declared_project(dir.path()).expect("no project key")
        );
        Ok(())
    }

    #[test]
    fn read_declared_project_reads_the_project_key_and_ignores_others() -> Result<(), std::io::Error>
    {
        let dir = tempfile::tempdir()?;
        fs::write(
            dir.path().join(".clanker"),
            "context = \"work\"\nproject = \"kb\"\n",
        )?;
        let slug = read_declared_project(dir.path())
            .expect("valid file")
            .expect("project key present");
        assert_eq!("kb", slug.as_str());
        Ok(())
    }

    #[test]
    fn read_declared_project_rejects_malformed_toml() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        fs::write(dir.path().join(".clanker"), "not valid toml =")?;
        let err = read_declared_project(dir.path()).expect_err("malformed");
        assert!(matches!(err, DeclareError::Parse { .. }));
        Ok(())
    }

    #[test]
    fn read_declared_project_rejects_a_non_string_project_key() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        fs::write(dir.path().join(".clanker"), "project = 5\n")?;
        let err = read_declared_project(dir.path()).expect_err("not a string");
        assert!(matches!(err, DeclareError::InvalidSlug { .. }));
        Ok(())
    }

    #[test]
    fn read_declared_project_rejects_an_invalid_slug() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        fs::write(dir.path().join(".clanker"), "project = \"Not Valid\"\n")?;
        let err = read_declared_project(dir.path()).expect_err("invalid grammar");
        assert!(matches!(err, DeclareError::InvalidSlug { .. }));
        Ok(())
    }

    #[test]
    fn project_root_is_the_repository_toplevel_from_a_nested_directory()
    -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        init_repo(dir.path())?;
        let nested = dir.path().join("src").join("inner");
        fs::create_dir_all(&nested)?;

        let root = project_root(&nested).expect("repository found");
        assert_eq!(dir.path(), root.path);
        assert_eq!(Some(dir.path().join(".git")), root.common_dir);
        Ok(())
    }

    #[test]
    fn project_root_is_none_with_no_repository_and_no_declaration() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        assert_eq!(None, project_root(dir.path()));
        Ok(())
    }

    #[test]
    fn project_root_is_none_when_the_git_file_has_no_gitdir_line() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        fs::write(dir.path().join(".git"), "not a gitdir pointer\n")?;
        assert_eq!(None, project_root(dir.path()));
        Ok(())
    }

    #[test]
    fn project_root_outside_a_repository_finds_the_nearest_declaring_ancestor()
    -> Result<(), std::io::Error> {
        let umbrella = tempfile::tempdir()?;
        fs::write(umbrella.path().join(".clanker"), "project = \"notes\"\n")?;
        let nested = umbrella.path().join("drafts").join("2026");
        fs::create_dir_all(&nested)?;

        let root = project_root(&nested).expect("declaring ancestor found");
        assert_eq!(umbrella.path(), root.path);
        assert_eq!(None, root.common_dir);
        Ok(())
    }

    #[test]
    fn project_root_inside_a_repository_never_examines_the_umbrella_declaration()
    -> Result<(), std::io::Error> {
        let umbrella = tempfile::tempdir()?;
        fs::write(umbrella.path().join(".clanker"), "project = \"umbrella\"\n")?;
        let repo_dir = umbrella.path().join("kb");
        init_repo(&repo_dir)?;

        let root = project_root(&repo_dir).expect("repository found");
        assert_eq!(repo_dir, root.path);
        assert_ne!(umbrella.path(), root.path);
        Ok(())
    }

    #[test]
    fn project_root_inside_a_repository_never_examines_a_subdirectory_declaration()
    -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        init_repo(dir.path())?;
        let nested = dir.path().join("packages").join("sub");
        fs::create_dir_all(&nested)?;
        fs::write(nested.join(".clanker"), "project = \"sub-project\"\n")?;

        let root = project_root(&nested).expect("repository found");
        assert_eq!(dir.path(), root.path);

        let declared = read_declared_project(&root.path).expect("root file readable");
        assert_eq!(None, declared);
        Ok(())
    }
}
