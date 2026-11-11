use crate::_utils::{ManifestDeps, WorkspaceFixture, assert_success, read_v11_bundle_path};
use pnpm_lockfile::Lockfile;
use std::ffi::OsStr;

const CONSUMER: &str = "@pnpm.e2e/has-optional-peer-also-in-deps";
const PROVIDER: &str = "@pnpm.e2e/bravo-dep";

#[test]
fn removing_a_deduplicated_peer_dependent_restores_required_children() {
    for auto_install in [false, true] {
        for format in ["v9", "v13"] {
            check_removal(None, auto_install, format);
        }
        if let Some(bundle) = read_v11_bundle_path() {
            check_removal(Some(&bundle), auto_install, "v9");
        }
    }
}

fn check_removal(bundle: Option<&OsStr>, auto_install: bool, format: &str) {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml(&format!(
        "autoInstallPeers: {auto_install}\nresolvePeersFromWorkspaceRoot: false\ndedupePeerDependents: true\nlockfile:\n  format: {format}\n",
    ));
    fixture.write_root_manifest(
        "root",
        ManifestDeps { prod: &[(CONSUMER, "1.0.0"), (PROVIDER, "1.0.0")], ..Default::default() },
    );
    fixture.project(
        "app",
        "app",
        ManifestDeps { prod: &[(CONSUMER, "1.0.0")], ..Default::default() },
    );
    let install = || {
        let output = match bundle {
            Some(bundle) => fixture.v11_command_at(bundle, &fixture.workspace, ["install"]),
            None => fixture.command_at(&fixture.workspace, ["install"]),
        };
        assert_success(&output);
    };
    install();
    fixture.write_root_manifest(
        "root",
        ManifestDeps { prod: &[(PROVIDER, "1.0.0")], ..Default::default() },
    );
    install();
    let lockfile = Lockfile::load_wanted_from_dir(&fixture.workspace).unwrap().unwrap();
    let importer = &lockfile.importers["packages/app"];
    let alias = CONSUMER.parse().unwrap();
    let key = importer.dependencies.as_ref().unwrap()[&alias].version.resolved_key(&alias).unwrap();
    let snapshot = &lockfile.snapshots.as_ref().unwrap()[&key];
    assert!(
        snapshot.dependencies
            .as_ref()
            .is_some_and(|deps| deps.contains_key(&PROVIDER.parse().unwrap())),
        "{bundle:?}, {auto_install}, {format}: {snapshot:?}",
    );
}
