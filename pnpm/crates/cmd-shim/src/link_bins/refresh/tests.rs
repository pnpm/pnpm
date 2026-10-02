use super::{DirectoryBinPlan, HashSet, LinkBinsOptions, Path};
use crate::{Host, link_bins};
use std::{fs, path::PathBuf};

mod counting;
use counting::{Counting, READ_DIRS, READ_HEADS, READ_MANIFESTS};

#[test]
fn discovers_once_and_refreshes_only_changed_winners() {
    let temp = tempfile::tempdir().unwrap();
    let modules = temp.path().join("node_modules");
    let winner = package(&modules, "tool", "tool");
    let loser = package(&modules, "rival", "tool");
    let stable = package(&modules, "stable", "stable");
    let bins = modules.join(".bin");
    let options = LinkBinsOptions { force: true, ..LinkBinsOptions::default() };
    link_bins::<Host>(&modules, &bins, &options).unwrap();
    let mut plan = DirectoryBinPlan::discover::<Counting>(&modules).unwrap();
    assert_eq!(READ_DIRS.lock().unwrap().len(), 1);
    assert_eq!(READ_MANIFESTS.lock().unwrap().len(), 3);
    plan.refresh::<Counting>(&HashSet::from([loser]), &bins, &options)
        .unwrap();
    assert!(READ_HEADS.lock().unwrap().is_empty());
    fs::write(winner.join("cli.js"), "#!/usr/bin/env node --no-warnings\n").unwrap();
    plan.refresh::<Counting>(&HashSet::from([winner.clone()]), &bins, &options)
        .unwrap();
    assert_eq!(READ_DIRS.lock().unwrap().len(), 1);
    assert_eq!(READ_MANIFESTS.lock().unwrap().len(), 5);
    assert!(
        READ_HEADS
            .lock()
            .unwrap()
            .iter()
            .all(|path| path.starts_with(&winner)),
    );
    assert!(
        !READ_HEADS
            .lock()
            .unwrap()
            .iter()
            .any(|path| path.starts_with(&stable)),
    );
    assert!(fs::read_to_string(bins.join("tool")).unwrap().contains("--no-warnings"));
}

#[test]
fn removed_winner_restores_collision_fallback_and_new_bins_are_linked() {
    let temp = tempfile::tempdir().unwrap();
    let modules = temp.path().join("node_modules");
    let winner = package(&modules, "tool", "tool");
    let _fallback = package(&modules, "rival", "tool");
    let options = LinkBinsOptions { force: true, ..LinkBinsOptions::default() };
    let bins = modules.join(".bin");
    link_bins::<Host>(&modules, &bins, &options).unwrap();
    let mut plan = DirectoryBinPlan::discover::<Host>(&modules).unwrap();
    fs::write(winner.join("package.json"), r#"{"name":"tool","bin":{"fresh":"cli.js"}}"#).unwrap();
    plan.refresh::<Host>(&HashSet::from([winner.clone()]), &bins, &options)
        .unwrap();
    assert!(fs::read_to_string(bins.join("tool")).unwrap().contains("rival"));
    assert!(bins.join("fresh").exists());
    fs::write(winner.join("package.json"), r#"{"name":"tool"}"#).unwrap();
    plan.refresh::<Host>(&HashSet::from([winner]), &bins, &options)
        .unwrap();
    assert!(!bins.join("fresh").exists());
    if cfg!(windows) {
        assert!(!bins.join("fresh.cmd").exists());
        assert!(!bins.join("fresh.ps1").exists());
    }
}

#[test]
fn failed_write_retains_pending_removed_commands_for_retry() {
    let temp = tempfile::tempdir().unwrap();
    let modules = temp.path().join("node_modules");
    let winner = package(&modules, "tool", "old");
    let bins = modules.join(".bin");
    let options = LinkBinsOptions { force: true, ..LinkBinsOptions::default() };
    link_bins::<Host>(&modules, &bins, &options).unwrap();
    let mut plan = DirectoryBinPlan::discover::<Host>(&modules).unwrap();
    fs::write(winner.join("package.json"), r#"{"name":"tool","bin":{"fresh":"cli.js"}}"#).unwrap();
    fs::create_dir(bins.join("fresh")).unwrap();
    let completed = HashSet::from([winner]);
    assert!(plan.refresh::<Host>(&completed, &bins, &options).is_err());
    fs::remove_dir(bins.join("fresh")).unwrap();
    plan.refresh::<Host>(&completed, &bins, &options).unwrap();
    assert!(bins.join("fresh").exists());
    assert!(!bins.join("old").exists());
}

fn package(modules: &Path, name: &str, command: &str) -> PathBuf {
    let root = modules.join(name);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("package.json"),
        serde_json::json!({"name":name,"bin":{command:"cli.js"}}).to_string(),
    )
    .unwrap();
    fs::write(root.join("cli.js"), "#!/usr/bin/env node\n").unwrap();
    root
}

#[test]
fn deleted_windows_bins_preserve_outputs_owned_by_surviving_dotted_commands() {
    let provided = HashSet::from(["tool.cmd".to_owned(), "keep".to_owned()]);
    let removed = super::removal::removable_names("tool", &provided, true);
    assert_eq!(removed, ["tool", "tool.ps1", "tool.exe"]);
    let removed = super::removal::removable_names("keep.cmd", &provided, true);
    assert_eq!(removed, ["keep.cmd.cmd", "keep.cmd.ps1", "keep.cmd.exe"]);
}

#[test]
fn refreshing_an_alias_preserves_equal_rank_discovery_order() {
    let temp = tempfile::tempdir().unwrap();
    let modules = temp.path().join("node_modules");
    for alias in ["first-alias", "second-alias"] {
        let root = package(&modules, alias, "tool");
        fs::write(root.join("package.json"), r#"{"name":"tool","version":"1.0.0","bin":"cli.js"}"#)
            .unwrap();
    }
    let mut plan = DirectoryBinPlan::discover::<Host>(&modules).unwrap();
    let initial_winner = plan
        .winner("tool")
        .unwrap()
        .1
        .location
        .clone();
    fs::write(initial_winner.join("cli.js"), "#!/usr/bin/env node --no-warnings\n").unwrap();
    let bins = modules.join(".bin");
    let options = LinkBinsOptions { force: true, ..LinkBinsOptions::default() };
    plan.refresh::<Host>(&HashSet::from([initial_winner.clone()]), &bins, &options)
        .unwrap();
    assert_eq!(plan.winner("tool").unwrap().1.location, initial_winner);
    let shim = fs::read_to_string(bins.join("tool")).unwrap();
    assert!(
        shim.contains(
            initial_winner
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
        ),
    );
    assert!(shim.contains("--no-warnings"));
}

#[test]
fn multi_bin_provider_updates_all_commands_and_retains_its_alias_rank_after_deletion() {
    let temp = tempfile::tempdir().unwrap();
    let modules = temp.path().join("node_modules");
    let provider = package(&modules, "first-alias", "first");
    let fallback = package(&modules, "second-alias", "first");
    let manifest = serde_json::json!({
        "name": "same-package", "version": "1.0.0",
        "bin": {"first": "cli.js", "second": "cli.js"},
    });
    for directory in [&provider, &fallback] {
        fs::write(directory.join("package.json"), manifest.to_string()).unwrap();
    }
    let source = |directory: &PathBuf| {
        crate::PackageBinSource::new(directory.clone(), std::sync::Arc::new(manifest.clone()))
    };
    let sources = [source(&provider), source(&fallback)];
    let mut plan = DirectoryBinPlan::from_packages::<Host>(&sources);
    let bins = modules.join(".bin");
    let options = LinkBinsOptions { force: true, ..LinkBinsOptions::default() };
    let completed = HashSet::from([provider.clone()]);
    fs::write(provider.join("cli.js"), "#!/usr/bin/env node --no-warnings\n").unwrap();
    plan.refresh::<Host>(&completed, &bins, &options).unwrap();
    for name in ["first", "second"] {
        let shim = fs::read_to_string(bins.join(name)).unwrap();
        assert!(shim.contains("first-alias"));
        assert!(shim.contains("--no-warnings"));
    }
    fs::remove_file(provider.join("package.json")).unwrap();
    plan.refresh::<Host>(&completed, &bins, &options).unwrap();
    assert!(plan.package_source(&provider).is_none());
    for name in ["first", "second"] {
        assert!(fs::read_to_string(bins.join(name)).unwrap().contains("second-alias"));
    }
    fs::write(provider.join("package.json"), manifest.to_string()).unwrap();
    plan.refresh::<Host>(&completed, &bins, &options).unwrap();
    for name in ["first", "second"] {
        assert!(fs::read_to_string(bins.join(name)).unwrap().contains("first-alias"));
    }
    fs::write(provider.join("package.json"), r#"{"name":"same-package","version":"1.0.0"}"#)
        .unwrap();
    plan.refresh::<Host>(&completed, &bins, &options).unwrap();
    assert!(
        plan.package_source(&provider)
            .unwrap()
            .manifest
            .get("bin")
            .is_none(),
    );
}

#[test]
fn newly_created_manifest_adds_a_provider_then_deletion_restores_prior_winners() {
    let temp = tempfile::tempdir().unwrap();
    let modules = temp.path().join("node_modules");
    package(&modules, "rival", "tool");
    let mut plan = DirectoryBinPlan::discover::<Host>(&modules).unwrap();
    let provider = package(&modules, "tool", "tool");
    assert!(plan.package_source(&provider).is_none());
    fs::write(
        provider.join("package.json"),
        r#"{"name":"tool","bin":{"tool":"cli.js","fresh":"cli.js"}}"#,
    )
    .unwrap();
    let options = LinkBinsOptions { force: true, ..LinkBinsOptions::default() };
    let bins = modules.join(".bin");
    let completed = HashSet::from([provider.clone()]);
    plan.refresh::<Host>(&completed, &bins, &options).unwrap();
    assert!(fs::read_to_string(bins.join("tool")).unwrap().contains("tool/cli.js"));
    assert!(bins.join("fresh").exists());
    fs::remove_file(provider.join("package.json")).unwrap();
    plan.refresh::<Host>(&completed, &bins, &options).unwrap();
    assert!(plan.package_source(&provider).is_none());
    assert!(fs::read_to_string(bins.join("tool")).unwrap().contains("rival"));
    assert!(!bins.join("fresh").exists());
}
