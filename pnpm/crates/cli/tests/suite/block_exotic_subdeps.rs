//! Enforcing `blockExoticSubdeps` on `--frozen-lockfile` and warm-lockfile installs.

mod deploy_and_mirrors;
mod http_server;

use crate::_utils::{
    self,
    append_workspace_yaml_key,
    gzipped_tarball,
    write_manifest_value,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use http_server::TestHttpServer;
use pnpm_testing_utils_registry::{
    bin::CommandTempCwd,
    git_repo::GitRepoFixture,
};
use serde_json::json;
use std::{
    fs,
    path::Path,
    process::Command,
    sync::atomic::Ordering,
};

fn pnpm_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find pnpm binary").with_current_dir(workspace)
}

fn exotic_tarball_bytes() -> Vec<u8> {
    gzipped_tarball(&[
        ("package/package.json", r#"{"name":"exotic","version":"0.0.0","main":"index.js"}"#),
        ("package/index.js", "module.exports = 'EXOTIC_PAYLOAD';\n"),
    ])
}

#[test]
fn frozen_and_warm_installs_reject_url_tarball_subdep_without_fetching() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();

    let exotic_server = TestHttpServer::start(exotic_tarball_bytes(), Vec::new());
    let exotic_url = format!("{}/exotic.tgz", exotic_server.base_url);
    let registry_server = TestHttpServer::start(
        Vec::new(),
        vec![
            ("reg-parent", "1.0.0", json!({ "exotic": exotic_url })),
            ("safe-pkg", "1.0.0", json!({})),
        ],
    );

    fs::write(workspace.join(".npmrc"), format!("registry={}/\n", registry_server.base_url))
        .expect("write .npmrc");
    fs::write(workspace.join("pnpm-workspace.yaml"), "blockExoticSubdeps: false\n")
        .expect("write pnpm-workspace.yaml");
    write_manifest_value(
        &workspace,
        &json!({
            "name": "repro-app",
            "version": "1.0.0",
            "dependencies": {
                "reg-parent": "1.0.0"
            }
        }),
    );

    pnpm_at(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert!(
        workspace.join("node_modules/.pnpm").exists(),
        "initial install with blockExoticSubdeps: false must materialize node_modules",
    );

    fs::write(workspace.join("pnpm-workspace.yaml"), "").expect("clear pnpm-workspace.yaml");

    let hot_repeat = pnpm_at(&workspace)
        .with_arg("install")
        .assert()
        .failure();
    let hot_stderr = String::from_utf8_lossy(&hot_repeat.get_output().stderr);
    let hot_stdout = String::from_utf8_lossy(&hot_repeat.get_output().stdout);
    assert!(
        hot_stderr.contains("ERR_PNPM_EXOTIC_SUBDEP")
            || hot_stdout.contains("ERR_PNPM_EXOTIC_SUBDEP"),
        "hot repeat install must fail with ERR_PNPM_EXOTIC_SUBDEP:\n{hot_stdout}\n{hot_stderr}",
    );
    assert!(
        !hot_stdout.contains("Already up to date"),
        "hot repeat install must not report Already up to date:\n{hot_stdout}",
    );

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    exotic_server.exotic_hits.store(0, Ordering::SeqCst);
    let cold_store = root.path().join("cold-store");
    let cold_cache = root.path().join("cold-cache");

    let frozen = pnpm_at(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .with_arg("--store-dir")
        .with_arg(&cold_store)
        .with_arg("--cache-dir")
        .with_arg(&cold_cache)
        .assert()
        .failure();
    let frozen_out = format!(
        "{}\n{}",
        String::from_utf8_lossy(&frozen.get_output().stdout),
        String::from_utf8_lossy(&frozen.get_output().stderr),
    );
    assert!(frozen_out.contains("ERR_PNPM_EXOTIC_SUBDEP"), "{frozen_out}");
    assert_eq!(
        exotic_server.exotic_hits.load(Ordering::SeqCst),
        0,
        "exotic.tgz must never be fetched when --frozen-lockfile rejects it",
    );
    assert!(!workspace.join("node_modules/reg-parent").exists());

    let warm = pnpm_at(&workspace)
        .with_arg("install")
        .with_arg("--store-dir")
        .with_arg(&cold_store)
        .with_arg("--cache-dir")
        .with_arg(&cold_cache)
        .assert()
        .failure();
    let warm_out = format!(
        "{}\n{}",
        String::from_utf8_lossy(&warm.get_output().stdout),
        String::from_utf8_lossy(&warm.get_output().stderr),
    );
    assert!(warm_out.contains("ERR_PNPM_EXOTIC_SUBDEP"), "{warm_out}");
    assert_eq!(
        exotic_server.exotic_hits.load(Ordering::SeqCst),
        0,
        "exotic.tgz must never be fetched when warm install rejects it",
    );

    pnpm_at(&workspace)
        .with_args(["install", "--frozen-lockfile", "--trust-lockfile"])
        .with_arg("--store-dir")
        .with_arg(&cold_store)
        .with_arg("--cache-dir")
        .with_arg(&cold_cache)
        .assert()
        .success();

    let after_trust = pnpm_at(&workspace)
        .with_arg("install")
        .with_arg("--store-dir")
        .with_arg(&cold_store)
        .with_arg("--cache-dir")
        .with_arg(&cold_cache)
        .assert()
        .failure();
    let after_trust_out = format!(
        "{}\n{}",
        String::from_utf8_lossy(&after_trust.get_output().stdout),
        String::from_utf8_lossy(&after_trust.get_output().stderr),
    );
    assert!(after_trust_out.contains("ERR_PNPM_EXOTIC_SUBDEP"), "{after_trust_out}");
}

#[test]
fn frozen_and_warm_installs_reject_git_subdep() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();

    let git_repo = GitRepoFixture::init(root.path(), "exotic-git");
    git_repo.write_file(
        "package.json",
        r#"{"name":"exotic-git","version":"1.0.0","main":"index.js"}"#,
    );
    git_repo.write_file("index.js", "module.exports = 'git';\n");
    let _commit = git_repo.commit("init");
    let served_git = git_repo.serve_http();
    let git_spec = served_git.git_url_at("main");

    let registry_server = TestHttpServer::start(
        Vec::new(),
        vec![("git-parent", "1.0.0", json!({ "exotic-git": git_spec }))],
    );

    fs::write(workspace.join(".npmrc"), format!("registry={}/\n", registry_server.base_url))
        .expect("write .npmrc");
    fs::write(workspace.join("pnpm-workspace.yaml"), "blockExoticSubdeps: false\n")
        .expect("write pnpm-workspace.yaml");
    write_manifest_value(
        &workspace,
        &json!({
            "name": "git-subdep-app",
            "version": "1.0.0",
            "dependencies": {
                "git-parent": "1.0.0"
            }
        }),
    );

    pnpm_at(&workspace)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();

    fs::write(workspace.join("pnpm-workspace.yaml"), "blockExoticSubdeps: true\n")
        .expect("enable blockExoticSubdeps");

    for args in [vec!["install", "--frozen-lockfile"], vec!["install"]] {
        let run = pnpm_at(&workspace)
            .with_args(&args)
            .assert()
            .failure();
        let combined = format!(
            "{}\n{}",
            String::from_utf8_lossy(&run.get_output().stdout),
            String::from_utf8_lossy(&run.get_output().stderr),
        );
        assert!(combined.contains("ERR_PNPM_EXOTIC_SUBDEP"), "args={args:?}\n{combined}");
    }
}

#[test]
fn direct_importer_workspace_and_overridden_deps_pass_frozen_lockfile() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let exotic_server = TestHttpServer::start(exotic_tarball_bytes(), Vec::new());
    let exotic_url = format!("{}/exotic.tgz", exotic_server.base_url);

    let git_repo = GitRepoFixture::init(root.path(), "direct-git");
    git_repo.write_file(
        "package.json",
        r#"{"name":"direct-git","version":"1.0.0","main":"index.js"}"#,
    );
    git_repo.write_file("index.js", "module.exports = 'direct-git';\n");
    let _commit = git_repo.commit("init");
    let git_spec = git_repo.git_url_at("main");

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - 'packages/*'\nblockExoticSubdeps: true\ninjectWorkspacePackages: true\noverrides:\n  '@pnpm.e2e/pkg-with-1-dep>@pnpm.e2e/dep-of-pkg-with-1-dep': '100.1.0'\n",
    )
    .expect("write pnpm-workspace.yaml");
    append_workspace_yaml_key(&workspace, "minimumReleaseAge", 0);

    let pkg_a = workspace.join("packages/pkg-a");
    fs::create_dir_all(&pkg_a).expect("create packages/pkg-a");
    write_manifest_value(
        &pkg_a,
        &json!({
            "name": "pkg-a",
            "version": "1.0.0",
            "dependencies": {
                "exotic": exotic_url
            }
        }),
    );

    write_manifest_value(
        &workspace,
        &json!({
            "name": "root-app",
            "version": "1.0.0",
            "dependencies": {
                "direct-git": git_spec,
                "pkg-a": "workspace:*",
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0"
            }
        }),
    );

    pnpm_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove root node_modules");
    let _ = fs::remove_dir_all(pkg_a.join("node_modules"));

    pnpm_at(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    assert!(_utils::has_link(&workspace, "direct-git"));
    assert!(_utils::has_link(&workspace, "pkg-a"));

    let deploy_dir = root.path().join("deployed-root");
    pnpm_at(&workspace)
        .with_args(["--filter", "root-app", "deploy"])
        .with_arg(&deploy_dir)
        .assert()
        .success();
    assert!(_utils::has_link(&deploy_dir, "pkg-a"));

    let legacy_deploy_dir = root.path().join("deployed-root-legacy");
    pnpm_at(&workspace)
        .with_args(["--filter", "root-app", "deploy", "--legacy"])
        .with_arg(&legacy_deploy_dir)
        .assert()
        .success();
    assert!(_utils::has_link(&legacy_deploy_dir, "pkg-a"));
    drop(npmrc_info);
}

#[test]
fn frozen_and_warm_installs_reject_forged_runtime_binary_and_phantom_importer_peer() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();

    let exotic_server = TestHttpServer::start(exotic_tarball_bytes(), Vec::new());
    let exotic_url = format!("{}/exotic.tgz", exotic_server.base_url);
    let exotic_integrity = {
        use sha2::{
            Digest,
            Sha512,
        };
        let digest = Sha512::digest(exotic_tarball_bytes());
        format!(
            "sha512-{}",
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, digest),
        )
    };
    let registry_server =
        TestHttpServer::start(Vec::new(), vec![("reg-parent", "1.0.0", json!({}))]);

    fs::write(workspace.join(".npmrc"), format!("registry={}/\n", registry_server.base_url))
        .expect("write .npmrc");
    write_manifest_value(
        &workspace,
        &json!({
            "name": "hardening-app",
            "version": "1.0.0",
            "dependencies": { "reg-parent": "1.0.0" }
        }),
    );
    pnpm_at(&workspace)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    let baseline_lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read baseline lockfile");
    let reg_parent_integrity = baseline_lockfile
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("resolution: {integrity: "))
        .and_then(|rest| rest.strip_suffix('}'))
        .expect("extract reg-parent integrity")
        .to_string();

    let forged_runtime_lockfile = format!(
        "lockfileVersion: '9.0'\n\nimporters:\n  .:\n    dependencies:\n      reg-parent:\n        specifier: 1.0.0\n        version: 1.0.0\n\npackages:\n  reg-parent@1.0.0:\n    resolution: {{integrity: {reg_parent_integrity}}}\n  node@runtime:22.0.0:\n    resolution: {{type: binary, archive: tarball, url: {exotic_url}, integrity: {exotic_integrity}, bin: index.js}}\n    version: 22.0.0\n\nsnapshots:\n  reg-parent@1.0.0:\n    dependencies:\n      node: runtime:22.0.0\n  node@runtime:22.0.0: {{}}\n",
    );
    fs::write(workspace.join("pnpm-lock.yaml"), forged_runtime_lockfile)
        .expect("write forged runtime lockfile");
    exotic_server.exotic_hits.store(0, Ordering::SeqCst);

    for args in [vec!["install", "--frozen-lockfile"], vec!["install"]] {
        let run = pnpm_at(&workspace)
            .with_args(&args)
            .with_arg("--cache-dir")
            .with_arg(root.path().join("cache-runtime"))
            .assert()
            .failure();
        let out = format!(
            "{}\n{}",
            String::from_utf8_lossy(&run.get_output().stdout),
            String::from_utf8_lossy(&run.get_output().stderr),
        );
        assert!(out.contains("ERR_PNPM_EXOTIC_SUBDEP"), "args={args:?}\n{out}");
        assert_eq!(exotic_server.exotic_hits.load(Ordering::SeqCst), 0);
    }

    let forged_phantom_peer_lockfile = format!(
        "lockfileVersion: '9.0'\n\nimporters:\n  .:\n    dependencies:\n      reg-parent:\n        specifier: 1.0.0\n        version: 1.0.0(exotic@{exotic_url})\n  ghost-pkg:\n    dependencies:\n      exotic:\n        specifier: {exotic_url}\n        version: {exotic_url}\n\npackages:\n  reg-parent@1.0.0:\n    resolution: {{integrity: {reg_parent_integrity}}}\n    peerDependencies:\n      exotic: '*'\n  exotic@{exotic_url}:\n    resolution: {{tarball: {exotic_url}, integrity: {exotic_integrity}}}\n    version: 0.0.0\n\nsnapshots:\n  reg-parent@1.0.0(exotic@{exotic_url}):\n    dependencies:\n      exotic: {exotic_url}\n  exotic@{exotic_url}: {{}}\n  ghost-snap@1.0.0: {{}}\n",
    );
    fs::write(workspace.join("pnpm-lock.yaml"), forged_phantom_peer_lockfile)
        .expect("write forged phantom importer lockfile");

    for args in [vec!["install", "--frozen-lockfile"], vec!["install"]] {
        let run = pnpm_at(&workspace)
            .with_args(&args)
            .with_arg("--cache-dir")
            .with_arg(root.path().join("cache-phantom"))
            .assert()
            .failure();
        let out = format!(
            "{}\n{}",
            String::from_utf8_lossy(&run.get_output().stdout),
            String::from_utf8_lossy(&run.get_output().stderr),
        );
        assert!(out.contains("ERR_PNPM_EXOTIC_SUBDEP"), "args={args:?}\n{out}");
        assert_eq!(exotic_server.exotic_hits.load(Ordering::SeqCst), 0);
    }
}
