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
    let temp = tempfile::tempdir().unwrap();
    let real_project = temp.path().join("real");
    fs::create_dir_all(real_project.join("node_modules")).unwrap();
    let linked_project = temp.path().join("linked");
    pnpm_fs::symlink_dir(&real_project, &linked_project).unwrap();

    let mut claimed = ClaimedCommands::default();
    claimed.claim(&real_project.join("node_modules"), commands(&["a"]));
    assert!(claimed.claim_recorded(&linked_project.join("node_modules"), commands(&["b"])));
    assert_eq!(claimed.get(&real_project.join("node_modules")), Some(&commands(&["a", "b"])));
}

#[test]
fn prune_keeps_the_commands_claimed_under_any_path_of_the_directory() {
    let temp = tempfile::tempdir().unwrap();
    let real_project = temp.path().join("real");
    let modules_dir = real_project.join("node_modules");
    for command in ["a", "b", "stale"] {
        package(&modules_dir, command);
    }
    let options = LinkBinsOptions { force: true, ..LinkBinsOptions::default() };
    link_bins::<Host>(&modules_dir, &modules_dir.join(".bin"), &options).unwrap();
    let linked_project = temp.path().join("linked");
    pnpm_fs::symlink_dir(&real_project, &linked_project).unwrap();

    let mut claimed = ClaimedCommands::default();
    claimed.claim(&modules_dir, commands(&["a"]));
    claimed.claim(&linked_project.join("node_modules"), commands(&["b"]));
    claimed
        .prune_stale_bins([linked_project.join("node_modules").as_path(), modules_dir.as_path()])
        .unwrap();

    let bins_dir = modules_dir.join(".bin");
    assert!(bins_dir.join("a").exists());
    assert!(bins_dir.join("b").exists());
    assert!(!bins_dir.join("stale").exists());
}
