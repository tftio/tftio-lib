# tftio-lib

Shared CLI and agent-mode plumbing library for tftio Rust tools

## Getting started

Toolchain, task execution, and hook tools are managed by mise.
The Rust toolchain is declared in `mise.toml`; rustup is an implementation
detail and `rust-toolchain.toml` is intentionally absent.
Entering the directory does not prepare, install, or regenerate anything: setup
is explicit, so no lockfile ever changes because someone walked into the
repository.

```sh
mise trust --quiet
mise install
mise run setup:idea   # optional; regenerates the gitignored .idea/
```

Tools come from `mise activate <shell>` in an interactive shell, and from mise
shims for non-interactive processes such as editors and coding agents.

Dependencies move on one deliberate command, and never on their own:

```sh
mise run update       # mise tools, cargo crates, prek hooks
mise run check:locks  # read-only; fails if Cargo.lock is stale
```

## Tasks

```sh
mise run check  # check-only hooks, as CI runs them
mise run lint   # manual autofix hooks
mise run test   # test suite
mise run ci     # full CI gate
```

The generated Rust gate includes formatting, TOML formatting, shell linting,
spelling, clippy, nextest, docs, unused-dependency detection, advisory audit,
license/source policy, packaging, and at least 95% line coverage.

## Modules

### `project`: shared project identity

`tftio_lib::project` is the fleet's one implementation of project identity: a
slug, chosen once, held in a registry, and derived from where work is
happening. Every consumer (`clanker`, `kb`, `mnene`, `planner`,
`silent-critic`) depends on this module rather than re-implementing any part
of it. Nothing in the module reads `std::env` or the current directory;
every value a function needs arrives as an argument.

**File locations.** The registry is a file the operator edits, normally at
`${XDG_CONFIG_HOME:-~/.config}/tftio/projects.toml`, with a machine-local
overlay `projects.local.toml` beside it for paths that exist on one machine
only. `project::default_registry_dir` computes that directory from an
explicit `xdg_config_home` and `home`; `project::load_registry` loads and
merges both files. A repository may also declare its own project at
`<repository root>/.clanker` under a `project` key.

**Schema.**

```toml
# projects.toml / projects.local.toml
[project.kb]
remotes = ["github.com/tftio/kb"]
paths = ["~/Documents/Repositories/kb/main"]

[project.notes]
paths = ["~/Documents/notes"]
```

```toml
# <repository root>/.clanker
project = "kb"
```

The base file and its overlay merge per slug: the union of `remotes` and of
`paths`, deduplicated. A missing base file is an empty registry; a missing
overlay is fine; a malformed file is an error carrying its path and the TOML
diagnostic. `Registry::validate` reports a bad slug, an unnormalizable or
non-normalized remote, a remote or a path registered under two slugs, and a
relative path.

**Derivation order.** `project::resolve` applies, in order: a `.clanker`
declaration at the project root; a registry `paths` entry that is the
working directory or an ancestor of it (the most specific match wins); the
working directory's origin remote looked up among registry `remotes`; a
slug derived from an unregistered remote's last path segment; otherwise no
project. The `Source` reported with a resolution (`declared`, `path`,
`remote`, or `derived`) names which tier resolved it, and is stored beside
the slug wherever this fleet persists one.

**The root-anchored declaration rule.** Inside a git repository, the only
file ever consulted for a declaration is `<repository top level>/.clanker`
— the repository's own root for a linked worktree, never the directory its
worktrees share, and never any ancestor above it or subdirectory below it.
Outside a repository, the root is the nearest ancestor whose `.clanker`
declares a `project` key, found the same way git finds `.git`; nothing
above that root is read. An umbrella directory's declaration is therefore
never read for a repository nested beneath it, and a subdirectory of a
repository can never declare a project other than its root's.

**Remote normalization.** `project::normalize_remote` reduces every spelling
of a remote URL this fleet is likely to see — scp-like shorthand, `ssh://`,
`https://`, `http://`, `git://`, `file://`, and a bare path — to one
canonical `<host>/<path>` form, so `git@github.com:tftio/kb.git`,
`ssh://git@github.com/tftio/kb`, and `https://github.com/tftio/kb` all
normalize to `github.com/tftio/kb`. Scheme, user (including any embedded
credential), and port are stripped, the host is lowercased, and the path's
case is preserved; no credential ever appears in a returned value or an
error. A `file://` URL or an absolute/explicitly-relative filesystem path
normalizes under the pseudo-host `local`, documented in `project::remote`.

**Purity.** `project::resolve` is a pure function of its arguments and
performs no I/O. `project::discover_inputs` is the convenience most callers
want: it walks the filesystem for a directory's project root, declaration,
and origin remote, and returns exactly what `resolve` needs. A consumer
that acts once per session (a launch hook) may cache a resolution; a
consumer that acts per command (`mnene`) calls `discover_inputs` and
`resolve` every time.

### `agent`: capability and hook surface

A tool declares its agent-mode surface once, as a `const AgentSurfaceSpec`
attached to its `ToolSpec`: named capabilities (`AgentCapability`, filtered
into the CLI under agent-mode supervision and emitted as `SKILL.md`
artifacts by `meta agent emit-skills`) and, alongside them, lifecycle hooks
(`AgentHook`, emitted by `meta agent emit-hooks`). Both are carried by the
tool's own binary and generated at install time: an installer invokes the
emitter rather than shipping its own copy of a skill or a hook script, so the
artifact a tool tests is the artifact that gets installed.

An `AgentHook` names a harness lifecycle event (`HookEvent::SessionEnd`,
`HookEvent::Stop`, …), an embedded script (typically `include_str!`'d so the
file a tool tests is the file its binary emits), a timeout, and an optional
status message:

```rust
const SESSION_END_HOOK: AgentHook = AgentHook::new(
    "session-end-kb",
    HookEvent::SessionEnd,
    include_str!("../scripts/session-end-kb.sh"),
    30,
)
.with_status_message("Distilling session into kb");

const AGENT_SURFACE: AgentSurfaceSpec =
    AgentSurfaceSpec::new(&CAPABILITIES).with_hooks(&[SESSION_END_HOOK]);
```

`<tool> meta agent emit-hooks --target <claude|codex> --out <dir>` writes
each applicable hook's script to `<dir>/<name>.sh` (mode `0o755`) and prints
the target harness's registration fragment as JSON on stdout — a document
shaped like the slice of the harness's own settings file that registers
hooks, so installation is a `jq` splice rather than a translation:

```json
{
  "hooks": {
    "SessionEnd": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "/dir/session-end-kb.sh",
            "timeout": 30,
            "statusMessage": "Distilling session into kb"
          }
        ]
      }
    ]
  }
}
```

Codex's `~/.codex/hooks.json` takes the identical shape under the same
`"hooks"` key, but Codex has no `SessionEnd`-equivalent lifecycle event;
only `Stop` is supported for Codex. A hook declared against an event with no Codex equivalent
is therefore left out of the Codex fragment entirely (its script is not
written, either) rather than invented a key Codex does not read. A tool
with no hooks applicable to the requested target writes nothing and prints
`{}`, exiting `0`. `agent_hook::event_key` is the exhaustive, no-catch-all
mapping this behavior comes from; adding a `HookEvent` variant is a compile
error in that function until every target's key (or lack of one) is decided.

**Stdout contract.** `emit-hooks`'s stdout carries exactly one thing: the
registration fragment, printed once as a single JSON document. Nothing else
is ever written there, so `<tool> meta agent emit-hooks ... | jq .` works
without filtering. Progress (`wrote <path>` for each script written) goes to
stderr instead. This differs from `emit-skills`, whose stdout carries only
`wrote <path>` lines and no machine-readable output — `emit-skills` prints
no document for a caller to parse, so its output has no such contract to
keep and is unchanged by this.
