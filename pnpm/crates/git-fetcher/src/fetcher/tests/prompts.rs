use super::exec_git_with;
use std::{collections::BTreeMap, env, fs};
use tempfile::tempdir;

/// The fetcher's git runs behind the live reporter, so it must never wait
/// on a terminal or ssh prompt. A `git` shim echoes the variables that
/// disable them; the expectation is derived from the process environment
/// because a user-configured ssh command is kept.
#[test]
fn git_runs_with_terminal_and_ssh_prompts_disabled() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempdir().unwrap();
    let shim = tmp.path().join("git");
    fs::write(
        &shim,
        "#!/bin/sh\nprintf '%s\\n' \"GIT_SSH_COMMAND=${GIT_SSH_COMMAND-}\" \"GIT_TERMINAL_PROMPT=${GIT_TERMINAL_PROMPT-}\"\n",
    )
    .unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();

    let stdout = exec_git_with(&shim, &["init"], None).unwrap();

    let mut expected = BTreeMap::from([
        ("GIT_SSH_COMMAND", env::var("GIT_SSH_COMMAND").unwrap_or_default()),
        ("GIT_TERMINAL_PROMPT", env::var("GIT_TERMINAL_PROMPT").unwrap_or_default()),
    ]);
    for (name, value) in pnpm_git_utils::non_interactive_git_env(|name| env::var_os(name).is_some())
    {
        expected.insert(name, value.to_owned());
    }
    let expected: Vec<String> = expected
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    assert_eq!(stdout.lines().collect::<Vec<_>>(), expected);
}
