//! Shared project identity: a slug, chosen once, held in a registry, and
//! derived from where work is happening by one function every consumer
//! calls.
//!
//! A project is a body of work, not a repository: a repository belongs to
//! at most one project, a project may span several repositories (each
//! declaring the same slug), and a project may have no repository at all
//! (a plain directory, declared by a registry path or a `.clanker`). git
//! is a place identity is declared and a source aliases come from; it is
//! never what the identity is.
//!
//! # File locations
//!
//! - The registry lives at `${XDG_CONFIG_HOME:-~/.config}/tftio/projects.toml`,
//!   with a machine-local overlay `projects.local.toml` beside it.
//!   [`default_registry_dir`] computes the directory from explicit
//!   arguments; [`load_registry`] loads and merges both files. Neither this
//!   module nor any function in it reads `std::env` or the current
//!   directory — every value arrives as an argument, so the module works
//!   identically in every consumer regardless of that consumer's own
//!   environment-reading policy.
//! - A repository may declare its own project in `<repository root>/.clanker`,
//!   under the `project` key. [`project_root`] finds the one directory
//!   whose `.clanker` is consulted, and [`read_declared_project`] reads
//!   that key.
//!
//! # Schema
//!
//! ```toml
//! # projects.toml
//! [project.kb]
//! remotes = ["github.com/tftio/kb"]
//! paths = ["~/Documents/Repositories/kb/main"]
//! ```
//!
//! ```toml
//! # <repository root>/.clanker
//! project = "kb"
//! ```
//!
//! # Derivation order
//!
//! [`resolve`] applies, in order: a `.clanker` declaration at the project
//! root; a registry `paths` entry that is the working directory or an
//! ancestor of it (most specific wins); the working directory's origin
//! remote looked up among registry `remotes`; a slug derived from an
//! unregistered remote's last path segment; otherwise no project. The
//! [`Source`] reported with a [`Resolution`] names which tier resolved it.
//!
//! # The root-anchored declaration rule
//!
//! Inside a git repository, the only file ever consulted for a declaration
//! is `<repository top level>/.clanker` — the repository's own root for a
//! linked worktree, never the directory the worktrees share, and never any
//! ancestor above it or any subdirectory below it. Outside a repository,
//! the root is the nearest ancestor whose `.clanker` declares a `project`
//! key, found the same way git finds `.git`; nothing above that root is
//! read. A declaration in an umbrella directory is therefore never read
//! for a repository nested beneath it, and a subdirectory of a repository
//! can never declare a project other than its root's.
//!
//! # I/O and purity
//!
//! [`resolve`] is a pure function of its arguments; it never touches the
//! filesystem. [`discover_inputs`] performs the filesystem walk a typical
//! caller needs and returns exactly what [`resolve`] takes, so most
//! consumers call the two in sequence rather than re-implementing the walk.
//! [`load_registry`], [`project_root`], [`read_declared_project`], and
//! [`git::read_origin_remote`] are where the module's I/O lives; nothing
//! here executes a subprocess, and no remote URL's credential ever appears
//! in a returned value or an error message.

mod declare;
mod git;
mod registry;
mod remote;
mod resolve;
mod slug;

pub use declare::{DeclareError, ProjectRoot, project_root, read_declared_project};
pub use git::{OriginRemoteError, read_origin_remote, repository_name};
pub use registry::{
    Registry, RegistryError, RegistryProject, ValidationIssue, default_registry_dir, load_registry,
};
pub use remote::{NormalizedRemote, RemoteError, normalize_remote};
pub use resolve::{DiscoveryError, Resolution, ResolutionInputs, Source, discover_inputs, resolve};
pub use slug::{SLUG_MAX_BYTES, Slug, SlugError};

#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::fs;

    /// Initialize an ordinary (non-worktree) repository at `repo_dir`.
    fn init_repo(repo_dir: &std::path::Path, origin: &str) -> Result<(), std::io::Error> {
        let git_dir = repo_dir.join(".git");
        fs::create_dir_all(&git_dir)?;
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n")?;
        fs::write(
            git_dir.join("config"),
            format!("[remote \"origin\"]\n\turl = {origin}\n"),
        )
    }

    /// Initialize a linked worktree at `worktree_dir`, sharing
    /// `gitdir_dir`'s parent-of-parent as its common directory, exactly as
    /// `git worktree add` lays one out.
    fn init_worktree(
        worktree_dir: &std::path::Path,
        gitdir_dir: &std::path::Path,
    ) -> Result<(), std::io::Error> {
        fs::create_dir_all(worktree_dir)?;
        fs::create_dir_all(gitdir_dir)?;
        let gitdir_line = format!("gitdir: {}\n", gitdir_dir.display());
        fs::write(worktree_dir.join(".git"), gitdir_line)?;
        fs::write(gitdir_dir.join("commondir"), "../..\n")?;
        fs::write(gitdir_dir.join("HEAD"), "ref: refs/heads/topic\n")
    }

    #[test]
    fn end_to_end_a_declaration_wins_over_a_disagreeing_registry_path() -> Result<(), std::io::Error>
    {
        let root = tempfile::tempdir()?;
        let repo_dir = root.path().join("kb");
        init_repo(&repo_dir, "git@github.com:tftio/kb.git")?;
        fs::write(repo_dir.join(".clanker"), "project = \"kb-declared\"\n")?;

        let registry_dir = tempfile::tempdir()?;
        fs::write(
            registry_dir.path().join("projects.toml"),
            format!(
                "[project.kb-by-path]\npaths = [\"{}\"]\n",
                repo_dir.display()
            ),
        )?;

        let registry = load_registry(registry_dir.path(), None).expect("valid registry");
        let inputs = discover_inputs(&repo_dir).expect("discovery");

        let resolution = resolve(
            inputs.declared.as_ref(),
            &repo_dir,
            inputs.remote.as_ref(),
            &registry,
        )
        .expect("resolution");
        assert_eq!("kb-declared", resolution.slug.as_str());
        assert_eq!(Source::Declared, resolution.source);
        Ok(())
    }

    #[test]
    fn end_to_end_umbrella_declaration_is_not_read_from_inside_a_repository_or_its_subdirectory()
    -> Result<(), std::io::Error> {
        let umbrella = tempfile::tempdir()?;
        fs::write(
            umbrella.path().join(".clanker"),
            "project = \"umbrella-project\"\n",
        )?;
        let repo_dir = umbrella.path().join("kb");
        init_repo(&repo_dir, "git@github.com:tftio/kb.git")?;
        let nested = repo_dir.join("src").join("inner");
        fs::create_dir_all(&nested)?;
        fs::write(nested.join(".clanker"), "project = \"nested-project\"\n")?;

        let registry = Registry::default();

        let inputs_from_repo_root = discover_inputs(&repo_dir).expect("discovery");
        assert_eq!(None, inputs_from_repo_root.declared);
        let resolution_from_root = resolve(
            inputs_from_repo_root.declared.as_ref(),
            &repo_dir,
            inputs_from_repo_root.remote.as_ref(),
            &registry,
        )
        .expect("resolution");
        assert_eq!(Source::Derived, resolution_from_root.source);
        assert_eq!("kb", resolution_from_root.slug.as_str());

        let inputs_from_nested = discover_inputs(&nested).expect("discovery");
        assert_eq!(None, inputs_from_nested.declared);

        let non_repo_dir = umbrella.path().join("notes");
        fs::create_dir_all(&non_repo_dir)?;
        let inputs_from_non_repo = discover_inputs(&non_repo_dir).expect("discovery");
        assert_eq!(
            Some(Slug::new("umbrella-project").expect("valid slug")),
            inputs_from_non_repo.declared
        );
        Ok(())
    }

    #[test]
    fn end_to_end_two_linked_worktrees_resolve_to_one_slug() -> Result<(), std::io::Error> {
        let root = tempfile::tempdir()?;
        let repo_dir = root.path().join("project");
        let worktrees = repo_dir.join(".git").join("worktrees");
        let main_worktree = repo_dir.join("main");
        let feature_worktree = repo_dir.join("feature-x");
        init_worktree(&main_worktree, &worktrees.join("main"))?;
        init_worktree(&feature_worktree, &worktrees.join("feature-x"))?;
        fs::write(
            repo_dir.join(".git").join("config"),
            "[remote \"origin\"]\n\turl = git@github.com:tftio/project.git\n",
        )?;

        let registry = Registry::default();

        let main_inputs = discover_inputs(&main_worktree).expect("discovery");
        let feature_inputs = discover_inputs(&feature_worktree).expect("discovery");

        let main_resolution = resolve(
            main_inputs.declared.as_ref(),
            &main_worktree,
            main_inputs.remote.as_ref(),
            &registry,
        )
        .expect("resolution");
        let feature_resolution = resolve(
            feature_inputs.declared.as_ref(),
            &feature_worktree,
            feature_inputs.remote.as_ref(),
            &registry,
        )
        .expect("resolution");

        assert_eq!(main_resolution.slug, feature_resolution.slug);
        assert_eq!("project", main_resolution.slug.as_str());
        Ok(())
    }
}
