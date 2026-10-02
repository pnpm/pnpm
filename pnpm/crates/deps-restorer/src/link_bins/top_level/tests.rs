use super::{
    HashMap, Host, LinkBinsOptions, Path, PathBuf, PreparedPackageBins, fs,
    link_top_level_bins_cached,
};
use pnpm_cmd_shim::DirectoryBinPlan;
use std::collections::HashSet;

#[test]
fn cached_importer_sources_keep_selected_alias_order_and_fall_back_for_new_locations() {
    let temp = tempfile::tempdir().unwrap();
    let modules = temp.path().join("node_modules");
    let first = package(&modules, "first", "tool", Some("tool"));
    let second = package(&modules, "second", "tool", Some("tool"));
    let no_bin = package(&modules, "empty", "empty", None);
    let plan = DirectoryBinPlan::discover::<Host>(&modules).unwrap();
    let cached = snapshots(&plan, &[first.clone(), second.clone(), no_bin.clone()]);
    assert_eq!(cached.len(), 3);
    for root in [&first, &second, &no_bin] {
        fs::write(root.join("package.json"), "cached manifest must not be reread").unwrap();
    }
    package(&modules, "new", "new", Some("new"));
    let options = LinkBinsOptions::default();
    link_top_level_bins_cached(
        &modules,
        &["second".into(), "first".into(), "empty".into(), "new".into()],
        &[],
        &[],
        &options,
        Some(&cached),
    )
    .unwrap();
    let shim = fs::read_to_string(modules.join(".bin/tool")).unwrap();
    assert!(shim.contains("second"));
    assert!(!shim.contains("first"));
    assert!(modules.join(".bin/new").exists());
}

#[test]
fn cached_importer_sources_use_the_completed_build_manifest() {
    let temp = tempfile::tempdir().unwrap();
    let modules = temp.path().join("node_modules");
    let root = package(&modules, "tool", "tool", Some("old"));
    let mut plan = DirectoryBinPlan::discover::<Host>(&modules).unwrap();
    package(&modules, "tool", "tool", Some("fresh"));
    let options = LinkBinsOptions { force: true, ..LinkBinsOptions::default() };
    plan.refresh::<Host>(&HashSet::from([root.clone()]), &modules.join(".bin"), &options)
        .unwrap();
    let cached = snapshots(&plan, std::slice::from_ref(&root));
    fs::write(root.join("package.json"), "completed manifest must not be reread").unwrap();
    fs::remove_dir_all(modules.join(".bin")).unwrap();
    link_top_level_bins_cached(&modules, &["tool".into()], &[], &[], &options, Some(&cached))
        .unwrap();
    assert!(modules.join(".bin/fresh").exists());
    assert!(!modules.join(".bin/old").exists());
}

#[test]
fn cached_importer_sources_preserve_direct_hoisted_and_peer_precedence() {
    let temp = tempfile::tempdir().unwrap();
    let modules = temp.path().join("node_modules");
    let direct = package(&modules, "direct-alias", "z-direct", Some("tool"));
    let hoisted = package(&modules, "hoisted-alias", "tool", Some("tool"));
    let peer = package(temp.path(), "peer-slot", "a-peer", Some("tool"));
    let plan = DirectoryBinPlan::discover::<Host>(&modules).unwrap();
    let cached = snapshots(&plan, &[direct, hoisted]);
    let options = LinkBinsOptions { force: true, ..LinkBinsOptions::default() };
    link_top_level_bins_cached(
        &modules,
        &["direct-alias".into()],
        &["hoisted-alias".into()],
        std::slice::from_ref(&peer),
        &options,
        Some(&cached),
    )
    .unwrap();
    assert!(fs::read_to_string(modules.join(".bin/tool")).unwrap().contains("direct-alias"));
    link_top_level_bins_cached(
        &modules,
        &[],
        &["hoisted-alias".into()],
        std::slice::from_ref(&peer),
        &options,
        Some(&cached),
    )
    .unwrap();
    assert!(fs::read_to_string(modules.join(".bin/tool")).unwrap().contains("hoisted-alias"));
    link_top_level_bins_cached(&modules, &[], &[], &[peer], &options, Some(&cached)).unwrap();
    assert!(fs::read_to_string(modules.join(".bin/tool")).unwrap().contains("peer-slot"));
}

fn snapshots(
    plan: &DirectoryBinPlan,
    locations: &[PathBuf],
) -> HashMap<PathBuf, PreparedPackageBins> {
    locations
        .iter()
        .filter_map(|location| {
            plan.package_bins(location)
                .map(|source| (location.clone(), source))
        })
        .collect()
}

fn package(modules: &Path, alias: &str, name: &str, bin: Option<&str>) -> PathBuf {
    let root = modules.join(alias);
    fs::create_dir_all(&root).unwrap();
    let manifest = match bin {
        Some(bin) => serde_json::json!({"name": name, "version":"1.0.0", "bin":{bin:"cli.js"}}),
        None => serde_json::json!({"name": name, "version":"1.0.0"}),
    };
    fs::write(root.join("package.json"), manifest.to_string()).unwrap();
    fs::write(root.join("cli.js"), "#!/usr/bin/env node\n").unwrap();
    root
}
