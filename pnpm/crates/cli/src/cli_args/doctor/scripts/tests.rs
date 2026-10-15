use super::{NODE_FILE_NAME, UnusableNode, check_node_on_path, find_node_entries};
use crate::cli_args::doctor::CheckStatus;
use pretty_assertions::assert_eq;
use std::{env, fs, path::Path};

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
