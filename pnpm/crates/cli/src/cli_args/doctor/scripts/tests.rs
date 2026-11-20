use super::{
    NODE_FILE_NAME,
    UnusableNode,
    check_node_on_path,
    check_script_shell,
    find_node_entries,
    shell_settings,
};
use crate::cli_args::doctor::CheckStatus;
use pnpm_config::Config;
use pretty_assertions::assert_eq;
use std::{
    env,
    fs,
    path::Path,
};

#[cfg(unix)]
use super::shell_behind;

fn write_node(dir: &Path) {
    fs::create_dir_all(dir).expect("create PATH dir");
    let node = dir.join(NODE_FILE_NAME);
    fs::write(&node, "#!/bin/sh\n").expect("write node");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&node, fs::Permissions::from_mode(0o755)).expect("chmod node");
    }
}

#[test]
fn lists_the_node_scripts_run_first() {
    let root = tempfile::tempdir().expect("create temp dir");
    let first = root.path().join("first");
    let second = root.path().join("second");
    write_node(&first);
    write_node(&second);
    let path =
        env::join_paths([&first, &root.path().join("empty"), &first, &second]).expect("join PATH");

    let check = check_node_on_path(Some(&path));

    dbg!(&check);
    assert_eq!(check.status, CheckStatus::Pass);
    assert_eq!(
        check.detail.as_deref(),
        Some(
            format!(
                "{} (also on PATH: {})",
                first.join(NODE_FILE_NAME).display(),
                second.join(NODE_FILE_NAME).display(),
            )
            .as_str()
        ),
    );
}

#[test]
fn warns_when_no_node_is_on_path() {
    let root = tempfile::tempdir().expect("create temp dir");

    let check = check_node_on_path(Some(root.path().as_os_str()));

    dbg!(&check);
    assert_eq!(check.status, CheckStatus::Warn);
    assert_eq!(check.detail.as_deref(), Some("no node executable found"));
}

#[test]
fn warns_when_path_is_unset() {
    let check = check_node_on_path(None);

    dbg!(&check);
    assert_eq!(check.status, CheckStatus::Warn);
}

#[test]
fn a_directory_named_node_is_not_a_node_executable() {
    let root = tempfile::tempdir().expect("create temp dir");
    fs::create_dir_all(root.path().join(NODE_FILE_NAME)).expect("create node dir");

    let (usable, unusable) = find_node_entries(&[root.path().to_path_buf()]);

    assert_eq!(usable, Vec::<std::path::PathBuf>::new());
    assert_eq!(
        unusable,
        [UnusableNode { path: root.path().join(NODE_FILE_NAME), reason: "not a file" }],
    );
}

/// A version manager that swaps its per-shell link leaves a dangling `node`
/// behind; the lookup skips it and the check has to say so.
#[cfg(unix)]
#[test]
fn warns_about_a_broken_node_link_before_a_working_one() {
    let root = tempfile::tempdir().expect("create temp dir");
    let broken = root.path().join("broken");
    let working = root.path().join("working");
    fs::create_dir_all(&broken).expect("create broken dir");
    std::os::unix::fs::symlink(root.path().join("gone"), broken.join("node")).expect("link node");
    write_node(&working);
    let path = env::join_paths([&broken, &working]).expect("join PATH");

    let check = check_node_on_path(Some(&path));

    dbg!(&check);
    assert_eq!(check.status, CheckStatus::Warn);
    assert_eq!(
        check.detail.as_deref(),
        Some(
            format!(
                "{}; skipped: {} (broken link)",
                working.join("node").display(),
                broken.join("node").display(),
            )
            .as_str()
        ),
    );
}

#[cfg(unix)]
#[test]
fn a_node_without_execute_permission_is_skipped() {
    let root = tempfile::tempdir().expect("create temp dir");
    fs::write(root.path().join("node"), "").expect("write node");

    let (usable, unusable) = find_node_entries(&[root.path().to_path_buf()]);

    assert_eq!(usable, Vec::<std::path::PathBuf>::new());
    assert_eq!(
        unusable,
        [UnusableNode { path: root.path().join("node"), reason: "not executable" }],
    );
}

/// The owner's permission bits decide for the owner, even when others may
/// execute the file.
#[cfg(unix)]
#[test]
fn a_node_only_others_may_execute_is_skipped() {
    use std::os::unix::fs::PermissionsExt;

    // Root may execute any file that has an execute bit set.
    // SAFETY: `geteuid` has no preconditions.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let root = tempfile::tempdir().expect("create temp dir");
    let node = root.path().join("node");
    fs::write(&node, "#!/bin/sh\n").expect("write node");
    fs::set_permissions(&node, fs::Permissions::from_mode(0o645)).expect("chmod node");

    let (usable, unusable) = find_node_entries(&[root.path().to_path_buf()]);

    assert_eq!(usable, Vec::<std::path::PathBuf>::new());
    assert_eq!(unusable, [UnusableNode { path: node, reason: "not executable" }]);
}

#[cfg(unix)]
#[test]
fn shell_behind_follows_a_linked_sh() {
    let root = tempfile::tempdir().expect("create temp dir");
    let real = root.path().join("dash");
    fs::write(&real, "").expect("write shell");
    let sh = root.path().join("sh");
    std::os::unix::fs::symlink(&real, &sh).expect("link sh");

    let behind = shell_behind(&sh);

    assert_eq!(behind, Some(fs::canonicalize(&real).expect("canonicalize shell")));
    assert_eq!(shell_behind(&fs::canonicalize(&real).expect("canonicalize shell")), None);
}

#[test]
fn fails_when_the_script_shell_is_missing() {
    let root = tempfile::tempdir().expect("create temp dir");
    let missing = root.path().join("no-such-shell");
    let config = Config { script_shell: Some(missing.display().to_string()), ..Config::default() };

    let check = check_script_shell(&config);

    dbg!(&check);
    assert_eq!(check.status, CheckStatus::Fail);
    assert_eq!(
        check.detail.as_deref(),
        Some(format!("{} was not found", missing.display()).as_str()),
    );
}

#[test]
fn the_probe_install_gets_the_effective_script_shell() {
    let default_shell =
        pnpm_executor::select_shell(None, cfg!(windows)).expect("select the default shell").program;
    assert_eq!(
        shell_settings(&Config::default()),
        [
            "--config.shell-emulator=false".to_owned(),
            format!("--config.script-shell={}", default_shell.display()),
        ],
    );
    let configured = Config {
        script_shell: Some("/opt/shell".to_owned()),
        shell_emulator: true,
        ..Config::default()
    };
    assert_eq!(
        shell_settings(&configured),
        ["--config.shell-emulator=false", "--config.script-shell=/opt/shell"],
    );
    let batch_file =
        Config { script_shell: Some("/tools/shell.bat".to_owned()), ..Config::default() };
    assert_eq!(
        shell_settings(&batch_file),
        ["--config.shell-emulator=false", "--config.script-shell=/tools/shell.bat"],
    );
    let relative = Config { script_shell: Some("tools/sh".to_owned()), ..Config::default() };
    let anchored = std::path::absolute("tools/sh").expect("make the shell path absolute");
    assert_eq!(
        shell_settings(&relative),
        [
            "--config.shell-emulator=false".to_owned(),
            format!("--config.script-shell={}", anchored.display()),
        ],
    );
    let bare = Config { script_shell: Some("bash".to_owned()), ..Config::default() };
    assert_eq!(
        shell_settings(&bare),
        ["--config.shell-emulator=false", "--config.script-shell=bash"],
    );
    let emulated = Config { shell_emulator: true, ..Config::default() };
    assert_eq!(shell_settings(&emulated), ["--config.shell-emulator=true"]);
}
