use super::{
    Arc, HashMap, LinkBinsOptions, PackageBinSource, PreparedPackageBins,
    link_bins_of_packages_precomputed,
};
use crate::{Host, link_bins_of_packages};
use std::fs;

#[test]
fn cached_commands_match_normal_links_and_still_probe_and_repair_every_winner() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("tool");
    fs::create_dir_all(root.join("commands")).unwrap();
    fs::write(root.join("commands/tool"), "#!/usr/bin/env node\n").unwrap();
    let source = PackageBinSource::new(
        root.clone(),
        Arc::new(serde_json::json!({
            "name": "tool", "directories": {"bin": "commands"},
        })),
    );
    let prepared =
        HashMap::from([(root.clone(), PreparedPackageBins::new::<Host>(source.clone()))]);
    let bins = temp.path().join(".bin");
    let options = LinkBinsOptions { force: true, ..LinkBinsOptions::default() };
    link_bins_of_packages::<Host>(std::slice::from_ref(&source), &bins, &options).unwrap();
    let expected = fs::read(bins.join("tool")).unwrap();
    fs::remove_dir_all(&bins).unwrap();
    link_bins_of_packages_precomputed::<Host>(
        std::slice::from_ref(&source),
        &prepared,
        &bins,
        &options,
    )
    .unwrap();
    assert_eq!(fs::read(bins.join("tool")).unwrap(), expected);
    fs::write(root.join("commands/new"), "#!/usr/bin/env node\n").unwrap();
    fs::write(root.join("commands/tool"), "#!/usr/bin/env node --no-warnings\n").unwrap();
    fs::write(bins.join("tool"), "broken shim").unwrap();
    link_bins_of_packages_precomputed::<Host>(&[source], &prepared, &bins, &options).unwrap();
    assert!(!bins.join("new").exists(), "declarations come from the prepared snapshot");
    assert!(fs::read_to_string(bins.join("tool")).unwrap().contains("--no-warnings"));
}

#[test]
fn a_different_manifest_snapshot_uses_fresh_commands() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("tool");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("cli.js"), "#!/usr/bin/env node\n").unwrap();
    let source = PackageBinSource::new(
        root.clone(),
        Arc::new(serde_json::json!({
            "name": "tool", "bin": {"old": "cli.js"},
        })),
    );
    let prepared = HashMap::from([(root.clone(), PreparedPackageBins::new::<Host>(source))]);
    let changed = PackageBinSource::new(
        root,
        Arc::new(serde_json::json!({
            "name": "tool", "bin": {"new": "cli.js"},
        })),
    );
    let bins = temp.path().join(".bin");
    link_bins_of_packages_precomputed::<Host>(
        &[changed],
        &prepared,
        &bins,
        &LinkBinsOptions::default(),
    )
    .unwrap();
    assert!(bins.join("new").exists());
    assert!(!bins.join("old").exists());
}
