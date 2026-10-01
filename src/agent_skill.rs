//! Ungated agent-skill artifact generation.
//!
//! This surface emits descriptors for the tool's declared agent capabilities
//! in formats consumable by LLM coding agents (`Claude Code`, `OpenAI Codex CLI`).
//! Unlike the redaction-oriented surface in [`crate::agent`], this surface is
//! always available — installing skill artifacts is a human-initiated action,
//! not a supervised agent inspection path.

use std::{
    fmt::Write as _,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

// clap::Subcommand used via crate::agent types
use serde_json::{Value, json};

use crate::agent::{AgentSubcommand, DescribeFormat, EmitScope, EmitTarget, ListFormat};
use crate::agent_hook::run_emit_hooks;
use crate::{AgentCapability, ProcessEnv, ToolSpec};

/// Maximum length, in characters, of a `SKILL.md` frontmatter `description`.
///
/// Harnesses disagree about this. Claude Code and Codex accept any length; Pi
/// 0.87 rejects a skill whose description exceeds 1024 characters outright,
/// reporting a skill conflict and loading nothing. Emitted artifacts therefore
/// hold the description to this budget, dropping whole trailing clauses rather
/// than truncating text (see `synthesize_description`).
///
/// A crate declaring capabilities should assert this bound over its own
/// [`crate::AgentCapability`] set: this crate cannot see that text, and a
/// summary long enough to overflow on its own is the declaring crate's to
/// shorten.
pub const SKILL_DESCRIPTION_BUDGET: usize = 1024;

/// Errors that can occur when resolving the skill artifact destination directory.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum EmitDirError {
    /// User-scope resolution requires `$HOME`, which was not set at the edge.
    #[error("$HOME is not set")]
    HomeUnset,
}

/// Dispatch an [`AgentSubcommand`] for the given tool.
///
/// Refuses to run when agent mode is active (see [`crate::AgentModeContext`]). Skill
/// generation is a human-initiated, dev-time action; under supervision the
/// surface is blocked to avoid leaking declared capabilities to a worker
/// process or letting a worker write artifacts into the host's skill registry.
#[must_use]
pub fn run_agent_subcommand(spec: &ToolSpec, env: &ProcessEnv, command: &AgentSubcommand) -> i32 {
    if env.agent.active {
        eprintln!("agent skill generation is not available under agent supervision");
        return 1;
    }
    match command {
        AgentSubcommand::List { format } => {
            println!("{}", render_list(spec, *format));
            0
        }
        AgentSubcommand::Describe { name, format } => find_capability(spec, name).map_or_else(
            || {
                eprintln!("unknown agent capability: {name}");
                1
            },
            |capability| {
                println!("{}", render_describe(spec, capability, *format));
                0
            },
        ),
        AgentSubcommand::EmitSkills {
            target,
            scope,
            out,
            install,
        } => run_emit_skills(
            spec,
            env.home.as_deref(),
            *target,
            *scope,
            out.as_deref(),
            *install,
        ),
        AgentSubcommand::EmitHooks { target, out } => run_emit_hooks(spec, *target, out),
    }
}

fn run_emit_skills(
    spec: &ToolSpec,
    home: Option<&Path>,
    target: EmitTarget,
    scope: EmitScope,
    out: Option<&Path>,
    install: bool,
) -> i32 {
    if !install && out.is_none() {
        eprintln!(
            "agent emit-skills requires --install or --out=DIR (refusing to dump multiple artifacts to stdout)"
        );
        return 2;
    }

    let capabilities = capabilities_for(spec);
    if capabilities.is_empty() {
        eprintln!("tool declares no agent capabilities");
        return 1;
    }

    let base = match resolve_emit_dir(home, target, scope, out) {
        Ok(path) => path,
        Err(err) => {
            eprintln!("could not resolve skill destination: {err}");
            return 1;
        }
    };

    for capability in capabilities {
        let skill_name = skill_name(spec, capability);
        let dir = base.join(&skill_name);
        if let Err(err) = fs::create_dir_all(&dir) {
            eprintln!("failed to create {}: {err}", dir.display());
            return 1;
        }
        let body = render_skill_md(spec, capability);
        let path = dir.join("SKILL.md");
        if let Err(err) = write_file(&path, &body) {
            eprintln!("failed to write {}: {err}", path.display());
            return 1;
        }
        println!("wrote {}", path.display());
    }

    0
}

fn write_file(path: &Path, body: &str) -> io::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(body.as_bytes())?;
    if !body.ends_with('\n') {
        file.write_all(b"\n")?;
    }
    Ok(())
}

/// Resolve the directory under which `<skill>/SKILL.md` artifacts will be written.
///
/// `home` is the process `HOME` directory read at the binary edge (`REPO_INVARIANTS.md` ENG-008).
///
/// # Errors
///
/// Returns [`EmitDirError::HomeUnset`] when the user-scope path requires `$HOME`
/// and it is unset.
pub fn resolve_emit_dir(
    home: Option<&Path>,
    target: EmitTarget,
    scope: EmitScope,
    out: Option<&Path>,
) -> Result<PathBuf, EmitDirError> {
    if let Some(path) = out {
        return Ok(path.to_path_buf());
    }
    let segment = match target {
        EmitTarget::Claude => ".claude",
        EmitTarget::Codex => ".codex",
    };
    match scope {
        EmitScope::Project => Ok(PathBuf::from(segment).join("skills")),
        EmitScope::User => {
            let home = home.ok_or(EmitDirError::HomeUnset)?;
            Ok(home.join(segment).join("skills"))
        }
    }
}

fn capabilities_for(spec: &ToolSpec) -> &'static [AgentCapability] {
    spec.agent_surface
        .map_or(&[][..], |surface| surface.capabilities)
}

fn find_capability(spec: &ToolSpec, name: &str) -> Option<&'static AgentCapability> {
    capabilities_for(spec)
        .iter()
        .find(|capability| capability.name == name)
}

/// Construct the canonical skill artifact name for a capability.
#[must_use]
pub fn skill_name(spec: &ToolSpec, capability: &AgentCapability) -> String {
    format!("{}-{}", spec.bin_name, capability.name)
}

/// Render the `agent list` output.
#[must_use]
pub fn render_list(spec: &ToolSpec, format: ListFormat) -> String {
    let capabilities = capabilities_for(spec);
    match format {
        ListFormat::Text => render_list_text(spec, capabilities),
        ListFormat::Json => render_list_json(spec, capabilities).to_string(),
    }
}

fn render_list_text(spec: &ToolSpec, capabilities: &[AgentCapability]) -> String {
    let mut lines = vec![format!("tool: {}", spec.bin_name)];
    if capabilities.is_empty() {
        lines.push(String::from("capabilities: none declared"));
    } else {
        lines.push(String::from("capabilities:"));
        for capability in capabilities {
            lines.push(format!(
                "- {}: {}",
                capability.name,
                summary_or_default(capability),
            ));
        }
    }
    lines.join("\n")
}

fn render_list_json(spec: &ToolSpec, capabilities: &[AgentCapability]) -> Value {
    let entries = capabilities
        .iter()
        .map(|capability| {
            json!({
                "name": capability.name,
                "summary": summary_or_default(capability),
                "skill_name": skill_name(spec, capability),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "tool": spec.bin_name,
        "version": spec.version,
        "capabilities": entries,
    })
}

/// Render `agent describe` for a single capability.
#[must_use]
pub fn render_describe(
    spec: &ToolSpec,
    capability: &AgentCapability,
    format: DescribeFormat,
) -> String {
    match format {
        DescribeFormat::Text => render_describe_text(spec, capability),
        DescribeFormat::Json => render_describe_json(spec, capability).to_string(),
        DescribeFormat::SkillMd => render_skill_md(spec, capability),
    }
}

fn render_describe_text(spec: &ToolSpec, capability: &AgentCapability) -> String {
    let mut sections = vec![
        format!("tool: {}", spec.bin_name),
        format!("capability: {}", capability.name),
        format!("summary: {}", summary_or_default(capability)),
    ];
    if let Some(text) = capability.when_to_use {
        sections.push(format!("when-to-use: {text}"));
    }
    if let Some(text) = capability.when_not_to_use {
        sections.push(format!("when-not-to-use: {text}"));
    }
    sections.push(format!("commands:\n{}", render_command_lines(capability)));
    sections.push(format!("flags:\n{}", render_flag_lines(capability)));
    sections.push(format!("examples:\n{}", render_example_lines(capability)));
    sections.push(format!("output: {}", output_or_default(capability)));
    sections.push(format!(
        "constraints: {}",
        constraints_or_default(capability)
    ));
    sections.join("\n")
}

fn render_describe_json(spec: &ToolSpec, capability: &AgentCapability) -> Value {
    json!({
        "tool": spec.bin_name,
        "version": spec.version,
        "capability": capability.name,
        "skill_name": skill_name(spec, capability),
        "summary": summary_or_default(capability),
        "when_to_use": capability.when_to_use,
        "when_not_to_use": capability.when_not_to_use,
        "commands": capability
            .commands
            .iter()
            .map(|selector| selector.path.join(" "))
            .collect::<Vec<_>>(),
        "flags": capability
            .flags
            .iter()
            .map(|flag| {
                if flag.command_path.is_empty() {
                    format!("--{}", flag.long)
                } else {
                    format!("{} --{}", flag.command_path.join(" "), flag.long)
                }
            })
            .collect::<Vec<_>>(),
        "examples": capability.examples.unwrap_or(&[]),
        "output": output_or_default(capability),
        "constraints": constraints_or_default(capability),
    })
}

/// Render a Claude/Codex-compatible `SKILL.md` body for one capability.
///
/// The artifact has YAML frontmatter (`name`, `description`) and a Markdown body
/// covering when-to-use, commands, flags, examples, output, and constraints.
#[must_use]
pub fn render_skill_md(spec: &ToolSpec, capability: &AgentCapability) -> String {
    let name = skill_name(spec, capability);
    let description = synthesize_description(capability);
    let mut body = String::new();
    body.push_str("---\n");
    let _ = writeln!(body, "name: {name}");
    let _ = writeln!(body, "description: {}", yaml_escape(&description));
    body.push_str("---\n\n");
    let _ = writeln!(body, "# {} — {}\n", spec.bin_name, capability.name);

    let _ = writeln!(body, "{}\n", summary_or_default(capability));

    if let Some(text) = capability.when_to_use {
        body.push_str("## When to use\n\n");
        body.push_str(text);
        body.push_str("\n\n");
    }
    if let Some(text) = capability.when_not_to_use {
        body.push_str("## When not to use\n\n");
        body.push_str(text);
        body.push_str("\n\n");
    }

    body.push_str("## Commands\n\n");
    if capability.commands.is_empty() {
        body.push_str("- none declared\n");
    } else {
        for selector in capability.commands {
            let _ = writeln!(body, "- `{} {}`", spec.bin_name, selector.path.join(" "));
        }
    }
    body.push('\n');

    body.push_str("## Flags\n\n");
    if capability.flags.is_empty() {
        body.push_str("- none declared\n");
    } else {
        for flag in capability.flags {
            if flag.command_path.is_empty() {
                let _ = writeln!(body, "- `--{}`", flag.long);
            } else {
                let _ = writeln!(body, "- `{} --{}`", flag.command_path.join(" "), flag.long);
            }
        }
    }
    body.push('\n');

    body.push_str("## Examples\n\n");
    match capability.examples {
        Some(examples) if !examples.is_empty() => {
            for example in examples {
                let _ = writeln!(body, "- `{example}`");
            }
        }
        _ => body.push_str("- none declared\n"),
    }
    body.push('\n');

    body.push_str("## Output\n\n");
    body.push_str(&output_or_default(capability));
    body.push_str("\n\n");

    body.push_str("## Constraints\n\n");
    body.push_str(&constraints_or_default(capability));
    body.push('\n');

    body
}

/// Build the frontmatter description within [`SKILL_DESCRIPTION_BUDGET`].
///
/// The clauses are ordered by what each is worth to a harness deciding whether
/// to load the skill: the summary, then the positive trigger, then the negative
/// one. The longest prefix of that order which fits the budget wins. Nothing is
/// cut mid-sentence and nothing is lost from the artifact -- every clause is
/// still rendered in full in the body, under `## When to use` and
/// `## When not to use`.
///
/// A summary that exceeds the budget on its own is returned unchanged rather
/// than truncated: the declaring crate owns that text, and a test over its own
/// capabilities is where that should fail. See [`SKILL_DESCRIPTION_BUDGET`].
fn synthesize_description(capability: &AgentCapability) -> String {
    let mut parts = vec![summary_or_default(capability)];

    let clauses = [
        capability
            .when_to_use
            .map(|text| format!("Use when {}", lower_first(text))),
        capability
            .when_not_to_use
            .map(|text| format!("Do not use when {}", lower_first(text))),
    ];

    for clause in clauses.into_iter().flatten() {
        let mut candidate = parts.clone();
        candidate.push(clause);
        let joined = candidate.join(". ");
        if joined.chars().count() > SKILL_DESCRIPTION_BUDGET {
            break;
        }
        parts = candidate;
    }

    parts.join(". ")
}

fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_lowercase().collect::<String>() + chars.as_str()
    })
}

fn yaml_escape(value: &str) -> String {
    let needs_quote = value.contains(':')
        || value.contains('#')
        || value.contains('\n')
        || value.starts_with(['-', '?', '!', '&', '*', '|', '>', '%', '@', '`']);
    if needs_quote {
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        let single_line = escaped.replace('\n', " ");
        format!("\"{single_line}\"")
    } else {
        value.to_string()
    }
}

fn summary_or_default(capability: &AgentCapability) -> String {
    if let Some(summary) = capability.summary {
        return String::from(summary);
    }
    if let Some(primary) = capability.commands.first() {
        return format!(
            "Use {} via {}",
            capability.name.replace('-', " "),
            primary.path.join(" ")
        );
    }
    format!("Use {}", capability.name.replace('-', " "))
}

fn output_or_default(capability: &AgentCapability) -> String {
    capability.output.map_or_else(
        || {
            capability.commands.first().map_or_else(
                || String::from("output follows the existing CLI contract"),
                |primary| {
                    format!(
                        "output follows the existing CLI contract for {}",
                        primary.path.join(" ")
                    )
                },
            )
        },
        String::from,
    )
}

fn constraints_or_default(capability: &AgentCapability) -> String {
    capability.constraints.map_or_else(
        || String::from("existing command validation and auth rules still apply"),
        String::from,
    )
}

fn render_command_lines(capability: &AgentCapability) -> String {
    if capability.commands.is_empty() {
        String::from("- none declared")
    } else {
        capability
            .commands
            .iter()
            .map(|selector| format!("- {}", selector.path.join(" ")))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn render_flag_lines(capability: &AgentCapability) -> String {
    if capability.flags.is_empty() {
        String::from("- none declared")
    } else {
        capability
            .flags
            .iter()
            .map(|flag| {
                if flag.command_path.is_empty() {
                    format!("- --{}", flag.long)
                } else {
                    format!("- {} --{}", flag.command_path.join(" "), flag.long)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn render_example_lines(capability: &AgentCapability) -> String {
    capability.examples.map_or_else(
        || String::from("- none declared"),
        |examples| {
            if examples.is_empty() {
                String::from("- none declared")
            } else {
                examples
                    .iter()
                    .map(|example| format!("- {example}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AgentCapability, AgentModeContext, AgentSurfaceSpec, CommandSelector, FlagSelector,
        LicenseType, RepoInfo, ToolSpec,
    };

    fn inactive_env() -> ProcessEnv {
        ProcessEnv {
            agent: AgentModeContext { active: false },
            home: None,
        }
    }

    fn active_env() -> ProcessEnv {
        ProcessEnv {
            agent: AgentModeContext { active: true },
            home: None,
        }
    }

    const SCAN_COMMAND: CommandSelector = CommandSelector::new(&["scan"]);
    const SCAN_LIMIT_FLAG: FlagSelector = FlagSelector::new(&["scan"], "limit");

    const SCAN_CAPABILITY: AgentCapability = AgentCapability::new(
        "scan-tree",
        "Scan a directory tree",
        &[SCAN_COMMAND],
        &[SCAN_LIMIT_FLAG],
    )
    .with_examples(&["tool scan --limit 5"])
    .with_output("plain text on stdout, exit 1 on findings")
    .with_constraints("reads only the working tree")
    .with_when_to_use("the user wants to enumerate matching files in the current tree")
    .with_when_not_to_use("the user is asking about remote or non-filesystem state");

    const AGENT_SURFACE: AgentSurfaceSpec = AgentSurfaceSpec::new(&[SCAN_CAPABILITY]);

    fn spec() -> ToolSpec {
        ToolSpec::new(
            "tool",
            "Tool",
            "1.2.3",
            LicenseType::MIT,
            RepoInfo::new("owner", "repo"),
            true,
            true,
        )
        .with_agent_surface(&AGENT_SURFACE)
    }

    #[test]
    fn list_text_includes_capability() {
        let rendered = render_list(&spec(), ListFormat::Text);
        assert!(rendered.contains("tool: tool"));
        assert!(rendered.contains("- scan-tree: Scan a directory tree"));
    }

    #[test]
    fn list_json_includes_skill_name() {
        let rendered = render_list(&spec(), ListFormat::Json);
        assert!(rendered.contains("\"skill_name\":\"tool-scan-tree\""));
        assert!(rendered.contains("\"version\":\"1.2.3\""));
    }

    #[test]
    fn describe_text_emits_when_sections() {
        let rendered = render_describe(&spec(), &SCAN_CAPABILITY, DescribeFormat::Text);
        assert!(rendered.contains("when-to-use: the user wants"));
        assert!(rendered.contains("when-not-to-use: the user is asking"));
        assert!(rendered.contains("- scan --limit"));
    }

    #[test]
    fn describe_json_round_trips_optional_fields() {
        let rendered = render_describe(&spec(), &SCAN_CAPABILITY, DescribeFormat::Json);
        let value: Value = serde_json::from_str(&rendered).expect("valid json");
        assert_eq!(value["capability"], "scan-tree");
        assert_eq!(value["skill_name"], "tool-scan-tree");
        assert_eq!(
            value["when_to_use"],
            "the user wants to enumerate matching files in the current tree"
        );
        assert_eq!(value["commands"][0], "scan");
        assert_eq!(value["flags"][0], "scan --limit");
    }

    #[test]
    fn skill_md_has_frontmatter_and_sections() {
        let rendered = render_skill_md(&spec(), &SCAN_CAPABILITY);
        assert!(rendered.starts_with("---\n"));
        assert!(rendered.contains("name: tool-scan-tree"));
        assert!(rendered.contains("description:"));
        assert!(rendered.contains("## When to use"));
        assert!(rendered.contains("## When not to use"));
        assert!(rendered.contains("## Commands"));
        assert!(rendered.contains("- `tool scan`"));
        assert!(rendered.contains("## Flags"));
        assert!(rendered.contains("- `scan --limit`"));
        assert!(rendered.contains("## Examples"));
        assert!(rendered.contains("- `tool scan --limit 5`"));
        assert!(rendered.contains("## Output"));
        assert!(rendered.contains("## Constraints"));
    }

    #[test]
    fn skill_md_quotes_description_when_colon_present() {
        const COLON_CAPABILITY: AgentCapability =
            AgentCapability::new("with-colon", "Summary: contains a colon", &[], &[]);
        const COLON_SURFACE: AgentSurfaceSpec = AgentSurfaceSpec::new(&[COLON_CAPABILITY]);
        let spec = ToolSpec::new(
            "tool",
            "Tool",
            "1.2.3",
            LicenseType::MIT,
            RepoInfo::new("owner", "repo"),
            true,
            true,
        )
        .with_agent_surface(&COLON_SURFACE);
        let rendered = render_skill_md(&spec, &COLON_CAPABILITY);
        assert!(rendered.contains("description: \"Summary: contains a colon\""));
    }

    /// Build a `'static` filler string of the requested length. `AgentCapability`
    /// borrows `'static` text, and the budget rungs need prose far longer than a
    /// readable literal, so the test leaks a generated string rather than
    /// carrying a thousand characters of filler in the source.
    fn filler(len: usize) -> &'static str {
        Box::leak(
            "detail ".repeat(len.div_ceil(7))[..len]
                .to_string()
                .into_boxed_str(),
        )
    }

    fn description_of(rendered: &str) -> String {
        let line = rendered
            .lines()
            .find(|line| line.starts_with("description: "))
            .expect("rendered skill has a description");
        line.trim_start_matches("description: ")
            .trim_matches('"')
            .to_string()
    }

    #[test]
    fn description_keeps_both_clauses_within_budget() {
        let capability = AgentCapability::new("short", "A summary", &[], &[])
            .with_when_to_use("the user wants it")
            .with_when_not_to_use("the user wants something else");
        let description = description_of(&render_skill_md(&spec(), &capability));
        assert_eq!(
            description,
            "A summary. Use when the user wants it. Do not use when the user wants something else"
        );
        assert!(description.chars().count() <= SKILL_DESCRIPTION_BUDGET);
    }

    #[test]
    fn description_drops_the_negative_clause_when_it_overflows() {
        // Summary plus trigger fits; the exclusion would carry it past the budget.
        let exclusion = filler(300);
        let capability = AgentCapability::new("wide", filler(600), &[], &[])
            .with_when_to_use(filler(300))
            .with_when_not_to_use(exclusion);
        let rendered = render_skill_md(&spec(), &capability);
        let description = description_of(&rendered);
        assert!(description.chars().count() <= SKILL_DESCRIPTION_BUDGET);
        assert!(description.contains("Use when"));
        assert!(!description.contains("Do not use when"));
        // The body keeps what the frontmatter dropped.
        assert!(rendered.contains("## When not to use"));
        assert!(rendered.contains(exclusion));
    }

    #[test]
    fn description_drops_the_positive_clause_when_the_trigger_overflows() {
        let summary = filler(900);
        let capability = AgentCapability::new("wider", summary, &[], &[])
            .with_when_to_use(filler(300))
            .with_when_not_to_use(filler(50));
        let rendered = render_skill_md(&spec(), &capability);
        let description = description_of(&rendered);
        assert_eq!(description, summary);
        // Dropping the positive clause must not let the negative one back in.
        assert!(!description.contains("Do not use when"));
        assert!(rendered.contains("## When to use"));
        assert!(rendered.contains("## When not to use"));
    }

    #[test]
    fn description_passes_through_an_over_budget_summary() {
        let summary = filler(1100);
        let capability =
            AgentCapability::new("widest", summary, &[], &[]).with_when_to_use("the user wants it");
        let description = description_of(&render_skill_md(&spec(), &capability));
        assert_eq!(description, summary);
        assert!(description.chars().count() > SKILL_DESCRIPTION_BUDGET);
    }

    #[test]
    fn run_agent_describe_unknown_returns_one() {
        let exit = run_agent_subcommand(
            &spec(),
            &inactive_env(),
            &AgentSubcommand::Describe {
                name: "nope".into(),
                format: DescribeFormat::Text,
            },
        );
        assert_eq!(exit, 1);
    }

    #[test]
    fn run_agent_subcommand_blocks_under_active_agent_mode() {
        let env = active_env();

        let exit_list = run_agent_subcommand(
            &spec(),
            &env,
            &AgentSubcommand::List {
                format: ListFormat::Text,
            },
        );
        let exit_describe = run_agent_subcommand(
            &spec(),
            &env,
            &AgentSubcommand::Describe {
                name: "scan-tree".into(),
                format: DescribeFormat::Text,
            },
        );
        let dir = tempdir();
        let exit_emit = run_agent_subcommand(
            &spec(),
            &env,
            &AgentSubcommand::EmitSkills {
                target: EmitTarget::Claude,
                scope: EmitScope::User,
                out: Some(dir.clone()),
                install: false,
            },
        );
        let wrote_file = dir.join("tool-scan-tree").join("SKILL.md").exists();
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(exit_list, 1);
        assert_eq!(exit_describe, 1);
        assert_eq!(exit_emit, 1);
        assert!(
            !wrote_file,
            "emit-skills must not write artifacts under agent mode"
        );
    }

    #[test]
    fn run_emit_skills_without_target_or_install_errors() {
        let exit = run_agent_subcommand(
            &spec(),
            &inactive_env(),
            &AgentSubcommand::EmitSkills {
                target: EmitTarget::Claude,
                scope: EmitScope::User,
                out: None,
                install: false,
            },
        );
        assert_eq!(exit, 2);
    }

    #[test]
    fn run_emit_skills_writes_skill_files_under_out_dir() {
        let dir = tempdir();
        let exit = run_agent_subcommand(
            &spec(),
            &inactive_env(),
            &AgentSubcommand::EmitSkills {
                target: EmitTarget::Claude,
                scope: EmitScope::User,
                out: Some(dir.clone()),
                install: false,
            },
        );
        assert_eq!(exit, 0);
        let path = dir.join("tool-scan-tree").join("SKILL.md");
        let contents = std::fs::read_to_string(&path).expect("skill file should exist");
        assert!(contents.contains("name: tool-scan-tree"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_emit_dir_user_scope_uses_home_for_claude() {
        let path = resolve_emit_dir(
            Some(Path::new("/tmp/fake-home")),
            EmitTarget::Claude,
            EmitScope::User,
            None,
        )
        .expect("resolves with $HOME");
        assert_eq!(path, PathBuf::from("/tmp/fake-home/.claude/skills"));
    }

    #[test]
    fn resolve_emit_dir_user_scope_uses_home_for_codex() {
        let path = resolve_emit_dir(
            Some(Path::new("/tmp/fake-home")),
            EmitTarget::Codex,
            EmitScope::User,
            None,
        )
        .expect("resolves with $HOME");
        assert_eq!(path, PathBuf::from("/tmp/fake-home/.codex/skills"));
    }

    #[test]
    fn resolve_emit_dir_user_scope_errors_without_home() {
        let err = resolve_emit_dir(None, EmitTarget::Claude, EmitScope::User, None)
            .expect_err("user scope requires $HOME");
        assert_eq!(err, EmitDirError::HomeUnset);
        assert_eq!(err.to_string(), "$HOME is not set");
    }

    #[test]
    fn resolve_emit_dir_project_scope_is_relative() {
        let path = resolve_emit_dir(None, EmitTarget::Codex, EmitScope::Project, None)
            .expect("project scope resolves without home");
        assert_eq!(path, PathBuf::from(".codex/skills"));
    }

    #[test]
    fn resolve_emit_dir_explicit_out_overrides_scope() {
        let path = resolve_emit_dir(
            None,
            EmitTarget::Claude,
            EmitScope::User,
            Some(Path::new("/tmp/explicit")),
        )
        .expect("explicit path overrides resolution");
        assert_eq!(path, PathBuf::from("/tmp/explicit"));
    }

    // A flag declared at the tool root (no command path).
    const GLOBAL_FLAG: FlagSelector = FlagSelector::new(&[], "verbose");
    const GLOBAL_CAPABILITY: AgentCapability = AgentCapability::new(
        "global-cap",
        "Global capability",
        &[SCAN_COMMAND],
        &[GLOBAL_FLAG],
    );
    const GLOBAL_SURFACE: AgentSurfaceSpec = AgentSurfaceSpec::new(&[GLOBAL_CAPABILITY]);

    // A surface that declares no capabilities at all.
    const EMPTY_SURFACE: AgentSurfaceSpec = AgentSurfaceSpec::new(&[]);

    // A capability with no summary, commands, flags, examples, output, or constraints.
    const EMPTY_CAP: AgentCapability = AgentCapability::minimal("empty-cap", &[], &[]);
    const EMPTY_CAP_SURFACE: AgentSurfaceSpec = AgentSurfaceSpec::new(&[EMPTY_CAP]);

    // A capability with commands but no summary or output, exercising synthesis.
    const CMD_ONLY_CAP: AgentCapability =
        AgentCapability::minimal("query-thing", &[SCAN_COMMAND], &[]);
    const CMD_ONLY_SURFACE: AgentSurfaceSpec = AgentSurfaceSpec::new(&[CMD_ONLY_CAP]);

    // A capability whose examples list is present but empty.
    const EMPTY_EXAMPLES_CAP: AgentCapability = AgentCapability::new(
        "with-empty-examples",
        "Has empty examples",
        &[SCAN_COMMAND],
        &[],
    )
    .with_examples(&[]);
    const EMPTY_EXAMPLES_SURFACE: AgentSurfaceSpec = AgentSurfaceSpec::new(&[EMPTY_EXAMPLES_CAP]);

    fn spec_with_surface(surface: &'static AgentSurfaceSpec) -> ToolSpec {
        ToolSpec::new(
            "tool",
            "Tool",
            "1.2.3",
            LicenseType::MIT,
            RepoInfo::new("owner", "repo"),
            true,
            true,
        )
        .with_agent_surface(surface)
    }

    fn bare_spec() -> ToolSpec {
        ToolSpec::new(
            "tool",
            "Tool",
            "1.2.3",
            LicenseType::MIT,
            RepoInfo::new("owner", "repo"),
            true,
            true,
        )
    }

    #[test]
    fn run_agent_list_inactive_returns_zero() {
        let exit = run_agent_subcommand(
            &spec(),
            &inactive_env(),
            &AgentSubcommand::List {
                format: ListFormat::Text,
            },
        );
        assert_eq!(exit, 0);
    }

    #[test]
    fn run_agent_describe_known_capability_returns_zero() {
        let exit = run_agent_subcommand(
            &spec(),
            &inactive_env(),
            &AgentSubcommand::Describe {
                name: "scan-tree".into(),
                format: DescribeFormat::Text,
            },
        );
        assert_eq!(exit, 0);
    }

    #[test]
    fn run_emit_skills_without_capabilities_returns_one() {
        let dir = tempdir();
        let exit = run_agent_subcommand(
            &spec_with_surface(&EMPTY_SURFACE),
            &inactive_env(),
            &AgentSubcommand::EmitSkills {
                target: EmitTarget::Claude,
                scope: EmitScope::User,
                out: Some(dir.clone()),
                install: false,
            },
        );
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(exit, 1);
    }

    #[test]
    fn run_emit_skills_user_scope_without_home_returns_one() {
        // inactive_env() has home: None, so user-scope resolution fails.
        let exit = run_agent_subcommand(
            &spec(),
            &inactive_env(),
            &AgentSubcommand::EmitSkills {
                target: EmitTarget::Claude,
                scope: EmitScope::User,
                out: None,
                install: true,
            },
        );
        assert_eq!(exit, 1);
    }

    #[test]
    fn run_emit_skills_reports_dir_creation_failure() {
        let base = tempdir();
        // Point --out at a regular file so create_dir_all under it fails.
        let file_path = base.join("blocking-file");
        std::fs::write(&file_path, b"not a dir").expect("write blocking file");

        let exit = run_agent_subcommand(
            &spec(),
            &inactive_env(),
            &AgentSubcommand::EmitSkills {
                target: EmitTarget::Claude,
                scope: EmitScope::User,
                out: Some(file_path),
                install: false,
            },
        );
        std::fs::remove_dir_all(&base).ok();
        assert_eq!(exit, 1);
    }

    #[test]
    fn run_emit_skills_reports_write_failure() {
        let base = tempdir();
        // Pre-create the SKILL.md path as a directory so File::create fails.
        let skill_md_as_dir = base.join("tool-scan-tree").join("SKILL.md");
        std::fs::create_dir_all(&skill_md_as_dir).expect("create SKILL.md as directory");

        let exit = run_agent_subcommand(
            &spec(),
            &inactive_env(),
            &AgentSubcommand::EmitSkills {
                target: EmitTarget::Claude,
                scope: EmitScope::User,
                out: Some(base.clone()),
                install: false,
            },
        );
        std::fs::remove_dir_all(&base).ok();
        assert_eq!(exit, 1);
    }

    #[test]
    fn write_file_appends_trailing_newline_when_missing() {
        let base = tempdir();

        let without = base.join("without-newline.txt");
        write_file(&without, "no newline").expect("write without newline");
        assert_eq!(
            std::fs::read_to_string(&without).expect("read without-newline file"),
            "no newline\n"
        );

        let with = base.join("with-newline.txt");
        write_file(&with, "has newline\n").expect("write with newline");
        assert_eq!(
            std::fs::read_to_string(&with).expect("read with-newline file"),
            "has newline\n"
        );

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn render_list_text_reports_no_capabilities_when_surface_absent() {
        let rendered = render_list(&bare_spec(), ListFormat::Text);
        assert!(rendered.contains("tool: tool"));
        assert!(rendered.contains("capabilities: none declared"));
    }

    #[test]
    fn render_describe_skill_md_matches_render_skill_md() {
        let spec = spec();
        let via_describe = render_describe(&spec, &SCAN_CAPABILITY, DescribeFormat::SkillMd);
        assert_eq!(via_describe, render_skill_md(&spec, &SCAN_CAPABILITY));
        assert!(via_describe.starts_with("---\n"));
    }

    #[test]
    fn global_flag_renders_without_command_prefix() {
        let spec = spec_with_surface(&GLOBAL_SURFACE);

        let rendered_json = render_describe(&spec, &GLOBAL_CAPABILITY, DescribeFormat::Json);
        let value: Value = serde_json::from_str(&rendered_json).expect("valid json");
        assert_eq!(value["flags"][0], "--verbose");

        let markdown = render_skill_md(&spec, &GLOBAL_CAPABILITY);
        assert!(
            markdown.contains("- `--verbose`"),
            "skill md flag: {markdown}"
        );

        let text = render_describe(&spec, &GLOBAL_CAPABILITY, DescribeFormat::Text);
        assert!(text.contains("- --verbose"), "describe text flag: {text}");
    }

    #[test]
    fn minimal_empty_capability_uses_default_sections() {
        let spec = spec_with_surface(&EMPTY_CAP_SURFACE);
        let text = render_describe(&spec, &EMPTY_CAP, DescribeFormat::Text);
        assert!(
            text.contains("summary: Use empty cap"),
            "summary default: {text}"
        );
        assert!(
            text.contains("commands:\n- none declared"),
            "commands default: {text}"
        );
        assert!(
            text.contains("flags:\n- none declared"),
            "flags default: {text}"
        );
        assert!(
            text.contains("examples:\n- none declared"),
            "examples default: {text}"
        );
        assert!(
            text.contains("output: output follows the existing CLI contract"),
            "output default: {text}"
        );
    }

    #[test]
    fn minimal_capability_with_commands_synthesizes_summary_and_output() {
        let spec = spec_with_surface(&CMD_ONLY_SURFACE);
        let text = render_describe(&spec, &CMD_ONLY_CAP, DescribeFormat::Text);
        assert!(
            text.contains("summary: Use query thing via scan"),
            "synthesized summary: {text}"
        );
        assert!(
            text.contains("output: output follows the existing CLI contract for scan"),
            "synthesized output: {text}"
        );
    }

    #[test]
    fn empty_examples_capability_renders_none_declared() {
        let spec = spec_with_surface(&EMPTY_EXAMPLES_SURFACE);
        let text = render_describe(&spec, &EMPTY_EXAMPLES_CAP, DescribeFormat::Text);
        assert!(
            text.contains("examples:\n- none declared"),
            "empty examples: {text}"
        );
    }

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir().join(format!(
            "tftio-lib-agent-skill-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        if let Err(e) = std::fs::remove_dir_all(&base) {
            eprintln!("failed to clean up tempdir {}: {e}", base.display());
        }
        std::fs::create_dir_all(&base).expect("create tempdir");
        base
    }
}
