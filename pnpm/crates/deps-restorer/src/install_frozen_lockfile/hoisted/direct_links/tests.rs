use super::HoistedLinkScope;
use pnpm_cmd_shim::{LinkBinsOptions, PackageBinSource};
use std::{collections::BTreeMap, fs, path::Path};

fn write_package(root: &Path, command: &str) {
    fs::create_dir_all(root).unwrap();
    fs::write(root.join("cli.js"), "#!/usr/bin/env node\n").unwrap();
    fs::write(
        root.join("package.json"),
        serde_json::json!({
            "name": "provider", "version": "1.0.0", "bin": {command: "cli.js"},
        })
        .to_string(),
    )
    .unwrap();
}

fn initial_link(modules: &Path, aliases: &[&str]) -> Vec<PackageBinSource> {
    let deps = aliases
        .iter()
        .map(|alias| (*alias, None))
        .collect::<Vec<_>>();
    crate::link_bins::link_named_dep_bins_with_sources(
        modules,
        &deps,
        &LinkBinsOptions::default(),
        true,
    )
    .unwrap()
    .1
}

fn direct_link(
    modules: &Path,
    aliases: &[&str],
    sources: Option<&[PackageBinSource]>,
) -> Result<bool, super::HoistedLinkerError> {
    let dependencies = aliases
        .iter()
        .map(|alias| ((*alias).to_owned(), modules.join(alias)))
        .collect();
    scope(modules)
        .link_direct_dependencies(
            &BTreeMap::from([(".".to_owned(), dependencies)]),
            &LinkBinsOptions::default(),
            sources,
        )
}

fn scope(modules: &Path) -> HoistedLinkScope<'_> {
    HoistedLinkScope {
        importer_id: ".".to_owned(),
        root_modules_dir: modules,
        modules_dir: modules.to_owned(),
        is_workspace_root: true,
    }
}

#[test]
fn identical_candidates_do_not_read_manifests_or_relink_twice() {
    let temp = tempfile::tempdir().unwrap();
    let modules = temp.path().join("node_modules");
    write_package(&modules.join("tool"), "tool");
    let sources = initial_link(&modules, &["tool"]);
    let expected = fs::read(modules.join(".bin/tool")).unwrap();
    // A poisoned read proves this call reuses the completed pass without filesystem work.
    fs::write(modules.join("tool/package.json"), "invalid JSON").unwrap();
    assert!(!direct_link(&modules, &["tool"], Some(&sources)).unwrap());
    assert_eq!(fs::read(modules.join(".bin/tool")).unwrap(), expected);
    assert!(direct_link(&modules, &["tool"], None).is_err());
}

#[test]
fn different_candidate_order_relinks_equal_rank_aliases() {
    let temp = tempfile::tempdir().unwrap();
    let modules = temp.path().join("node_modules");
    write_package(&modules.join("alpha"), "tool");
    write_package(&modules.join("beta"), "tool");
    let sources = initial_link(&modules, &["beta", "alpha"]);
    let initial = fs::read(modules.join(".bin/tool")).unwrap();
    direct_link(&modules, &["alpha", "beta"], Some(&sources)).unwrap();
    let direct = fs::read(modules.join(".bin/tool")).unwrap();
    assert_ne!(initial, direct);
    initial_link(&modules, &["alpha", "beta"]);
    assert_eq!(fs::read(modules.join(".bin/tool")).unwrap(), direct);
}

#[test]
fn direct_subset_relinks_the_selected_candidate() {
    let temp = tempfile::tempdir().unwrap();
    let modules = temp.path().join("node_modules");
    write_package(&modules.join("alpha"), "tool");
    write_package(&modules.join("beta"), "tool");
    let sources = initial_link(&modules, &["alpha", "beta"]);
    let initial = fs::read(modules.join(".bin/tool")).unwrap();
    direct_link(&modules, &["beta"], Some(&sources)).unwrap();
    assert_ne!(fs::read(modules.join(".bin/tool")).unwrap(), initial);
}

#[test]
fn remapped_alias_relinks_even_when_candidate_locations_match() {
    let temp = tempfile::tempdir().unwrap();
    let modules = temp.path().join("node_modules");
    let old = temp.path().join("old");
    let new = temp.path().join("new");
    write_package(&old, "old-command");
    write_package(&new, "new-command");
    fs::create_dir_all(&modules).unwrap();
    crate::symlink_package(&old, &modules.join("tool")).unwrap();
    let sources = initial_link(&modules, &["tool"]);
    let dependencies = BTreeMap::from([("tool".to_owned(), new)]);
    assert!(
        scope(&modules)
            .link_direct_dependencies(
                &BTreeMap::from([(".".to_owned(), dependencies)]),
                &LinkBinsOptions::default(),
                Some(&sources),
            )
            .unwrap(),
    );
    assert!(modules.join(".bin/new-command").exists());
}
