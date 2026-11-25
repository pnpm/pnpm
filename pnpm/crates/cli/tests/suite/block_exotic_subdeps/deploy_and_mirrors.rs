use super::{
    exotic_tarball_bytes,
    http_server::TestHttpServer,
    pnpm_at,
};
use crate::_utils::{
    self,
    append_workspace_yaml_key,
    gzipped_tarball,
    write_manifest_value,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils_registry::bin::CommandTempCwd;
use serde_json::json;
use std::{
    fs,
    sync::atomic::Ordering,
};

#[test]
fn deploy_and_legacy_deploy_preserve_unnamed_workspace_project_exemptions() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let exotic_server = TestHttpServer::start(exotic_tarball_bytes(), Vec::new());
    let exotic_url = format!("{}/exotic.tgz", exotic_server.base_url);

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - 'packages/*'\nblockExoticSubdeps: true\n",
    )
    .expect("write pnpm-workspace.yaml");
    append_workspace_yaml_key(&workspace, "minimumReleaseAge", 0);

    write_manifest_value(
        &workspace,
        &json!({
            "private": true,
            "dependencies": {
                "exotic": exotic_url
            }
        }),
    );

    let unnamed_sibling = workspace.join("packages/unnamed-lib");
    fs::create_dir_all(&unnamed_sibling).expect("create unnamed-lib");
    write_manifest_value(
        &unnamed_sibling,
        &json!({
            "version": "1.0.0",
            "main": "index.js",
            "dependencies": {
                "exotic": exotic_url
            }
        }),
    );
    fs::write(unnamed_sibling.join("index.js"), "module.exports = 'unnamed';\n")
        .expect("write unnamed index.js");

    let named_sibling = workspace.join("packages/named-lib");
    fs::create_dir_all(&named_sibling).expect("create named-lib");
    write_manifest_value(
        &named_sibling,
        &json!({
            "name": "named-lib",
            "version": "1.0.0",
            "main": "index.js",
            "dependencies": {
                "exotic": exotic_url
            }
        }),
    );
    fs::write(named_sibling.join("index.js"), "module.exports = 'named';\n")
        .expect("write named index.js");

    let app_dir = workspace.join("packages/app");
    fs::create_dir_all(&app_dir).expect("create app");
    write_manifest_value(
        &app_dir,
        &json!({
            "name": "deployed-app",
            "version": "1.0.0",
            "dependencies": {
                "unnamed-lib": "workspace:../unnamed-lib",
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0"
            }
        }),
    );

    let legacy_app_dir = workspace.join("packages/legacy-app");
    fs::create_dir_all(&legacy_app_dir).expect("create legacy-app");
    write_manifest_value(
        &legacy_app_dir,
        &json!({
            "name": "legacy-app",
            "version": "1.0.0",
            "dependencies": {
                "named-lib": "workspace:*",
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0"
            }
        }),
    );

    pnpm_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let shared_deploy = root.path().join("out-shared");
    pnpm_at(&workspace)
        .with_args(["--filter", "deployed-app", "deploy"])
        .with_arg(&shared_deploy)
        .assert()
        .success();
    assert!(_utils::has_link(&shared_deploy, "unnamed-lib"));

    let legacy_deploy = root.path().join("out-legacy");
    pnpm_at(&workspace)
        .with_args(["--filter", "legacy-app", "deploy", "--legacy"])
        .with_arg(&legacy_deploy)
        .assert()
        .success();
    assert!(_utils::has_link(&legacy_deploy, "named-lib"));
    drop(npmrc_info);
}

#[test]
fn removing_node_download_mirror_invalidates_repeat_install_and_ignore_manifest_check_blocks_phantom_importers()
 {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();

    let node_tarball =
        gzipped_tarball(&[("node-v22.0.0-linux-x64/bin/node", "#!/bin/sh\necho v22.0.0\n")]);
    let node_integrity = {
        use sha2::{
            Digest,
            Sha512,
        };
        let digest = Sha512::digest(&node_tarball);
        format!(
            "sha512-{}",
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, digest),
        )
    };
    let mirror_server =
        TestHttpServer::start(node_tarball, vec![("reg-parent", "1.0.0", json!({}))]);
    let mirror_node_url =
        format!("{}/node/v22.0.0/node-v22.0.0-linux-x64.tar.gz", mirror_server.base_url);

    fs::write(workspace.join(".npmrc"), format!("registry={}/\n", mirror_server.base_url))
        .expect("write .npmrc");
    write_manifest_value(
        &workspace,
        &json!({
            "name": "mirror-app",
            "version": "1.0.0",
            "dependencies": { "reg-parent": "1.0.0" }
        }),
    );
    pnpm_at(&workspace)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    let baseline = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    let reg_parent_integrity = baseline
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("resolution: {integrity: "))
        .and_then(|rest| rest.strip_suffix('}'))
        .expect("extract reg-parent integrity")
        .to_string();

    let runtime_lockfile = format!(
        "lockfileVersion: '9.0'\n\nimporters:\n  .:\n    dependencies:\n      reg-parent:\n        specifier: 1.0.0\n        version: 1.0.0\n\npackages:\n  reg-parent@1.0.0:\n    resolution: {{integrity: {reg_parent_integrity}}}\n  node@runtime:22.0.0:\n    resolution: {{type: binary, archive: tarball, prefix: node-v22.0.0-linux-x64, url: {mirror_node_url}, integrity: {node_integrity}, bin: bin/node}}\n    version: 22.0.0\n\nsnapshots:\n  reg-parent@1.0.0:\n    dependencies:\n      node: runtime:22.0.0\n  node@runtime:22.0.0: {{}}\n",
    );
    fs::write(workspace.join("pnpm-lock.yaml"), runtime_lockfile).expect("write runtime lockfile");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "blockExoticSubdeps: true\nnodeDownloadMirrors:\n  release: '{}/node/'\n",
            mirror_server.base_url,
        ),
    )
    .expect("write mirror config");

    pnpm_at(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    fs::write(workspace.join("pnpm-workspace.yaml"), "blockExoticSubdeps: true\n")
        .expect("remove nodeDownloadMirrors");

    let repeat = pnpm_at(&workspace)
        .with_arg("install")
        .assert()
        .failure();
    let repeat_out = format!(
        "{}\n{}",
        String::from_utf8_lossy(&repeat.get_output().stdout),
        String::from_utf8_lossy(&repeat.get_output().stderr),
    );
    assert!(repeat_out.contains("ERR_PNPM_EXOTIC_SUBDEP"), "{repeat_out}");
    assert!(!repeat_out.contains("Already up to date"), "{repeat_out}");

    let exotic_url = format!("{}/exotic.tgz", mirror_server.base_url);
    let phantom_lockfile = format!(
        "lockfileVersion: '9.0'\n\nimporters:\n  .:\n    dependencies:\n      reg-parent:\n        specifier: 1.0.0\n        version: 1.0.0(exotic@{exotic_url})\n  phantom-importer:\n    dependencies:\n      exotic:\n        specifier: {exotic_url}\n        version: {exotic_url}\n\npackages:\n  reg-parent@1.0.0:\n    resolution: {{integrity: {reg_parent_integrity}}}\n    peerDependencies:\n      exotic: '*'\n  exotic@{exotic_url}:\n    resolution: {{tarball: {exotic_url}, integrity: {node_integrity}}}\n    version: 0.0.0\n\nsnapshots:\n  reg-parent@1.0.0(exotic@{exotic_url}):\n    dependencies:\n      exotic: {exotic_url}\n  exotic@{exotic_url}: {{}}\n",
    );
    fs::write(workspace.join("pnpm-lock.yaml"), phantom_lockfile).expect("write phantom lockfile");
    mirror_server.exotic_hits.store(0, Ordering::SeqCst);

    let ignore_manifest = pnpm_at(&workspace)
        .with_args(["install", "--frozen-lockfile", "--ignore-manifest-check"])
        .with_arg("--cache-dir")
        .with_arg(root.path().join("cache-ignore-manifest"))
        .assert()
        .failure();
    let ignore_out = format!(
        "{}\n{}",
        String::from_utf8_lossy(&ignore_manifest.get_output().stdout),
        String::from_utf8_lossy(&ignore_manifest.get_output().stderr),
    );
    assert!(ignore_out.contains("ERR_PNPM_EXOTIC_SUBDEP"), "{ignore_out}");
    assert_eq!(mirror_server.exotic_hits.load(Ordering::SeqCst), 0);
}

#[test]
fn legacy_deploy_rejects_unmatched_root_importer_when_lockfile_dir_differs() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();

    let exotic_tarball = exotic_tarball_bytes();
    let exotic_integrity = {
        use sha2::{
            Digest,
            Sha512,
        };
        let digest = Sha512::digest(&exotic_tarball);
        format!(
            "sha512-{}",
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, digest),
        )
    };
    let server = TestHttpServer::start(exotic_tarball, vec![("reg-parent", "1.0.0", json!({}))]);
    let exotic_url = format!("{}/exotic.tgz", server.base_url);

    let shared_lock_dir = root.path().join("external-lock-dir");
    fs::create_dir_all(&shared_lock_dir).expect("create external-lock-dir");

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "packages:\n  - 'packages/*'\nblockExoticSubdeps: true\nlockfileDir: '{}'\n",
            shared_lock_dir
                .display()
                .to_string()
                .replace('\\', "/"),
        ),
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join(".npmrc"), format!("registry={}/\n", server.base_url))
        .expect("write .npmrc");
    // Delete root package.json so only packages/app is a discovered workspace project,
    // and NO discovered project maps to shared_lock_dir (".").
    let _ = fs::remove_file(workspace.join("package.json"));

    let app_dir = workspace.join("packages/app");
    fs::create_dir_all(&app_dir).expect("create packages/app");
    write_manifest_value(
        &app_dir,
        &json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "reg-parent": "1.0.0" }
        }),
    );

    pnpm_at(&workspace)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    let baseline =
        fs::read_to_string(shared_lock_dir.join("pnpm-lock.yaml")).expect("read external lockfile");
    let reg_parent_integrity = baseline
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("resolution: {integrity: "))
        .and_then(|rest| rest.strip_suffix('}'))
        .expect("extract reg-parent integrity")
        .to_string();
    let app_importer_id = pnpm_workspace::importer_id_from_root_dir(&shared_lock_dir, &app_dir);

    // Inject a phantom "." importer into shared_lock_dir/pnpm-lock.yaml that directly
    // depends on `exotic` so `reg-parent(exotic@...)` tries to claim peer satisfaction.
    let forged_lockfile = format!(
        "lockfileVersion: '9.0'\n\nimporters:\n  '{app_importer_id}':\n    dependencies:\n      reg-parent:\n        specifier: 1.0.0\n        version: 1.0.0(exotic@{exotic_url})\n  .:\n    dependencies:\n      exotic:\n        specifier: {exotic_url}\n        version: {exotic_url}\n\npackages:\n  reg-parent@1.0.0:\n    resolution: {{integrity: {reg_parent_integrity}}}\n    peerDependencies:\n      exotic: '*'\n  exotic@{exotic_url}:\n    resolution: {{tarball: {exotic_url}, integrity: {exotic_integrity}}}\n    version: 0.0.0\n\nsnapshots:\n  reg-parent@1.0.0(exotic@{exotic_url}):\n    dependencies:\n      exotic: {exotic_url}\n  exotic@{exotic_url}: {{}}\n",
    );
    fs::write(shared_lock_dir.join("pnpm-lock.yaml"), forged_lockfile)
        .expect("write forged external lockfile");
    server.exotic_hits.store(0, Ordering::SeqCst);

    let out_dir = root.path().join("deploy-out");
    let deploy_res = pnpm_at(&workspace)
        .with_args(["--filter", "app", "deploy", "--legacy"])
        .with_arg(&out_dir)
        .assert()
        .failure();
    let deploy_out = format!(
        "{}\n{}",
        String::from_utf8_lossy(&deploy_res.get_output().stdout),
        String::from_utf8_lossy(&deploy_res.get_output().stderr),
    );
    assert!(deploy_out.contains("ERR_PNPM_EXOTIC_SUBDEP"), "{deploy_out}");
    assert_eq!(server.exotic_hits.load(Ordering::SeqCst), 0);
}
