use super::ClaimedCommands;
use pnpm_cmd_shim::{Host, LinkBinsOptions, link_bins};
use std::{collections::HashSet, fs, path::Path};

fn commands(names: &[&str]) -> HashSet<String> {
    names
        .iter()
        .map(|name| (*name).to_owned())
        .collect()
}

fn package(modules_dir: &Path, command: &str) {
    let root = modules_dir.join(command);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("package.json"),
        serde_json::json!({ "name": command, "bin": { command: "cli.js" } }).to_string(),
    )
    .unwrap();
    fs::write(root.join("cli.js"), "#!/usr/bin/env node\n").unwrap();
}

#[test]
fn a_claim_is_keyed_by_the_normalized_path() {
    let mut claimed = ClaimedCommands::default();
    claimed.claim(Path::new("/repo/pkg/../node_modules"), commands(&["tool"]));
    claimed.claim(Path::new("/repo/./node_modules"), commands(&["other"]));
    assert_eq!(claimed.get(Path::new("/repo/node_modules")), Some(&commands(&["tool", "other"])));
}

#[test]
fn claim_recorded_adds_only_to_a_directory_a_pass_claimed() {
    let mut claimed = ClaimedCommands::default();
    assert!(!claimed.claim_recorded(Path::new("/repo/node_modules"), commands(&["tool"])));
    assert_eq!(claimed.get(Path::new("/repo/node_modules")), None);

    claimed.claim(Path::new("/repo/node_modules"), HashSet::new());
    assert!(claimed.claim_recorded(Path::new("/repo/node_modules"), commands(&["tool"])));
    assert_eq!(claimed.get(Path::new("/repo/node_modules")), Some(&commands(&["tool"])));
}

#[test]
fn claim_recorded_finds_a_directory_claimed_through_another_path() {
    let (_temp, base) = symlinked_temp_dir();
    let real_project = base.join("real");
    fs::create_dir_all(real_project.join("node_modules")).unwrap();
    let linked_project = base.join("linked");
    pnpm_fs::symlink_dir(&real_project, &linked_project).unwrap();

    let mut claimed = ClaimedCommands::default();
    claimed.claim(&real_project.join("node_modules"), commands(&["a"]));
    assert!(claimed.claim_recorded(&linked_project.join("node_modules"), commands(&["b"])));
    assert_eq!(claimed.get(&real_project.join("node_modules")), Some(&commands(&["a", "b"])));
}

/// A temp dir reached through a symlink, the way macOS reaches its temp dir
/// through `/var`, so no path a test builds is already resolved.
fn symlinked_temp_dir() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let real = temp.path().join("real-base");
    fs::create_dir_all(&real).unwrap();
    let base = temp.path().join("base");
    pnpm_fs::symlink_dir(&real, &base).unwrap();
    (temp, base)
}

fn link_package_bins(modules_dir: &Path, names: &[&str]) {
    for command in names {
        package(modules_dir, command);
    }
    let options = LinkBinsOptions { force: true, ..LinkBinsOptions::default() };
    link_bins::<Host>(modules_dir, &modules_dir.join(".bin"), &options).unwrap();
}

fn assert_bins(bins_dir: &Path, kept: &[&str], removed: &[&str]) {
    let remaining: Vec<_> = fs::read_dir(bins_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    dbg!(bins_dir, &remaining);
    for command in kept {
        assert!(bins_dir.join(command).exists(), "{command} should be kept");
    }
    for command in removed {
        assert!(!bins_dir.join(command).exists(), "{command} should be removed");
    }
}

#[test]
fn prune_keeps_the_commands_claimed_under_any_path_of_the_directory() {
    for real_path_first in [true, false] {
        let (_temp, base) = symlinked_temp_dir();
        let real_project = base.join("real");
        let modules_dir = real_project.join("node_modules");
        link_package_bins(&modules_dir, &["a", "b", "stale"]);
        let linked_project = base.join("linked");
        pnpm_fs::symlink_dir(&real_project, &linked_project).unwrap();
        let linked_modules_dir = linked_project.join("node_modules");

        let mut claimed = ClaimedCommands::default();
        claimed.claim(&modules_dir, commands(&["a"]));
        claimed.claim(&linked_modules_dir, commands(&["b"]));
        let targets = if real_path_first {
            [modules_dir.as_path(), linked_modules_dir.as_path()]
        } else {
            [linked_modules_dir.as_path(), modules_dir.as_path()]
        };
        claimed.prune_stale_bins(targets).unwrap();

        assert_bins(&modules_dir.join(".bin"), &["a", "b"], &["stale"]);
    }
}

#[test]
fn prune_keeps_the_commands_of_a_project_whose_modules_dir_links_to_another() {
    let (_temp, base) = symlinked_temp_dir();
    let modules_dir = base.join("a/node_modules");
    link_package_bins(&modules_dir, &["a", "b", "stale"]);
    fs::create_dir_all(base.join("b")).unwrap();
    let linked_modules_dir = base.join("b/node_modules");
    pnpm_fs::symlink_dir(&modules_dir, &linked_modules_dir).unwrap();

    let mut claimed = ClaimedCommands::default();
    claimed.claim(&modules_dir, commands(&["a"]));
    claimed.claim(&linked_modules_dir, commands(&["b"]));
    claimed
        .prune_stale_bins([modules_dir.as_path()])
        .unwrap();

    assert_bins(&modules_dir.join(".bin"), &["a", "b"], &["stale"]);
}
