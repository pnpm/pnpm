use super::moving_providers::{
    install_lockfile, normalize_registry_origin, registry_from_manifests,
};
use crate::_utils::{ManifestDeps, WorkspaceFixture, read_v11_bundle_path};
use pnpm_lockfile::Lockfile;
use serde_json::json;

#[test]
fn aliased_package_retains_a_different_version_of_its_own_name_as_a_peer() {
    let bundle = read_v11_bundle_path();
    let mut fresh_by_settings = std::collections::HashMap::new();
    for (bundle, format, dedupe, suffix_length) in bundle
        .as_deref()
        .map(|bundle| (Some(bundle), "v9"))
        .into_iter()
        .chain([(None, "v9"), (None, "v13")])
        .flat_map(|(bundle, format)| [false, true].map(move |dedupe| (bundle, format, dedupe)))
        .flat_map(|(bundle, format, dedupe)| {
            [1, 1000].map(move |suffix_length| (bundle, format, dedupe, suffix_length))
        })
    {
        let registry_dir = tempfile::tempdir().unwrap();
        let mut fixture = WorkspaceFixture::new();
        fixture.registry.mock_instance = registry_from_manifests(
            registry_dir.path(),
            [
                json!({"name":"provider","version":"1.0.0"}),
                json!({"name":"provider","version":"2.0.0","dependencies":{"consumer":"1.0.0"}}),
                json!({"name":"provider","version":"3.0.0"}),
                json!({"name":"consumer","version":"1.0.0","peerDependencies":{"provider":"*"}}),
            ],
        );
        fixture.append_workspace_yaml(&format!(
            "autoInstallPeers: false\ndedupePeerDependents: {dedupe}\npeersSuffixMaxLength: {suffix_length}\nlockfile:\n  format: {format}\n",
        ));

        fixture.write_root_manifest(
            "root",
            ManifestDeps {
                prod: &[("aliased", "npm:provider@2.0.0"), ("provider", "1.0.0")],
                ..Default::default()
            },
        );
        fixture.project(
            "app",
            "app",
            ManifestDeps {
                prod: &[("aliased", "npm:provider@2.0.0"), ("provider", "3.0.0")],
                ..Default::default()
            },
        );
        install_lockfile(&fixture, bundle);
        let lockfile = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        for (importer, version) in [(".", "1.0.0"), ("packages/app", "3.0.0")] {
            let direct = lockfile.importers[importer].dependencies.as_ref().unwrap();
            let alias = "aliased".parse().unwrap();
            let key = direct[&alias].version.resolved_key(&alias).unwrap();
            let snapshots = lockfile.snapshots.as_ref().unwrap();
            let consumer = "consumer".parse().unwrap();
            let consumer_key = snapshots[&key].dependencies.as_ref().unwrap()[&consumer]
                .resolve(&consumer)
                .unwrap();
            assert_eq!(
                snapshots[&consumer_key].dependencies.as_ref().unwrap()
                    [&"provider".parse().unwrap()]
                    .to_string(),
                version,
                "{bundle:?} {format} {dedupe} {suffix_length} {importer}",
            );
        }
        install_lockfile(&fixture, bundle);
        let reused = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
        assert_eq!(lockfile, reused, "{bundle:?} {format} {dedupe} {suffix_length}");
        if format == "v9" {
            let normalized = normalize_registry_origin(lockfile, &fixture);
            if let Some(previous) =
                fresh_by_settings.insert((dedupe, suffix_length), normalized.clone())
            {
                assert_eq!(previous, normalized, "fresh parity, {dedupe} {suffix_length}");
            }
        }
    }
}
