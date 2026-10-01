//! Git repository discovery that never shells out to `git`.
//!
//! [`find_git_entry`], [`resolve_head_dir`], [`resolve_common_dir`], and
//! [`repository_name`] are moved here from `mnene`'s `src/config.rs`
//! verbatim in behavior (mnene depends on this module rather than keeping
//! its own copy). [`read_origin_remote`] is new: a minimal, dependency-free
//! parser for the `[remote "origin"]` section of the common directory's
//! `config` file.
//!
//! Nothing here runs a subprocess or links a git library; every function is
//! a plain read of the files git itself writes.

use std::fs;
use std::path::{Path, PathBuf};

use super::remote::{NormalizedRemote, RemoteError, normalize_remote};

/// Walk up from `start_dir` and its ancestors looking for an entry named
/// `.git`, returning the directory that contains it (the worktree
/// toplevel) and the `.git` entry's own path.
///
/// The `.git` entry may be a directory (an ordinary repository) or a file
/// (`gitdir: <path>`, for a linked worktree); [`resolve_head_dir`]
/// distinguishes the two.
pub fn find_git_entry(start_dir: &Path) -> Option<(PathBuf, PathBuf)> {
    for dir in start_dir.ancestors() {
        let candidate = dir.join(".git");
        if candidate.exists() {
            return Some((dir.to_path_buf(), candidate));
        }
    }
    None
}

/// Resolve `git_path` (the `.git` entry found under `toplevel`) to this
/// worktree's own git directory, which holds `HEAD`.
///
/// When `git_path` is a directory, that directory holds `HEAD` directly.
/// When it is a file, its first line is `gitdir: <path>`, and `<path>` is
/// resolved relative to `toplevel` (the worktree root, which is `git_path`'s
/// own parent directory by construction), per the git worktree layout.
pub fn resolve_head_dir(toplevel: &Path, git_path: &Path) -> Option<PathBuf> {
    if git_path.is_dir() {
        return Some(git_path.to_path_buf());
    }

    let contents = fs::read_to_string(git_path).ok()?;
    let first_line = contents.lines().next()?;
    let raw_gitdir = first_line.strip_prefix("gitdir:")?.trim();
    Some(toplevel.join(raw_gitdir))
}

/// Resolve the common git directory shared by every worktree of one
/// repository.
///
/// A linked worktree's git directory holds a `commondir` file naming the
/// shared directory, normally as a path relative to itself (`../..`). An
/// ordinary repository has no such file and is already the common
/// directory.
pub fn resolve_common_dir(git_dir: &Path) -> PathBuf {
    let Ok(contents) = fs::read_to_string(git_dir.join("commondir")) else {
        return git_dir.to_path_buf();
    };
    let Some(target) = contents.lines().next().map(str::trim) else {
        return git_dir.to_path_buf();
    };
    normalize_lexically(&git_dir.join(target))
}

/// Resolve `.` and `..` components lexically, so a basename can be taken
/// from a path built by joining a relative `commondir` target.
///
/// This is deliberately lexical rather than [`fs::canonicalize`]: it needs
/// no filesystem access and cannot fail, and the caller only wants a name.
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    normalized
}

/// Derive the repository's name from its common git directory.
///
/// A repository whose common directory is `<repo>/.git` is named by the
/// directory holding it, which covers both an ordinary checkout and a bare
/// repository used as a worktree container. A conventional bare clone is
/// named by its own directory with the `.git` suffix removed.
#[must_use]
pub fn repository_name(common_dir: &Path) -> Option<String> {
    let name = common_dir.file_name()?;
    if name == ".git" {
        return common_dir
            .parent()?
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
    }
    Some(
        Path::new(name)
            .file_stem()
            .filter(|_| Path::new(name).extension().is_some_and(|ext| ext == "git"))
            .unwrap_or(name)
            .to_string_lossy()
            .into_owned(),
    )
}

/// A failure reading or normalizing the origin remote from a common git
/// directory's `config` file.
#[derive(Debug, thiserror::Error)]
pub enum OriginRemoteError {
    /// The `config` file could not be read for a reason other than not
    /// existing.
    #[error("cannot read {path}: {source}")]
    Io {
        /// The `config` file that could not be read.
        path: PathBuf,
        /// The underlying I/O failure.
        source: std::io::Error,
    },
    /// The `[remote "origin"]` section's `url` value could not be
    /// normalized.
    #[error("origin remote url in {path} does not normalize: {source}")]
    Normalize {
        /// The `config` file the unnormalizable URL was read from.
        path: PathBuf,
        /// Why the URL could not be normalized.
        source: RemoteError,
    },
}

/// Read and normalize the `[remote "origin"]` URL from
/// `<common_dir>/config`.
///
/// Returns `Ok(None)` when the `config` file does not exist, when it has no
/// `[remote "origin"]` section, or when that section has no `url` key. This
/// is a minimal INI parser sufficient for the section git itself writes; it
/// does not aim to parse every construct the git config format allows
/// (line continuations, `include` directives, and so on), because reading
/// one key from one section is all this module needs.
///
/// # Errors
///
/// Returns [`OriginRemoteError::Io`] when the file exists but cannot be
/// read, and [`OriginRemoteError::Normalize`] when a `url` value is found
/// but does not normalize.
pub fn read_origin_remote(
    common_dir: &Path,
) -> Result<Option<NormalizedRemote>, OriginRemoteError> {
    let path = common_dir.join("config");
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(OriginRemoteError::Io { path, source }),
    };

    let Some(url) = find_origin_url(&contents) else {
        return Ok(None);
    };
    normalize_remote(&url)
        .map(Some)
        .map_err(|source| OriginRemoteError::Normalize { path, source })
}

/// Scan `contents` (the text of a git `config` file) for the `url` value of
/// the `[remote "origin"]` section.
fn find_origin_url(contents: &str) -> Option<String> {
    let mut in_origin_section = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if let Some(header) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_origin_section = header.trim() == "remote \"origin\"";
            continue;
        }
        if !in_origin_section {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=')
            && key.trim() == "url"
        {
            return Some(value.trim().trim_matches('"').to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{find_git_entry, normalize_lexically, repository_name, resolve_common_dir};
    use std::fs;
    use std::path::{Path, PathBuf};

    /// Initialize an ordinary (non-worktree) repository at `repo_dir`.
    fn init_repo(repo_dir: &Path) -> Result<(), std::io::Error> {
        let git_dir = repo_dir.join(".git");
        fs::create_dir_all(&git_dir)?;
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n")
    }

    /// Initialize a linked worktree at `worktree_dir`: a `.git` file
    /// pointing at `gitdir_dir`, which holds its own `HEAD` and the
    /// `commondir` file git writes to name the shared git directory.
    fn init_worktree(worktree_dir: &Path, gitdir_dir: &Path) -> Result<(), std::io::Error> {
        fs::create_dir_all(worktree_dir)?;
        fs::create_dir_all(gitdir_dir)?;
        let gitdir_line = format!("gitdir: {}\n", gitdir_dir.display());
        fs::write(worktree_dir.join(".git"), gitdir_line)?;
        fs::write(gitdir_dir.join("commondir"), "../..\n")?;
        fs::write(gitdir_dir.join("HEAD"), "ref: refs/heads/topic\n")
    }

    #[test]
    fn find_git_entry_returns_none_with_no_repository() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        assert_eq!(None, find_git_entry(dir.path()));
        Ok(())
    }

    #[test]
    fn find_git_entry_finds_a_repository_from_a_nested_directory() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        init_repo(dir.path())?;
        let nested = dir.path().join("src").join("inner");
        fs::create_dir_all(&nested)?;

        let (toplevel, git_path) = find_git_entry(&nested).expect("found repository");
        assert_eq!(dir.path(), toplevel);
        assert_eq!(dir.path().join(".git"), git_path);
        Ok(())
    }

    #[test]
    fn a_repository_name_needs_a_final_path_component() {
        assert_eq!(None, repository_name(Path::new("/")));
        assert_eq!(None, repository_name(Path::new("/.git")));
        assert_eq!(
            Some("project".to_string()),
            repository_name(Path::new("/src/project/.git"))
        );
        assert_eq!(
            Some("project".to_string()),
            repository_name(Path::new("/src/project.git"))
        );
        assert_eq!(
            Some("project".to_string()),
            repository_name(Path::new("/src/project"))
        );
    }

    #[test]
    fn normalization_resolves_relative_components_lexically() {
        assert_eq!(
            PathBuf::from("/src/project/.git"),
            normalize_lexically(Path::new("/src/project/.git/worktrees/a/../../."))
        );
        assert_eq!(PathBuf::new(), normalize_lexically(Path::new(".")));
    }

    #[test]
    fn an_empty_commondir_file_leaves_the_worktree_git_directory_in_place()
    -> Result<(), std::io::Error> {
        let root = tempfile::tempdir()?;
        let gitdir_dir = root.path().join("project").join("gitdir");
        fs::create_dir_all(&gitdir_dir)?;
        fs::write(gitdir_dir.join("commondir"), "")?;

        assert_eq!(gitdir_dir, resolve_common_dir(&gitdir_dir));
        Ok(())
    }

    #[test]
    fn two_worktrees_share_a_common_directory() -> Result<(), std::io::Error> {
        let root = tempfile::tempdir()?;
        let repo_dir = root.path().join("project");
        let worktrees = repo_dir.join(".git").join("worktrees");
        let first_gitdir = worktrees.join("main");
        let second_gitdir = worktrees.join("feature-x");
        init_worktree(&repo_dir.join("main"), &first_gitdir)?;
        init_worktree(&repo_dir.join("feature-x"), &second_gitdir)?;

        assert_eq!(
            resolve_common_dir(&first_gitdir),
            resolve_common_dir(&second_gitdir)
        );
        assert_eq!(repo_dir.join(".git"), resolve_common_dir(&first_gitdir));
        Ok(())
    }

    #[test]
    fn read_origin_remote_returns_none_when_config_is_absent() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        assert_eq!(
            None,
            super::read_origin_remote(dir.path()).expect("no config file")
        );
        Ok(())
    }

    #[test]
    fn read_origin_remote_returns_none_when_no_origin_section_exists() -> Result<(), std::io::Error>
    {
        let dir = tempfile::tempdir()?;
        fs::write(
            dir.path().join("config"),
            "[core]\n\trepositoryformatversion = 0\n",
        )?;
        assert_eq!(
            None,
            super::read_origin_remote(dir.path()).expect("no origin section")
        );
        Ok(())
    }

    #[test]
    fn read_origin_remote_reads_and_normalizes_the_origin_url() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        fs::write(
            dir.path().join("config"),
            "[core]\n\trepositoryformatversion = 0\n[remote \"origin\"]\n\turl = git@github.com:tftio/kb.git\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n[branch \"main\"]\n\tremote = origin\n",
        )?;

        let remote = super::read_origin_remote(dir.path())
            .expect("origin present")
            .expect("origin url found");
        assert_eq!("github.com/tftio/kb", remote.as_str());
        Ok(())
    }

    #[test]
    fn read_origin_remote_ignores_a_non_origin_remote() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        fs::write(
            dir.path().join("config"),
            "[remote \"upstream\"]\n\turl = https://example.com/other/repo\n",
        )?;
        assert_eq!(
            None,
            super::read_origin_remote(dir.path()).expect("no origin section")
        );
        Ok(())
    }

    #[test]
    fn read_origin_remote_errors_when_the_url_does_not_normalize() -> Result<(), std::io::Error> {
        let dir = tempfile::tempdir()?;
        fs::write(dir.path().join("config"), "[remote \"origin\"]\n\turl = \n")?;
        let err = super::read_origin_remote(dir.path()).expect_err("empty url");
        assert!(matches!(err, super::OriginRemoteError::Normalize { .. }));
        Ok(())
    }
}
