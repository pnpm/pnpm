use super::{
    assert_installed,
    command,
    mock_node_release,
    prepare_workspace,
    runtime_fixture,
    write_devengines_manifest,
    write_runtime_lockfile_for_group,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_graph_hasher::{
    host_arch,
    host_platform,
};
use serde_json::{
    Value,
    json,
};
use std::fs;

#[test]
fn hoisted_no_runtime_installs_can_be_repeated_and_restore_the_runtime() {
    let root = tempfile::tempdir().unwrap();
    let workspace = prepare_workspace(&root, "nodeLinker: hoisted\n");
    let mut server = mockito::Server::new();
    let fixture = runtime_fixture(&mut server, "node", "24.0.0", host_platform(), host_arch());
    write_devengines_manifest(&workspace, fixture.version, Some("download"));
    write_runtime_lockfile_for_group(&workspace, std::slice::from_ref(&fixture), "devDependencies");
    let tarball = pnpm_testing_utils::fixtures::minimal_tarball("dependency", "1.0.0");
    fs::write(workspace.join("dependency.tgz"), &tarball).unwrap();
    let manifest_path = workspace.join("package.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["dependencies"] = json!({ "dependency": "file:dependency.tgz" });
    fs::write(manifest_path, manifest.to_string()).unwrap();
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let mut lockfile: Value =
        serde_saphyr::from_str(&fs::read_to_string(&lockfile_path).unwrap()).unwrap();
    lockfile["importers"]["."]["dependencies"] = json!({
        "dependency": { "specifier": "file:dependency.tgz", "version": "file:dependency.tgz" },
    });
    lockfile["packages"]["dependency@file:dependency.tgz"] = json!({
        "name": "dependency",
        "version": "1.0.0",
        "resolution": { "tarball": "file:dependency.tgz" },
    });
    lockfile["snapshots"]["dependency@file:dependency.tgz"] = json!({});
    fs::write(&lockfile_path, serde_saphyr::to_string(&lockfile).unwrap()).unwrap();
    let wanted = fs::read(workspace.join("pnpm-lock.yaml")).unwrap();

    for extra in [None, None, Some("--force"), Some("--prod")] {
        let mut install = command(&workspace);
        install.args(["install", "--no-runtime", "--frozen-lockfile"]);
        if let Some(extra) = extra {
            install.arg(extra);
        }
        install.assert().success();
        assert!(!workspace.join("node_modules/node").exists());
        assert!(workspace.join("node_modules/dependency/package.json").exists());
        assert_eq!(fs::read(workspace.join("pnpm-lock.yaml")).unwrap(), wanted);
    }
    assert!(!fixture.archive_mock.matched());

    command(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert_installed(&workspace, std::slice::from_ref(&fixture));
}

#[test]
fn fresh_install_with_no_runtime_resolves_but_does_not_fetch_the_runtime() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new();
    let version = "24.0.0-rc.4";
    let [_index, _shasums, archive] = mock_node_release(&mut server, version);
    let workspace = prepare_workspace(
        &root,
        format!("nodeDownloadMirrors:\n  rc: '{}/'\n", server.url()).as_str(),
    );
    fs::write(
        workspace.join("package.json"),
        json!({ "dependencies": { "node": format!("runtime:{version}") } }).to_string(),
    )
    .unwrap();

    // No pnpm-lock.yaml, so the install takes the fresh-resolve path.
    command(&workspace)
        .with_args(["install", "--no-runtime"])
        .assert()
        .success();

    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).unwrap();
    assert!(
        lockfile.contains(format!("node@runtime:{version}").as_str()),
        "the resolved runtime stays in the lockfile:\n{lockfile}",
    );
    assert!(
        !workspace.join("node_modules/node").exists(),
        "the runtime must not be materialized under --no-runtime",
    );
    let bin_dir = workspace.join("node_modules/.bin");
    for bin in ["node", "node.exe", "node.cmd"] {
        assert!(!bin_dir.join(bin).exists(), "runtime bin {bin} must not be linked");
    }
    // A follow-up plain install treats the modules state as up to date
    // and does not restore the runtime — same as the TypeScript CLI.
    command(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert!(!workspace.join("node_modules/node").exists());
    assert!(!archive.matched(), "the runtime archive must never be downloaded");
}
