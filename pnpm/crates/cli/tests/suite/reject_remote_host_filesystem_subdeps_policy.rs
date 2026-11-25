use std::fs;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils_registry::bin::CommandTempCwd;
use serde_json::json;

use super::reject_remote_host_filesystem_subdeps::{
    build_tgz,
    pnpm_cmd,
    serve_remote_pkg,
    setup_host_secrets,
};

#[test]
fn equivalent_local_override_reinstalls_with_an_offline_frozen_lockfile() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let (home, auth_file) = setup_host_secrets(root.path());
    let mut server = mockito::Server::new();
    let manifest = json!({
        "name": "remote-pkg",
        "version": "1.0.0",
        "dependencies": { "helper": "^1.0.0" },
    });
    let _package = serve_remote_pkg(&mut server, "remote-pkg", "1.0.0", &manifest);
    let app = workspace.join("app");
    let helper = workspace.join("helper");
    fs::create_dir(&app).unwrap();
    fs::create_dir(&helper).unwrap();
    fs::write(helper.join("package.json"), r#"{"name":"helper","version":"1.0.0"}"#).unwrap();
    fs::write(
        app.join("package.json"),
        r#"{"dependencies":{"remote-pkg":"1.0.0","helper":"file:../helper"}}"#,
    )
    .unwrap();
    fs::write(app.join(".npmrc"), format!("registry={}/\n", server.url())).unwrap();
    fs::write(
        app.join("pnpm-workspace.yaml"),
        "overrides:\n  helper: file:../../workspace/helper\n",
    )
    .unwrap();

    pnpm_cmd(&app, &home, &auth_file)
        .with_arg("install")
        .assert()
        .success();
    pnpm_cmd(&app, &home, &auth_file)
        .with_args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
}

#[test]
fn ranged_override_reinstalls_on_frozen_and_warm_paths_while_rejecting_non_matching_declared_range()
{
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let (home, auth_file) = setup_host_secrets(root.path());
    let mut server = mockito::Server::new();

    let helper_dir = workspace.join("local-helper");
    fs::create_dir_all(&helper_dir).expect("create local-helper");
    fs::write(
        helper_dir.join("package.json"),
        json!({ "name": "helper", "version": "1.0.0" }).to_string(),
    )
    .expect("write local-helper manifest");

    let matching_pkg = json!({
        "name": "remote-pkg",
        "version": "1.0.0",
        "dependencies": { "helper": "^1.0.0" },
    });
    let non_matching_pkg = json!({
        "name": "other-pkg",
        "version": "1.0.0",
        "dependencies": { "helper": "^2.0.0" },
    });
    let _m1 = serve_remote_pkg(&mut server, "remote-pkg", "1.0.0", &matching_pkg);
    let _m2 = serve_remote_pkg(&mut server, "other-pkg", "1.0.0", &non_matching_pkg);

    fs::write(workspace.join(".npmrc"), format!("registry={}/\n", server.url()))
        .expect("write .npmrc");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "overrides:\n  'remote-pkg>helper@^1.0.0': file:./local-helper\n",
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
    pnpm_cmd(&workspace, &home, &auth_file)
        .with_arg("install")
        .assert()
        .success();
    pnpm_cmd(&workspace, &home, &auth_file)
        .with_arg("ci")
        .assert()
        .success();

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "overrides:\n  'other-pkg>helper@^1.0.0': file:./local-helper\n",
    )
    .expect("update pnpm-workspace.yaml");
    let lockfile_yaml =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    let forged = lockfile_yaml.replace("remote-pkg", "other-pkg");
    fs::write(workspace.join("pnpm-lock.yaml"), forged).expect("write forged lockfile");
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "other-pkg": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write updated manifest");

    let out = pnpm_cmd(&workspace, &home, &auth_file)
        .with_args(["install", "--frozen-lockfile"])
        .output()
        .expect("run forged frozen install");
    assert!(!out.status.success(), "declared ^2.0.0 must not match helper@^1.0.0 override");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED"), "stderr: {stderr}");

    drop((root, server));
}

#[test]
fn range_conditional_pnpmfile_read_package_hook_reinstalls_under_frozen_lockfile() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let (home, auth_file) = setup_host_secrets(root.path());
    let mut server = mockito::Server::new();

    let shared_dir = root.path().join("shared-outside");
    fs::create_dir_all(&shared_dir).expect("create shared-outside");
    fs::write(
        shared_dir.join("package.json"),
        json!({ "name": "shared", "version": "1.0.0" }).to_string(),
    )
    .expect("write shared manifest");

    let remote_manifest = json!({
        "name": "remote-pkg",
        "version": "1.0.0",
        "dependencies": { "shared": "^1.0.0" },
    });
    let _mocks = serve_remote_pkg(&mut server, "remote-pkg", "1.0.0", &remote_manifest);

    fs::write(workspace.join(".npmrc"), format!("registry={}/\n", server.url()))
        .expect("write .npmrc");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { readPackage(pkg) {\n  if (pkg.name === 'remote-pkg' && pkg.dependencies && pkg.dependencies.shared === '^1.0.0') {\n    pkg.dependencies.shared = 'link:../shared-outside';\n  }\n  return pkg;\n} } };\n",
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

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pnpm_cmd(&workspace, &home, &auth_file)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    drop((root, server));
}

#[test]
fn hook_keyed_on_a_package_extension_reinstalls_with_pnpm_ci() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let (home, auth_file) = setup_host_secrets(root.path());
    let mut server = mockito::Server::new();

    let shared_dir = root.path().join("shared-outside");
    fs::create_dir_all(&shared_dir).expect("create shared-outside");
    fs::write(
        shared_dir.join("package.json"),
        json!({ "name": "shared", "version": "1.0.0" }).to_string(),
    )
    .expect("write shared manifest");

    let remote_manifest = json!({ "name": "remote-pkg", "version": "1.0.0" });
    let _mocks = serve_remote_pkg(&mut server, "remote-pkg", "1.0.0", &remote_manifest);

    fs::write(workspace.join(".npmrc"), format!("registry={}/\n", server.url()))
        .expect("write .npmrc");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packageExtensions:\n  remote-pkg:\n    dependencies:\n      shared: ^1.0.0\n",
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { readPackage(pkg) {\n  if (pkg.name === 'remote-pkg' && pkg.dependencies && pkg.dependencies.shared === '^1.0.0') {\n    pkg.dependencies.shared = 'link:../shared-outside';\n  }\n  return pkg;\n} } };\n",
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
        .with_arg("ci")
        .assert()
        .success();

    drop((root, server));
}

#[test]
fn absent_workspace_project_with_outside_file_dep_does_not_block_frozen_install_and_removed_local_dep_stops_authorizing_peer()
 {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let (home, auth_file) = setup_host_secrets(root.path());
    let mut server = mockito::Server::new();

    let outside_vendor = root.path().join("outside-vendor");
    fs::create_dir_all(&outside_vendor).expect("create outside-vendor");
    fs::write(
        outside_vendor.join("package.json"),
        json!({ "name": "outside-vendor", "version": "1.0.0" }).to_string(),
    )
    .expect("write outside-vendor");

    let ok_pkg = json!({ "name": "ok-pkg", "version": "1.0.0" });
    let _m1 = serve_remote_pkg(&mut server, "ok-pkg", "1.0.0", &ok_pkg);

    fs::write(workspace.join(".npmrc"), format!("registry={}/\n", server.url()))
        .expect("write .npmrc");
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    fs::create_dir_all(workspace.join("packages/app")).expect("create packages/app");
    fs::write(
        workspace.join("packages/app/package.json"),
        json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "ok-pkg": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write app manifest");
    fs::create_dir_all(workspace.join("packages/unused")).expect("create packages/unused");
    fs::write(
        workspace.join("packages/unused/package.json"),
        json!({
            "name": "unused",
            "version": "1.0.0",
            "dependencies": { "outside-vendor": "file:../../../outside-vendor" },
        })
        .to_string(),
    )
    .expect("write unused manifest");

    pnpm_cmd(&workspace, &home, &auth_file)
        .with_arg("install")
        .assert()
        .success();

    // Simulate a Docker build context omitting `packages/unused`.
    fs::remove_dir_all(workspace.join("packages/unused")).expect("omit unused project");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove root node_modules");
    pnpm_cmd(&workspace, &home, &auth_file)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    // Removing a direct local dependency from manifest stops authorizing its local peer on warm install.
    let peer_target = root.path().join("peer-target");
    fs::create_dir_all(&peer_target).expect("create peer-target");
    fs::write(
        peer_target.join("package.json"),
        json!({ "name": "peer-target", "version": "1.0.0" }).to_string(),
    )
    .expect("write peer-target");
    let local_declarer = workspace.join("local-declarer");
    fs::create_dir_all(&local_declarer).expect("create local-declarer");
    fs::write(
        local_declarer.join("package.json"),
        json!({
            "name": "local-declarer",
            "version": "1.0.0",
            "peerDependencies": { "peer-target": "file:../peer-target" },
        })
        .to_string(),
    )
    .expect("write local-declarer");
    let evil_peer_parent = json!({
        "name": "evil-peer-parent",
        "version": "1.0.0",
        "peerDependencies": { "peer-target": "file:../peer-target" },
    });
    let _m2 = serve_remote_pkg(&mut server, "evil-peer-parent", "1.0.0", &evil_peer_parent);

    let single_proj = root.path().join("single-proj");
    fs::create_dir_all(single_proj.join("local-declarer")).expect("create single local-declarer");
    fs::copy(local_declarer.join("package.json"), single_proj.join("local-declarer/package.json"))
        .expect("copy local-declarer");
    fs::write(single_proj.join(".npmrc"), format!("registry={}/\n", server.url()))
        .expect("write single .npmrc");
    fs::write(single_proj.join("pnpm-workspace.yaml"), "autoInstallPeers: true\n")
        .expect("write single workspace yaml");
    fs::write(
        single_proj.join("package.json"),
        json!({
            "name": "single",
            "version": "1.0.0",
            "dependencies": {
                "local-declarer": "file:./local-declarer",
                "evil-peer-parent": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write single manifest");

    pnpm_cmd(&single_proj, &home, &auth_file)
        .with_arg("install")
        .assert()
        .success();

    // Remove `local-declarer` and bump `evil-peer-parent` spec or add `ok-pkg` so resolution runs with wanted lockfile.
    fs::write(
        single_proj.join("package.json"),
        json!({
            "name": "single",
            "version": "1.0.0",
            "dependencies": {
                "evil-peer-parent": "1.0.0",
                "ok-pkg": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("remove local-declarer from manifest");
    let warm_out = pnpm_cmd(&single_proj, &home, &auth_file)
        .with_args(["install", "--no-fix-lockfile"])
        .output()
        .expect("run warm install after removing local-declarer");
    assert!(
        !warm_out.status.success(),
        "removed local-declarer must not authorize evil-peer-parent on warm install",
    );
    let warm_stderr = String::from_utf8_lossy(&warm_out.stderr);
    assert!(warm_stderr.contains("ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED"), "stderr: {warm_stderr}");

    drop((root, server));
}

#[test]
fn non_registry_tarball_parent_never_falls_back_to_registry_packument_for_ranged_override() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let (home, auth_file) = setup_host_secrets(root.path());
    let mut server = mockito::Server::new();

    let helper_dir = workspace.join("local-helper");
    fs::create_dir_all(&helper_dir).expect("create local-helper");
    fs::write(
        helper_dir.join("package.json"),
        json!({ "name": "helper", "version": "1.0.0" }).to_string(),
    )
    .expect("write local-helper");

    // Registry `tarball-parent@1.0.0` declares `helper: ^1.0.0`, while the standalone URL tarball does NOT.
    let registry_manifest = json!({
        "name": "tarball-parent",
        "version": "1.0.0",
        "dependencies": { "helper": "^1.0.0" },
    });
    let _reg_mocks = serve_remote_pkg(&mut server, "tarball-parent", "1.0.0", &registry_manifest);

    let url_tarball_manifest = json!({
        "name": "tarball-parent",
        "version": "1.0.0",
    });
    let (tgz_bytes, tgz_integrity) = build_tgz(&url_tarball_manifest);
    let tarball_url = format!("{}/standalone-parent.tgz", server.url());
    let _tgz_mock = server
        .mock("GET", "/standalone-parent.tgz")
        .with_status(200)
        .with_body(tgz_bytes)
        .create();

    fs::write(workspace.join(".npmrc"), format!("registry={}/\n", server.url()))
        .expect("write .npmrc");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "overrides:\n  'tarball-parent>helper@^1.0.0': file:./local-helper\n",
    )
    .expect("write workspace yaml");
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "tarball-parent": tarball_url },
        })
        .to_string(),
    )
    .expect("write package.json");
    pnpm_cmd(&workspace, &home, &auth_file)
        .with_arg("install")
        .assert()
        .success();

    let clean_lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read clean lockfile");
    let header = clean_lockfile
        .split("\npackages:\n")
        .next()
        .expect("lockfile header before packages");
    let forged_lockfile = format!(
        "{header}\n\npackages:\n  'tarball-parent@{tarball_url}':\n    version: 1.0.0\n    resolution: {{integrity: {tgz_integrity}, tarball: {tarball_url}}}\n  helper@file:local-helper:\n    resolution: {{type: directory, directory: local-helper}}\n\nsnapshots:\n  'tarball-parent@{tarball_url}':\n    dependencies:\n      helper: file:local-helper\n  helper@file:local-helper: {{}}\n",
    );
    fs::write(workspace.join("pnpm-lock.yaml"), forged_lockfile).expect("write forged lockfile");

    let out = pnpm_cmd(&workspace, &home, &auth_file)
        .with_args(["install", "--frozen-lockfile"])
        .output()
        .expect("run frozen install");
    assert!(
        !out.status.success(),
        "URL tarball parent must not borrow pristine range from registry packument",
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED"), "stderr: {stderr}");

    drop((root, server));
}

#[test]
fn hook_authorized_local_peer_of_a_remote_git_parent_reinstalls_with_a_lockfile() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let (home, auth_file) = setup_host_secrets(root.path());
    for (dir, name) in [("helper-outside", "helper"), ("other-outside", "other")] {
        let path = root.path().join(dir);
        fs::create_dir_all(&path).expect("create local package");
        fs::write(
            path.join("package.json"),
            json!({ "name": name, "version": "1.0.0" }).to_string(),
        )
        .expect("write local manifest");
    }
    let git_repo =
        pnpm_testing_utils_registry::git_repo::GitRepoFixture::init(root.path(), "git-remote");
    git_repo.write_file(
        "package.json",
        &json!({
            "name": "git-remote",
            "version": "1.0.0",
            "peerDependencies": { "helper": "^1.0.0" },
        })
        .to_string(),
    );
    let commit = git_repo.commit("init");
    let served_git = git_repo.serve_http();
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { readPackage(pkg) {\n  if (pkg.name === 'git-remote' && pkg.peerDependencies) {\n    pkg.peerDependencies.helper = 'link:../helper-outside';\n  }\n  return pkg;\n} } };\n",
    )
    .expect("write .pnpmfile.cjs");
    let write_manifest = |extra: Option<(&str, &str)>| {
        let mut dependencies = json!({
            "git-remote": served_git.git_url_at(&commit),
        });
        if let Some((alias, spec)) = extra {
            dependencies[alias] = json!(spec);
        }
        fs::write(
            workspace.join("package.json"),
            json!({ "name": "app", "version": "1.0.0", "dependencies": dependencies }).to_string(),
        )
        .expect("write package.json");
    };

    write_manifest(None);
    pnpm_cmd(&workspace, &home, &auth_file)
        .with_arg("install")
        .assert()
        .success();

    write_manifest(Some(("other", "link:../other-outside")));
    pnpm_cmd(&workspace, &home, &auth_file)
        .with_arg("install")
        .assert()
        .success();

    drop((root, served_git));
}
