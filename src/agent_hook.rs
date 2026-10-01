//! Emission of declared lifecycle hooks for supported agent harnesses.
//!
//! Mirrors [`crate::agent_skill`]'s skill emission: a tool declares
//! [`crate::AgentHook`]s on its [`crate::AgentSurfaceSpec`], and `meta agent
//! emit-hooks` writes each hook's embedded script to disk and prints the
//! JSON registration fragment for the target harness. Like `emit-skills`,
//! this is a human-initiated, dev-time action; an installer merges the
//! printed fragment into the harness's own settings file rather than
//! shipping its own copy of the script, so the script a tool tests is the
//! script that gets installed.
//!
//! # Fragment shape
//!
//! Both targets print a document shaped like the harness's own settings
//! file, so the installer can `jq` it in directly rather than translate it:
//!
//! ```json
//! {
//!   "hooks": {
//!     "SessionEnd": [
//!       {
//!         "hooks": [
//!           {
//!             "type": "command",
//!             "command": "/abs/path/to/out/session-end-kb.sh",
//!             "timeout": 30,
//!             "statusMessage": "Distilling session into kb"
//!           }
//!         ]
//!       }
//!     ]
//!   }
//! }
//! ```
//!
//! This is the shape Claude Code reads from its `settings.json`, where an
//! installer can append it with
//! `.hooks.<EVENT> = ((.hooks.<EVENT> // []) + [{"hooks": [...]}])`, and the
//! shape Codex reads from `~/.codex/hooks.json`. Codex's shape is identical
//! except that Codex has no `SessionEnd`-equivalent lifecycle event: only
//! `Stop` is supported for Codex. A hook declared against an event with no Codex equivalent is
//! therefore left out of the Codex fragment (and its script is not written)
//! rather than invented a key Codex does not read; see [`event_key`].
//!
//! A tool with no hooks applicable to the requested target prints `{}` and
//! writes nothing, exiting `0`.
//!
//! # Stdout contract
//!
//! Stdout carries exactly one thing: the registration fragment described
//! above, printed once as a single JSON document (`{}` when no hook is
//! applicable to the target). Nothing else is ever written to stdout, so a
//! caller can pipe `meta agent emit-hooks` straight into `jq` without
//! filtering. Progress lines (`wrote <path>` for each script written) go to
//! stderr instead.

use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::Path,
};

use serde_json::{Map, Value, json};

use crate::{AgentHook, AgentSurfaceSpec, EmitTarget, HookEvent, ToolSpec};

fn hooks_for(spec: &ToolSpec) -> &'static [AgentHook] {
    spec.agent_surface.map_or(&[][..], AgentSurfaceSpec::hooks)
}

/// The settings key a hook's event maps to for a target harness, or `None`
/// when the harness has no equivalent lifecycle event to register against.
///
/// Exhaustive over [`HookEvent`] with no catch-all: adding a lifecycle event
/// forces a decision here for every existing target.
#[must_use]
pub const fn event_key(target: EmitTarget, event: HookEvent) -> Option<&'static str> {
    match event {
        HookEvent::SessionEnd => match target {
            EmitTarget::Claude => Some("SessionEnd"),
            // Codex's hooks.json has no SessionEnd-equivalent lifecycle
            // event; only Stop is supported for Codex. Inventing a key here
            // would register a hook Codex never reads.
            EmitTarget::Codex => None,
        },
        // Both current targets register Stop under the same key. A future
        // `EmitTarget` variant is still forced through this match (there is
        // no wildcard arm), so it must decide its own Stop key -- or lack of
        // one -- rather than silently inheriting "Stop".
        HookEvent::Stop => match target {
            EmitTarget::Claude | EmitTarget::Codex => Some("Stop"),
        },
    }
}

/// The file a hook's script is written to, relative to the `--out` directory.
#[must_use]
pub fn hook_file_name(hook: &AgentHook) -> String {
    format!("{}.sh", hook.name)
}

/// Render the `meta agent emit-hooks` registration fragment for `hooks`
/// against `target`, given the directory their scripts are (or will be)
/// written under.
///
/// Pure and I/O-free, so it can be asserted on directly: it computes each
/// hook's would-be path from `out_dir` and [`hook_file_name`] without
/// touching the filesystem. Hooks whose event has no equivalent on `target`
/// are left out of the fragment (see [`event_key`]). An empty result (no
/// hooks, or none applicable to `target`) renders as `{}`.
#[must_use]
pub fn render_hooks_fragment(hooks: &[AgentHook], target: EmitTarget, out_dir: &Path) -> Value {
    let mut by_event: BTreeMap<&'static str, Vec<Value>> = BTreeMap::new();

    for hook in hooks {
        let Some(key) = event_key(target, hook.event) else {
            continue;
        };
        let path = out_dir.join(hook_file_name(hook));
        let entry = json!({
            "hooks": [{
                "type": "command",
                "command": path.display().to_string(),
                "timeout": hook.timeout,
                "statusMessage": hook.status_message,
            }]
        });
        by_event.entry(key).or_default().push(entry);
    }

    if by_event.is_empty() {
        return json!({});
    }

    let mut hooks_obj = Map::new();
    for (key, entries) in by_event {
        hooks_obj.insert(key.to_string(), Value::Array(entries));
    }
    json!({ "hooks": Value::Object(hooks_obj) })
}

/// Write `script` to `path` and mark it executable (`0o755`).
fn write_hook_script(path: &Path, script: &str) -> io::Result<()> {
    fs::write(path, script)?;
    make_executable(path)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Run `meta agent emit-hooks --target <target> --out <out>` against the
/// process's real stdout and stderr.
///
/// A thin wrapper around [`run_emit_hooks_to`]; see it for the stdout/stderr
/// contract.
pub(crate) fn run_emit_hooks(spec: &ToolSpec, target: EmitTarget, out: &Path) -> i32 {
    run_emit_hooks_to(spec, target, out, &mut io::stdout(), &mut io::stderr())
}

/// Writes each declared hook whose event has a `target` equivalent to
/// `<out>/<name>.sh`, marks it executable, and prints the registration
/// fragment (see [`render_hooks_fragment`]) as a single JSON document on
/// `stdout`. A tool with no hooks applicable to `target` writes nothing and
/// prints `{}`.
///
/// `stdout` carries the fragment and nothing else, so a caller can pipe the
/// command straight into `jq`. Progress (`wrote <path>` for each script
/// written) goes to `stderr` instead. Progress and diagnostics on `stderr`
/// are best-effort. The fragment is this command's result, so failing to
/// write it to `stdout` is reported on `stderr` and exits 1.
pub(crate) fn run_emit_hooks_to(
    spec: &ToolSpec,
    target: EmitTarget,
    out: &Path,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> i32 {
    let hooks = hooks_for(spec);
    let fragment = render_hooks_fragment(hooks, target, out);

    let applicable = hooks
        .iter()
        .filter(|hook| event_key(target, hook.event).is_some());

    if fragment == json!({}) {
        return emit_fragment(&fragment, stdout, stderr);
    }

    if let Err(err) = fs::create_dir_all(out) {
        let _ = writeln!(stderr, "failed to create {}: {err}", out.display());
        return 1;
    }

    for hook in applicable {
        let path = out.join(hook_file_name(hook));
        if let Err(err) = write_hook_script(&path, hook.script) {
            let _ = writeln!(stderr, "failed to write {}: {err}", path.display());
            return 1;
        }
        let _ = writeln!(stderr, "wrote {}", path.display());
    }

    emit_fragment(&fragment, stdout, stderr)
}

/// Writes `fragment` to `stdout` as one line and returns the exit code: 0
/// when it was written, 1 (with a diagnostic on `stderr`) when it was not.
fn emit_fragment(
    fragment: &serde_json::Value,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> i32 {
    match writeln!(stdout, "{fragment}").and_then(|()| stdout.flush()) {
        Ok(()) => 0,
        Err(err) => {
            let _ = writeln!(
                stderr,
                "failed to write the hook registration fragment: {err}"
            );
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{AgentSurfaceSpec, LicenseType, RepoInfo, ToolSpec};

    const SESSION_END_HOOK: AgentHook = AgentHook::new(
        "session-end-kb",
        HookEvent::SessionEnd,
        "#!/usr/bin/env bash\necho captured\n",
        30,
    )
    .with_status_message("Distilling session into kb");

    const STOP_HOOK: AgentHook = AgentHook::new(
        "plan-validate",
        HookEvent::Stop,
        "#!/usr/bin/env bash\nexit 0\n",
        60,
    )
    .with_status_message("Validating planning documents");

    const HOOKS_SURFACE: AgentSurfaceSpec =
        AgentSurfaceSpec::new(&[]).with_hooks(&[SESSION_END_HOOK, STOP_HOOK]);
    const NO_HOOKS_SURFACE: AgentSurfaceSpec = AgentSurfaceSpec::new(&[]);
    const SESSION_END_ONLY_SURFACE: AgentSurfaceSpec =
        AgentSurfaceSpec::new(&[]).with_hooks(&[SESSION_END_HOOK]);

    fn spec_with(surface: &'static AgentSurfaceSpec) -> ToolSpec {
        ToolSpec::new(
            "kb",
            "kb",
            "1.2.3",
            LicenseType::MIT,
            RepoInfo::new("owner", "repo"),
            true,
            false,
        )
        .with_agent_surface(surface)
    }

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir().join(format!(
            "tftio-lib-agent-hook-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        fs::remove_dir_all(&base).ok();
        fs::create_dir_all(&base).expect("create tempdir");
        base
    }

    #[test]
    fn event_key_maps_session_end_and_stop_exhaustively() {
        assert_eq!(
            event_key(EmitTarget::Claude, HookEvent::SessionEnd),
            Some("SessionEnd")
        );
        assert_eq!(event_key(EmitTarget::Codex, HookEvent::SessionEnd), None);
        assert_eq!(event_key(EmitTarget::Claude, HookEvent::Stop), Some("Stop"));
        assert_eq!(event_key(EmitTarget::Codex, HookEvent::Stop), Some("Stop"));
    }

    #[test]
    fn hook_file_name_appends_sh_extension() {
        assert_eq!(hook_file_name(&SESSION_END_HOOK), "session-end-kb.sh");
    }

    #[test]
    fn render_hooks_fragment_for_claude_includes_path_and_status_message() {
        let out_dir = Path::new("/tmp/out");
        let fragment = render_hooks_fragment(&[SESSION_END_HOOK], EmitTarget::Claude, out_dir);

        assert_eq!(
            fragment,
            json!({
                "hooks": {
                    "SessionEnd": [{
                        "hooks": [{
                            "type": "command",
                            "command": "/tmp/out/session-end-kb.sh",
                            "timeout": 30,
                            "statusMessage": "Distilling session into kb",
                        }]
                    }]
                }
            })
        );
    }

    #[test]
    fn render_hooks_fragment_for_codex_omits_session_end_but_keeps_stop() {
        let out_dir = Path::new("/tmp/out");
        let fragment =
            render_hooks_fragment(&[SESSION_END_HOOK, STOP_HOOK], EmitTarget::Codex, out_dir);

        assert_eq!(
            fragment,
            json!({
                "hooks": {
                    "Stop": [{
                        "hooks": [{
                            "type": "command",
                            "command": "/tmp/out/plan-validate.sh",
                            "timeout": 60,
                            "statusMessage": "Validating planning documents",
                        }]
                    }]
                }
            })
        );
    }

    #[test]
    fn render_hooks_fragment_is_empty_object_with_no_hooks() {
        let fragment = render_hooks_fragment(&[], EmitTarget::Claude, Path::new("/tmp/out"));
        assert_eq!(fragment, json!({}));
    }

    #[test]
    fn run_emit_hooks_no_hooks_writes_nothing_and_returns_zero() {
        let dir = tempdir();
        let exit = run_emit_hooks(&spec_with(&NO_HOOKS_SURFACE), EmitTarget::Claude, &dir);
        assert_eq!(exit, 0);
        assert!(
            fs::read_dir(&dir).expect("read tempdir").next().is_none(),
            "no files should be written"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_emit_hooks_codex_target_with_session_end_only_writes_nothing() {
        let dir = tempdir();
        let exit = run_emit_hooks(
            &spec_with(&SESSION_END_ONLY_SURFACE),
            EmitTarget::Codex,
            &dir,
        );
        assert_eq!(exit, 0);
        assert!(
            fs::read_dir(&dir).expect("read tempdir").next().is_none(),
            "codex has no SessionEnd equivalent, so nothing should be written"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_emit_hooks_writes_executable_byte_identical_script() {
        let dir = tempdir();
        let exit = run_emit_hooks(&spec_with(&HOOKS_SURFACE), EmitTarget::Claude, &dir);
        assert_eq!(exit, 0);

        let path = dir.join("session-end-kb.sh");
        let contents = fs::read_to_string(&path).expect("hook script should exist");
        assert_eq!(contents, SESSION_END_HOOK.script());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = fs::metadata(&path)
                .expect("stat hook script")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o755);
        }

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_emit_hooks_to_stdout_carries_only_the_registration_fragment() {
        let dir = tempdir();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit = run_emit_hooks_to(
            &spec_with(&HOOKS_SURFACE),
            EmitTarget::Claude,
            &dir,
            &mut stdout,
            &mut stderr,
        );
        assert_eq!(exit, 0);

        let stdout_text = String::from_utf8(stdout).expect("stdout should be utf8");
        // Stdout must parse as exactly one JSON document: no leading or
        // trailing bytes, no other document concatenated onto it, so a
        // caller can pipe the command straight into `jq`.
        let mut deserializer =
            serde_json::Deserializer::from_str(&stdout_text).into_iter::<Value>();
        let fragment = deserializer
            .next()
            .expect("stdout should contain a JSON document")
            .expect("stdout should parse as JSON");
        assert!(
            deserializer.next().is_none(),
            "stdout should contain exactly one JSON document, got trailing content: {stdout_text:?}"
        );
        assert_eq!(
            fragment,
            render_hooks_fragment(&[SESSION_END_HOOK, STOP_HOOK], EmitTarget::Claude, &dir)
        );

        let stderr_text = String::from_utf8(stderr).expect("stderr should be utf8");
        assert!(
            !stdout_text.contains("wrote "),
            "progress lines must not appear on stdout: {stdout_text:?}"
        );
        assert!(
            stderr_text.contains("wrote "),
            "progress lines should appear on stderr: {stderr_text:?}"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_emit_hooks_to_stdout_is_bare_empty_object_when_no_hooks_apply() {
        let dir = tempdir();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit = run_emit_hooks_to(
            &spec_with(&NO_HOOKS_SURFACE),
            EmitTarget::Claude,
            &dir,
            &mut stdout,
            &mut stderr,
        );
        assert_eq!(exit, 0);

        let stdout_text = String::from_utf8(stdout).expect("stdout should be utf8");
        let fragment: Value = serde_json::from_str(stdout_text.trim()).expect("valid JSON");
        assert_eq!(fragment, json!({}));
        assert!(
            String::from_utf8(stderr).expect("stderr utf8").is_empty(),
            "no progress line is expected when nothing is written"
        );

        fs::remove_dir_all(&dir).ok();
    }

    /// A `stdout` that refuses every write, as a closed pipe does.
    struct ClosedPipe;

    impl Write for ClosedPipe {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn run_emit_hooks_to_fails_when_the_fragment_cannot_be_written() {
        let dir = tempdir();
        let mut stderr = Vec::new();

        let exit = run_emit_hooks_to(
            &spec_with(&HOOKS_SURFACE),
            EmitTarget::Claude,
            &dir,
            &mut ClosedPipe,
            &mut stderr,
        );

        assert_eq!(exit, 1);
        let stderr_text = String::from_utf8(stderr).expect("stderr utf8");
        assert!(
            stderr_text.contains("failed to write the hook registration fragment"),
            "stderr should name the failure: {stderr_text:?}"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_emit_hooks_reports_dir_creation_failure() {
        let base = tempdir();
        let file_path = base.join("blocking-file");
        fs::write(&file_path, b"not a dir").expect("write blocking file");
        let blocking_out = file_path.join("hooks");

        let exit = run_emit_hooks(
            &spec_with(&HOOKS_SURFACE),
            EmitTarget::Claude,
            &blocking_out,
        );
        assert_eq!(exit, 1);

        fs::remove_dir_all(&base).ok();
    }
}
