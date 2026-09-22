use super::exec_git_with;
use std::{
    collections::BTreeMap,
    env,
    fs,
    path::Path,
};
use tempfile::tempdir;

/// A `git` shim that echoes the variables deciding whether git or ssh may
/// prompt, so a test can see the environment the fetcher hands to git.
fn write_env_echoing_git_shim(dir: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(dir).unwrap();
    let shim = dir.join("git");
    fs::write(
        &shim,
        "#!/bin/sh\nprintf '%s\\n' \"GIT_SSH_COMMAND=${GIT_SSH_COMMAND-}\" \"GIT_TERMINAL_PROMPT=${GIT_TERMINAL_PROMPT-}\"\n",
    )
    .unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();
    shim
}

/// The shim's output for the inherited environment overlaid with `vars`.
fn env_lines(vars: &[(&str, String)]) -> Vec<String> {
    let mut lines = BTreeMap::from([
        ("GIT_SSH_COMMAND", env::var("GIT_SSH_COMMAND").unwrap_or_default()),
        ("GIT_TERMINAL_PROMPT", env::var("GIT_TERMINAL_PROMPT").unwrap_or_default()),
    ]);
    for (name, value) in vars {
        lines.insert(name, value.clone());
    }
    lines
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect()
}

/// The fetcher's git runs behind the live reporter, so it must never wait
/// on a terminal or ssh prompt. The expectation is derived from the process
/// environment and the host's git configuration because a user-selected ssh
/// command is kept.
#[test]
fn git_runs_with_terminal_and_ssh_prompts_disabled() {
    let tmp = tempdir().unwrap();
    let shim = write_env_echoing_git_shim(&tmp.path().join("shim"));
    let repo = tmp.path().join("repo");
    fs::create_dir(&repo).unwrap();

    let stdout = exec_git_with(&shim, &["fetch", "origin"], Some(&repo)).unwrap();

    let vars: Vec<(&str, String)> = pnpm_git_utils::non_interactive_git_env(
        |name| env::var_os(name).is_some(),
        || pnpm_git_utils::has_configured_ssh_command::<pnpm_git_utils::Host>(Some(&repo)),
    )
    .into_iter()
    .map(|(name, value)| (name, value.to_owned()))
    .collect();
    assert_eq!(stdout.lines().collect::<Vec<_>>(), env_lines(&vars));
}

/// The ssh command is read from the configuration in effect where the
/// invocation runs: a repository that selects one keeps it.
#[test]
fn a_repository_selecting_the_ssh_command_keeps_it() {
    let tmp = tempdir().unwrap();
    let shim = write_env_echoing_git_shim(&tmp.path().join("shim"));
    let repo = tmp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    exec_git_with(Path::new("git"), &["init", "-q"], Some(&repo)).unwrap();
    exec_git_with(
        Path::new("git"),
        &["config", "core.sshCommand", "ssh -i deploy_key"],
        Some(&repo),
    )
    .unwrap();

    let stdout = exec_git_with(&shim, &["fetch", "origin"], Some(&repo)).unwrap();

    let vars = [("GIT_TERMINAL_PROMPT", "0".to_owned())];
    assert_eq!(stdout.lines().collect::<Vec<_>>(), env_lines(&vars));
}
