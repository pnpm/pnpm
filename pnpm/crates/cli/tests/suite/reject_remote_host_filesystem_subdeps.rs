//! Non-local packages (registry, remote tarball, git) must not pull host
//! directories or tarballs into `node_modules` via transitive `file:`, `link:`,
//! or path-based `workspace:` dependencies, and tampered lockfiles with
//! unauthorized local non-importer entries must fail verification.

use std::{
    fs,
    io::Write as _,
    path::Path,
    process::Command,
};

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use flate2::{
    Compression,
    write::GzEncoder,
};
use pnpm_testing_utils::{
    bin::CommandTempCwd,
    command_env::CommandTestExt,
    git_repo::GitRepoFixture,
};
use serde_json::{
    Value,
    json,
};
use walkdir::WalkDir;

pub(super) fn pnpm_cmd(workspace: &Path, home_dir: &Path, auth_file: &Path) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find pnpm binary")
        .with_current_dir(workspace)
        .without_ambient_pnpm_config()
        .with_env("HOME", home_dir)
        .with_env("USERPROFILE", home_dir)
        .with_env("PNPM_CONFIG_NPMRC_AUTH_FILE", auth_file)
}

pub(super) fn setup_host_secrets(root: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let home = root.join("fake-home");
    let gh_dir = home.join(".config/gh");
    fs::create_dir_all(&gh_dir).expect("create fake ~/.config/gh");
    fs::write(gh_dir.join("hosts.yml"), "oauth_token: gho_FAKE_TEST_SECRET\n")
        .expect("write hosts.yml");

    let sibling_secret = root.join("secret");
    fs::create_dir_all(&sibling_secret).expect("create ../secret");
    fs::write(sibling_secret.join("credentials.txt"), "top_secret_value\n")
        .expect("write credentials.txt");

    let auth_file = root.join("empty-npmrc");
    fs::write(&auth_file, "").expect("write empty auth file");
    (home, auth_file)
}

pub(super) fn build_tgz(manifest: &Value) -> (Vec<u8>, String) {
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    {
        let mut tar = tar::Builder::new(&mut gz);
        let pkg_bytes = manifest.to_string().into_bytes();
        let mut header = tar::Header::new_gnu();
        header.set_size(pkg_bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, "package/package.json", pkg_bytes.as_slice())
            .expect("append package.json");
        let index_bytes = b"module.exports = 'remote-pkg'\n";
        let mut idx_header = tar::Header::new_gnu();
        idx_header.set_size(index_bytes.len() as u64);
        idx_header.set_mode(0o644);
        idx_header.set_cksum();
        tar.append_data(&mut idx_header, "package/index.js", &index_bytes[..])
            .expect("append index.js");
        tar.finish().expect("finish tar");
    }
    gz.flush().expect("flush gzip");
    let bytes = gz.finish().expect("finish gzip");
    let integrity = ssri::IntegrityOpts::new()
        .algorithm(ssri::Algorithm::Sha512)
        .chain(&bytes)
        .result()
        .to_string();
    (bytes, integrity)
}

pub(super) fn serve_remote_pkg(
    server: &mut mockito::Server,
    name: &str,
    version: &str,
    manifest: &Value,
) -> Vec<mockito::Mock> {
    let (tgz_bytes, integrity) = build_tgz(manifest);
    let tarball_path = format!("/{name}-{version}.tgz");
    let tarball_url = format!("{}{tarball_path}", server.url());

    let mut version_doc = manifest.clone();
    version_doc["dist"] = json!({
        "tarball": tarball_url,
        "integrity": integrity,
    });
    let packument = json!({
        "name": name,
        "dist-tags": { "latest": version },
        "versions": { version: version_doc },
    })
    .to_string();

    let meta_mock = server
        .mock("GET", format!("/{name}").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument)
        .create();
    let tgz_mock = server
        .mock("GET", tarball_path.as_str())
        .with_status(200)
        .with_header("content-type", "application/octet-stream")
        .with_body(tgz_bytes)
        .create();
    vec![meta_mock, tgz_mock]
}

fn assert_no_leaked_host_files(workspace: &Path) {
    let nm = workspace.join("node_modules");
    if !nm.exists() {
        return;
    }
    for entry in walk_node_modules(&nm, Links::Inspect) {
        if entry.path_is_symlink() {
            let target = fs::read_link(entry.path())
                .unwrap_or_else(|err| panic!("read link {:?}: {err}", entry.path()));
            let target_str = target.to_string_lossy();
            assert!(
                !target_str.contains(".config/gh") && !target_str.contains("secret"),
                "host directory symlink leaked into node_modules at {:?} -> {:?}",
                entry.path(),
                target,
            );
        }
    }
    for entry in walk_node_modules(&nm, Links::Follow) {
        let name = entry.file_name().to_string_lossy();
        assert!(
            name != "hosts.yml" && name != "credentials.txt",
            "host file leaked into node_modules at {:?}",
            entry.path(),
        );
    }
}

/// Every entry under `nm`. A dangling link is skipped, since a rejected
/// install can leave one behind. Any other error fails the test rather than
/// hiding the subtree below it.
#[derive(Clone, Copy)]
enum Links {
    /// Yield each symlink as an entry of its own.
    Inspect,
    /// Descend into what each symlink points at.
    Follow,
}

fn walk_node_modules(nm: &Path, links: Links) -> Vec<walkdir::DirEntry> {
    WalkDir::new(nm)
        .follow_links(match links {
            Links::Inspect => false,
            Links::Follow => true,
        })
        .into_iter()
        .filter_map(|entry| match entry {
            Ok(entry) => Some(entry),
            Err(err)
                if err
                    .io_error()
                    .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) =>
            {
                None
            }
            Err(err) => panic!("cannot inspect {nm:?}: {err}"),
        })
        .collect()
}

#[test]
fn rejects_registry_package_with_host_filesystem_subdependencies() {
    for (field, alias, bad_spec) in [
        ("dependencies", "host-gh", "file:~/.config/gh"),
        ("dependencies", "host-etc", "file:/etc"),
        ("dependencies", "host-parent", "file:../secret"),
        ("dependencies", "host-link", "link:~/.config/gh"),
        ("dependencies", "host-ws-path", "workspace:../secret"),
        ("dependencies", "host-git-file", "git+file:///tmp/repo"),
        ("dependencies", "host-alias", "npm:inner@file:~/.config/gh"),
        ("optionalDependencies", "host-opt", "file:~/.config/gh"),
        ("peerDependencies", "host-peer", "file:~/.config/gh"),
    ] {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        let (home, auth_file) = setup_host_secrets(root.path());
        let mut server = mockito::Server::new();
        let remote_manifest = json!({
            "name": "remote-pkg",
            "version": "1.0.0",
            field: { alias: bad_spec },
        });
        let _mocks = serve_remote_pkg(&mut server, "remote-pkg", "1.0.0", &remote_manifest);
        let _ws_404 = server
            .mock("GET", "/ws-pkg")
            .with_status(404)
            .create();

        fs::write(workspace.join(".npmrc"), format!("registry={}/\n", server.url()))
            .expect("write .npmrc");
        fs::write(
            workspace.join("pnpm-workspace.yaml"),
            "blockExoticSubdeps: true\nstrictDepBuilds: true\nautoInstallPeers: true\noverrides:\n  other-parent>host-parent: file:../secret\n",
        )
        .expect("write pnpm-workspace.yaml");
        fs::write(
            workspace.join("package.json"),
            json!({
                "name": "app",
                "version": "1.0.0",
                "dependencies": { "remote-pkg": "1.0.0" },
            })
            .to_string(),
        )
        .expect("write package.json");

        let output = pnpm_cmd(&workspace, &home, &auth_file)
            .with_arg("install")
            .output()
            .expect("run pnpm install");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(
            !output.status.success(),
            "install must fail for {field}.{alias}={bad_spec}; output:\n{combined}",
        );
        assert!(
            combined.contains("ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED"),
            "expected ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED for {field}.{alias}={bad_spec}; got:\n{combined}",
        );
        assert_no_leaked_host_files(&workspace);
        drop((root, server));
    }
}

#[test]
fn rejects_remote_tarball_and_git_packages_with_host_filesystem_subdependencies() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let (home, auth_file) = setup_host_secrets(root.path());
    let mut server = mockito::Server::new();

    let tarball_manifest = json!({
        "name": "tarball-remote",
        "version": "1.0.0",
        "dependencies": { "host-gh": "file:~/.config/gh" },
    });
    let (tgz_bytes, _) = build_tgz(&tarball_manifest);
    let _tgz_mock = server
        .mock("GET", "/tarball-remote.tgz")
        .with_status(200)
        .with_body(tgz_bytes)
        .create();

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "blockExoticSubdeps: false\nstrictDepBuilds: true\n",
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": {
                "tarball-remote": format!("{}/tarball-remote.tgz", server.url()),
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    let tarball_out = pnpm_cmd(&workspace, &home, &auth_file)
        .with_arg("install")
        .output()
        .expect("run tarball install");
    let tarball_text = format!(
        "{}{}",
        String::from_utf8_lossy(&tarball_out.stdout),
        String::from_utf8_lossy(&tarball_out.stderr),
    );
    assert!(!tarball_out.status.success(), "tarball install must fail:\n{tarball_text}");
    assert!(
        tarball_text.contains("ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED"),
        "expected ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED:\n{tarball_text}",
    );
    assert_no_leaked_host_files(&workspace);

    let git_repo = GitRepoFixture::init(root.path(), "git-remote");
    git_repo.write_file(
        "package.json",
        &json!({
            "name": "git-remote",
            "version": "1.0.0",
            "dependencies": { "host-parent": "file:../secret" },
        })
        .to_string(),
    );
    let commit = git_repo.commit("init");
    let served_git = git_repo.serve_http();
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "git-remote": served_git.git_url_at(&commit) },
        })
        .to_string(),
    )
    .expect("write git package.json");

    let git_out = pnpm_cmd(&workspace, &home, &auth_file)
        .with_arg("install")
        .output()
        .expect("run git install");
    let git_text = format!(
        "{}{}",
        String::from_utf8_lossy(&git_out.stdout),
        String::from_utf8_lossy(&git_out.stderr),
    );
    assert!(!git_out.status.success(), "git install must fail:\n{git_text}");
    assert!(
        git_text.contains("ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED"),
        "expected ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED:\n{git_text}",
    );
    assert_no_leaked_host_files(&workspace);
    drop((root, server));
}

#[test]
fn rejects_crafted_lockfile_with_non_importer_directory_on_frozen_and_warm_install() {
    for (label, lockfile_yaml, expected_code) in [
        (
            "directory-on-semver-key",
            "lockfileVersion: '9.0'\n\nimporters:\n  .:\n    dependencies:\n      remote-pkg:\n        specifier: 1.0.0\n        version: 1.0.0\n\npackages:\n  remote-pkg@1.0.0:\n    resolution: {type: directory, directory: ../secret}\n\nsnapshots:\n  remote-pkg@1.0.0: {}\n",
            "ERR_PNPM_RESOLUTION_SHAPE_MISMATCH",
        ),
        (
            "escaping-file-child-on-remote-snapshot",
            "lockfileVersion: '9.0'\n\nimporters:\n  .:\n    dependencies:\n      remote-pkg:\n        specifier: 1.0.0\n        version: 1.0.0\n\npackages:\n  remote-pkg@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\n  host-secret@file:../secret:\n    resolution: {type: directory, directory: ../secret}\n\nsnapshots:\n  remote-pkg@1.0.0:\n    dependencies:\n      host-secret: file:../secret\n  host-secret@file:../secret: {}\n",
            "ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED",
        ),
        (
            "escaping-link-child-on-remote-snapshot",
            "lockfileVersion: '9.0'\n\nimporters:\n  .:\n    dependencies:\n      remote-pkg:\n        specifier: 1.0.0\n        version: 1.0.0\n\npackages:\n  remote-pkg@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\n\nsnapshots:\n  remote-pkg@1.0.0:\n    dependencies:\n      host-secret: link:../secret\n",
            "ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED",
        ),
    ] {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        let (home, auth_file) = setup_host_secrets(root.path());
        let mut server = mockito::Server::new();
        let benign_manifest = json!({ "name": "remote-pkg", "version": "1.0.0" });
        let _mocks = serve_remote_pkg(&mut server, "remote-pkg", "1.0.0", &benign_manifest);

        fs::write(workspace.join(".npmrc"), format!("registry={}/\n", server.url()))
            .expect("write .npmrc");
        fs::write(workspace.join("pnpm-workspace.yaml"), "strictDepBuilds: true\n")
            .expect("write pnpm-workspace.yaml");
        fs::write(
            workspace.join("package.json"),
            json!({
                "name": "app",
                "version": "1.0.0",
                "dependencies": { "remote-pkg": "1.0.0" },
            })
            .to_string(),
        )
        .expect("write package.json");
        fs::write(workspace.join("pnpm-lock.yaml"), lockfile_yaml).expect("write crafted lockfile");

        for args in [vec!["install", "--frozen-lockfile"], vec!["install"]] {
            let out = pnpm_cmd(&workspace, &home, &auth_file)
                .with_args(&args)
                .output()
                .expect("run pnpm install");
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr),
            );
            assert!(!out.status.success(), "{label} must fail for {args:?}; output:\n{text}");
            assert!(
                text.contains(expected_code),
                "{label} expected {expected_code} for {args:?}; got:\n{text}",
            );
            assert_no_leaked_host_files(&workspace);
        }
        drop((root, server));
    }
}

#[test]
fn preserves_workspace_overrides_and_deep_workspace_links_on_install_and_frozen_lockfile() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let (home, auth_file) = setup_host_secrets(root.path());
    let mut server = mockito::Server::new();

    let remote_manifest = json!({
        "name": "remote-pkg",
        "version": "1.0.0",
        "dependencies": {
            "helper": "^1.0.0",
            "ws-pkg": "^1.0.0",
        },
    });
    let _mocks = serve_remote_pkg(&mut server, "remote-pkg", "1.0.0", &remote_manifest);
    let _ws_404 = server
        .mock("GET", "/ws-pkg")
        .with_status(404)
        .create();

    fs::create_dir_all(workspace.join("local-helper")).expect("create local-helper");
    fs::write(
        workspace.join("local-helper/package.json"),
        json!({ "name": "helper", "version": "1.0.0" }).to_string(),
    )
    .expect("write local-helper/package.json");

    fs::create_dir_all(workspace.join("packages/ws-pkg")).expect("create ws-pkg");
    fs::write(
        workspace.join("packages/ws-pkg/package.json"),
        json!({ "name": "ws-pkg", "version": "1.0.0" }).to_string(),
    )
    .expect("write ws-pkg/package.json");

    fs::write(workspace.join(".npmrc"), format!("registry={}/\n", server.url()))
        .expect("write .npmrc");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nlinkWorkspacePackages: deep\noverrides:\n  remote-pkg>helper: file:./local-helper\n",
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "remote-pkg": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    pnpm_cmd(&workspace, &home, &auth_file)
        .with_arg("install")
        .assert()
        .success();

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pnpm_cmd(&workspace, &home, &auth_file)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    let remote_deps = workspace.join("node_modules/.pnpm/remote-pkg@1.0.0/node_modules");
    for child in ["helper", "ws-pkg"] {
        assert!(
            remote_deps
                .join(child)
                .join("package.json")
                .exists(),
            "{child} must be linked",
        );
    }

    drop((root, server));
}

#[test]
fn pnpmfile_read_package_outside_workspace_rewrite_reinstalls_and_identity_hook_rejects_forgery() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let (home, auth_file) = setup_host_secrets(root.path());
    let mut server = mockito::Server::new();

    let shared_dir = root.path().join("shared-outside");
    fs::create_dir_all(&shared_dir).expect("create shared-outside");
    fs::write(
        shared_dir.join("package.json"),
        json!({ "name": "shared", "version": "1.0.0" }).to_string(),
    )
    .expect("write shared package.json");

    let remote_manifest = json!({
        "name": "remote-pkg",
        "version": "1.0.0",
        "dependencies": { "shared": "^1.0.0" },
    });
    let _mocks = serve_remote_pkg(&mut server, "remote-pkg", "1.0.0", &remote_manifest);
    let _ws_404 = server
        .mock("GET", "/ws-pkg")
        .with_status(404)
        .create();

    fs::write(workspace.join(".npmrc"), format!("registry={}/\n", server.url()))
        .expect("write .npmrc");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { readPackage(pkg) {\n  if (pkg.name === 'remote-pkg') pkg.dependencies.shared = 'file:../shared-outside';\n  return pkg;\n} } };\n",
    )
    .expect("write .pnpmfile.cjs");
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "remote-pkg": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    pnpm_cmd(&workspace, &home, &auth_file)
        .with_arg("install")
        .assert()
        .success();
    pnpm_cmd(&workspace, &home, &auth_file)
        .with_args(["install", "--no-prefer-frozen-lockfile"])
        .assert()
        .success();
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pnpm_cmd(&workspace, &home, &auth_file)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    pnpm_cmd(&workspace, &home, &auth_file)
        .with_arg("ci")
        .assert()
        .success();

    // The recorded outside-workspace child is no longer authorized once the hook stops adding it.
    let identity_pnpmfile = "module.exports = { hooks: { readPackage(pkg) { return pkg; } } };\n";
    fs::write(workspace.join(".pnpmfile.cjs"), identity_pnpmfile).expect("write identity hook");
    let output = pnpm_cmd(&workspace, &home, &auth_file)
        .with_args(["install", "--lockfile-only"])
        .output()
        .expect("run pnpm install --lockfile-only");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(!output.status.success(), "install must fail; output:\n{combined}");
    assert!(
        combined.contains("ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED"),
        "expected ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED; got:\n{combined}",
    );

    drop((root, server));
}

#[test]
fn isolates_hoisted_local_peer_authorizations_by_declaring_parent_and_importer() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let (home, auth_file) = setup_host_secrets(root.path());
    let mut server = mockito::Server::new();

    let peer_dir = root.path().join("shared-peer");
    fs::create_dir_all(&peer_dir).expect("create shared-peer");
    fs::write(
        peer_dir.join("package.json"),
        json!({ "name": "peer-x", "version": "1.0.0" }).to_string(),
    )
    .expect("write peer-x");

    let peer_spec = format!("file:{}", peer_dir.to_string_lossy().replace('\\', "/"));

    let trusted_manifest = json!({
        "name": "trusted-parent",
        "version": "1.0.0",
    });
    let evil_manifest = json!({
        "name": "evil-parent",
        "version": "1.0.0",
        "peerDependencies": { "peer-x": peer_spec },
    });
    let _m1 = serve_remote_pkg(&mut server, "trusted-parent", "1.0.0", &trusted_manifest);
    let _m2 = serve_remote_pkg(&mut server, "evil-parent", "1.0.0", &evil_manifest);

    // 1. Scoped `packageExtensions` on `trusted-parent` must NOT authorize `evil-parent`'s peer.
    fs::write(workspace.join(".npmrc"), format!("registry={}/\n", server.url()))
        .expect("write .npmrc");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "autoInstallPeers: true\npackageExtensions:\n  trusted-parent:\n    peerDependencies:\n      peer-x: '{peer_spec}'\n",
        ),
    )
    .expect("write workspace yaml");
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "trusted-parent": "1.0.0", "evil-parent": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    let out = pnpm_cmd(&workspace, &home, &auth_file)
        .with_arg("install")
        .output()
        .expect("run install");
    assert!(!out.status.success(), "evil-parent must not piggyback on trusted-parent extension");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED"), "stderr: {stderr}");

    // 2. Local package peer in importer A must NOT authorize `evil-b` in importer B.
    let local_lib = workspace.join("local-lib");
    fs::create_dir_all(&local_lib).expect("create local-lib");
    fs::write(
        local_lib.join("package.json"),
        json!({
            "name": "local-lib",
            "version": "1.0.0",
            "peerDependencies": { "peer-x": peer_spec },
        })
        .to_string(),
    )
    .expect("write local-lib");
    fs::create_dir_all(workspace.join("packages/imp-a")).expect("create imp-a");
    fs::write(
        workspace.join("packages/imp-a/package.json"),
        json!({
            "name": "imp-a",
            "version": "1.0.0",
            "dependencies": { "local-lib": "file:../../local-lib" },
        })
        .to_string(),
    )
    .expect("write imp-a");
    fs::create_dir_all(workspace.join("packages/imp-b")).expect("create imp-b");
    let evil_b_manifest = json!({
        "name": "evil-b",
        "version": "1.0.0",
        "peerDependencies": { "peer-x": peer_spec },
    });
    let _m3 = serve_remote_pkg(&mut server, "evil-b", "1.0.0", &evil_b_manifest);
    fs::write(
        workspace.join("packages/imp-b/package.json"),
        json!({
            "name": "imp-b",
            "version": "1.0.0",
            "dependencies": { "evil-b": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write imp-b");
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "root", "private": true }).to_string(),
    )
    .expect("write root manifest");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nautoInstallPeers: true\n",
    )
    .expect("write workspace yaml");

    let out_ws = pnpm_cmd(&workspace, &home, &auth_file)
        .with_arg("install")
        .output()
        .expect("run workspace install");
    assert!(!out_ws.status.success(), "importer A local peer must not authorize importer B");
    let stderr_ws = String::from_utf8_lossy(&out_ws.stderr);
    assert!(stderr_ws.contains("ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED"), "stderr: {stderr_ws}");

    drop((root, server));
}

#[test]
fn optional_local_peer_declarer_must_authorize_a_hoisted_peer() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let (home, auth_file) = setup_host_secrets(root.path());
    let mut server = mockito::Server::new();

    let peer_dir = root.path().join("shared-peer");
    fs::create_dir_all(&peer_dir).expect("create shared-peer");
    fs::write(
        peer_dir.join("package.json"),
        json!({ "name": "peer-x", "version": "1.0.0" }).to_string(),
    )
    .expect("write peer-x");
    let peer_spec = format!("file:{}", peer_dir.to_string_lossy().replace('\\', "/"));

    let trusted_manifest = json!({ "name": "trusted-parent", "version": "1.0.0" });
    let evil_manifest = json!({
        "name": "evil-parent",
        "version": "1.0.0",
        "peerDependencies": { "peer-x": peer_spec },
        "peerDependenciesMeta": { "peer-x": { "optional": true } },
    });
    let _m1 = serve_remote_pkg(&mut server, "trusted-parent", "1.0.0", &trusted_manifest);
    let _m2 = serve_remote_pkg(&mut server, "evil-parent", "1.0.0", &evil_manifest);

    fs::write(workspace.join(".npmrc"), format!("registry={}/\n", server.url()))
        .expect("write .npmrc");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "autoInstallPeers: true\npackageExtensions:\n  trusted-parent:\n    peerDependencies:\n      peer-x: '{peer_spec}'\n",
        ),
    )
    .expect("write workspace yaml");
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "trusted-parent": "1.0.0", "evil-parent": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    let out = pnpm_cmd(&workspace, &home, &auth_file)
        .with_arg("install")
        .output()
        .expect("run install");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "the optional declarer must not share the hoisted peer");
    assert!(stderr.contains("ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED"), "stderr: {stderr}");

    drop((root, server));
}
