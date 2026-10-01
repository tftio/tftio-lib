# Repository invariants

These invariants are binding for humans and coding agents working in this repository.
Bypasses must be explicit: use `INVARIANT-BYPASS(<ID>): <reason>` in the smallest
possible change and explain why the invariant cannot hold.

## Engineering invariants

| ID | Invariant | Enforcement |
|---|---|---|
| ENG-001 | Keep changes single-purpose and use Conventional Commits for committed work. | Review + `cog verify` |
| ENG-002 | Never commit secrets, credentials, or tokens; inject configuration from the environment rather than hardcoding it. | Review + secret scanning |
| ENG-003 | Every behavior change ships with tests that exercise real systems, assert outcomes rather than implementation, and cover error paths. Do not skip, xfail, or delete tests to make checks pass. | Tests + review |
| ENG-004 | Fail loudly with actionable context at the source; do not add silent `except` blocks or swallowed `Result` values. | Review |
| ENG-005 | Distinguish recoverable domain errors (return typed error values) from invariant violations (assert and crash with diagnostics). | Review |
| ENG-006 | Parse external data into typed containers at the system boundary; validate once on the way in and trust the types inside. | Review |
| ENG-007 | Keep retained state immutable; store snapshots in fields, module- and class-level bindings, closures, and caches, never aliased mutable objects. | Review |
| ENG-008 | Keep business logic in pure, deterministic functions; confine I/O, logging, and state mutation to a thin imperative shell. | Review |
| ENG-009 | Model data so illegal states are unrepresentable; dispatch over closed sum types exhaustively with no silent catch-all. | Review |
| ENG-010 | Wrap third-party dependencies behind domain-specific interfaces rather than threading their APIs through the codebase. | Review |
| ENG-011 | Fit the repository's established conventions; do not rewrite working code solely to change its library, tooling, or style. | Review |
| ENG-012 | Keep formatting, linting, typing, and tests clean before handing off work. | `mise run check` |
| ENG-013 | `mise.toml` plus `mise.lock` are the repository-owned tool declarations; hooks and CI must invoke `mise run` tasks rather than reimplementing checks. | Review + CI |

## Rust invariants

| ID | Invariant | Enforcement |
|---|---|---|
| RS-001 | Use Rust 2024 with the exact Rust toolchain declared in `mise.toml`; do not add `rust-toolchain.toml` as a second source of truth. | Review + `mise run check` |
| RS-002 | Commit `Cargo.lock` for every Rust repository, including libraries, so local and CI dependency resolution use the same artifact. | Review + CI |
| RS-003 | Deny unsafe code, missing docs, clippy warnings, unwrap/expect/panic/todo/unimplemented/dbg, wildcard imports, enum glob imports, and unchecked indexing. | `mise run check:clippy` |
| RS-004 | Keep command entry points thin; put behavior in the library crate and cover it with tests. | Tests + review |
| RS-005 | Model recoverable failures as typed error enums with `thiserror`; use `anyhow` only at binary or integration boundaries. | Review |
| RS-006 | Validate dependency advisories, licenses, duplicate dependency shape, unused dependencies, documentation, packaging, and spelling before handoff. | `mise run check` |
| RS-007 | Maintain at least 95% line coverage. This repository-local floor (the generated default is 100%) accommodates residual paths that no genuine behavior-bearing test can reach in-process: real-TTY color/spinner branches, `std::process::exit`-diverging entry points, OS-level `readdir`/`file_type`/canonicalize error closures, non-UTF-8 path names, `#[non_exhaustive]` catch-all arms, and infallible-serialize arms. Lowering the floor further requires an explicit invariant bypass. | `mise run check:coverage` |
