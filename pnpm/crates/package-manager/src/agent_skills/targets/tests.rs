use super::agent_skills_dir_from_lookup;
use pretty_assertions::assert_eq;
use std::{ffi::OsString, fs};

#[test]
fn detects_claude_code() {
    assert_eq!(
        agent_skills_dir_from_lookup(|var| (var == "CLAUDECODE").then(|| OsString::from("1"))),
        Some(".claude/skills"),
    );
    assert_eq!(
        agent_skills_dir_from_lookup(|var| (var == "CLAUDE_CODE").then(|| OsString::from("1"))),
        Some(".claude/skills"),
    );
}

#[test]
fn detects_cursor() {
    assert_eq!(
        agent_skills_dir_from_lookup(|var| (var == "CURSOR_AGENT").then(|| OsString::from("1"))),
        Some(".cursor/skills"),
    );
}

#[test]
fn detects_gemini_cli() {
    assert_eq!(
        agent_skills_dir_from_lookup(|var| (var == "GEMINI_CLI").then(|| OsString::from("1"))),
        Some(".gemini/skills"),
    );
}

#[test]
fn detects_antigravity() {
    assert_eq!(
        agent_skills_dir_from_lookup(|var| {
            (var == "ANTIGRAVITY_AGENT").then(|| OsString::from("1"))
        }),
        Some(".agents/skills"),
    );
}

#[test]
fn detects_copilot() {
    assert_eq!(
        agent_skills_dir_from_lookup(|var| (var == "COPILOT_AGENT").then(|| OsString::from("1"))),
        Some(".github/skills"),
    );
    assert_eq!(
        agent_skills_dir_from_lookup(|var| (var == "COPILOT_CLI").then(|| OsString::from("1"))),
        Some(".github/skills"),
    );
}

#[test]
fn detects_codex() {
    assert_eq!(
        agent_skills_dir_from_lookup(|var| {
            (var == "CODEX_THREAD_ID").then(|| OsString::from("xyz"))
        }),
        Some(".agents/skills"),
    );
    assert_eq!(
        agent_skills_dir_from_lookup(|var| (var == "CODEX_SANDBOX").then(|| OsString::from("1"))),
        Some(".agents/skills"),
    );
}

#[test]
fn detects_generic_ai_agent() {
    assert_eq!(
        agent_skills_dir_from_lookup(|var| (var == "AI_AGENT").then(|| OsString::from("1"))),
        Some(".agents/skills"),
    );
}

#[test]
fn ignores_empty_environment_variables() {
    assert_eq!(agent_skills_dir_from_lookup(|var| (var == "CLAUDECODE").then(OsString::new)), None);
}

#[test]
fn prefers_specific_agent_over_generic_fallback() {
    assert_eq!(
        agent_skills_dir_from_lookup(|var| {
            match var {
                "AI_AGENT" | "CURSOR_AGENT" => Some(OsString::from("1")),
                _ => None,
            }
        }),
        Some(".cursor/skills"),
    );
}

#[test]
fn returns_none_when_no_agent_variable_is_set() {
    assert_eq!(agent_skills_dir_from_lookup(|_| None), None);
}

#[test]
fn rejects_target_escaping_workspace_through_symlink() {
    let workspace = tempfile::tempdir().expect("create workspace tempdir");
    let external = tempfile::tempdir().expect("create external tempdir");

    pnpm_fs::symlink_dir(external.path(), &workspace.path().join(".agents"))
        .expect("symlink .agents to external directory");

    let targets = super::target_dirs(
        &pnpm_config::Config::default(),
        workspace.path(),
        Some(".agents/skills"),
    )
    .expect("resolve targets");

    assert_eq!(targets, Vec::<std::path::PathBuf>::new());
    assert!(!external.path().join("skills").exists());
}

#[test]
fn rejects_configured_skills_dir_escaping_workspace() {
    let workspace = tempfile::tempdir().expect("create workspace tempdir");
    let config = pnpm_config::Config {
        skills_dirs: Some(vec!["../outside/skills".to_string()]),
        ..pnpm_config::Config::default()
    };

    let targets = super::target_dirs(&config, workspace.path(), None).expect("resolve targets");
    assert_eq!(targets, Vec::<std::path::PathBuf>::new());
}

#[test]
fn creates_and_returns_safe_target_inside_workspace() {
    let workspace = tempfile::tempdir().expect("create workspace tempdir");
    let expected = workspace.path().join(".agents/skills");

    let targets = super::target_dirs(
        &pnpm_config::Config::default(),
        workspace.path(),
        Some(".agents/skills"),
    )
    .expect("resolve targets");

    assert_eq!(targets, vec![expected.clone()]);
    assert!(expected.is_dir());
}

#[test]
fn targets_changed_ignores_escaping_target() {
    let workspace = tempfile::tempdir().expect("create workspace tempdir");
    let external = tempfile::tempdir().expect("create external tempdir");

    pnpm_fs::symlink_dir(external.path(), &workspace.path().join(".agents"))
        .expect("symlink .agents to external directory");

    assert!(!super::targets_changed(
        &pnpm_config::Config::default(),
        workspace.path(),
        Some(".agents/skills"),
        &[],
    ));
}

#[test]
fn accepts_target_symlinked_inside_workspace() {
    let workspace = tempfile::tempdir().expect("create workspace tempdir");
    let internal = workspace.path().join("internal_agents");
    fs::create_dir(&internal).expect("create internal dir");

    pnpm_fs::symlink_dir(&internal, &workspace.path().join(".agents"))
        .expect("symlink .agents to internal directory");

    let targets = super::target_dirs(
        &pnpm_config::Config::default(),
        workspace.path(),
        Some(".agents/skills"),
    )
    .expect("resolve targets");

    assert_eq!(targets, vec![workspace.path().join(".agents/skills")]);
    assert!(internal.join("skills").is_dir());
}
