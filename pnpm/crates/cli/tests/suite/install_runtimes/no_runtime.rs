use super::{
    RuntimeFixture, assert_installed, command, mock_node_release, prepare_workspace,
    runtime_fixture, write_devengines_manifest, write_runtime_lockfile_for_group,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_graph_hasher::{host_arch, host_platform};
use serde_json::{Value, json};
use std::{fs, path::Path};

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
fn runtime_false_setting_skips_the_runtime() {
    for (yaml, args) in [("runtime: false\n", &[][..]), ("", &["--config.runtime=false"][..])] {
        let root = tempfile::tempdir().unwrap();
        let workspace = prepare_workspace(&root, yaml);
        let mut server = mockito::Server::new();
        let fixture = runtime_fixture(&mut server, "node", "24.0.0", host_platform(), host_arch());
        write_devengines_manifest(&workspace, fixture.version, Some("download"));
        write_runtime_lockfile_for_group(
            &workspace,
            std::slice::from_ref(&fixture),
            "devDependencies",
        );

        command(&workspace)
            .with_args(["install", "--frozen-lockfile"])
            .with_args(args)
            .assert()
            .success();
        assert!(!workspace.join("node_modules/node").exists(), "{yaml:?} {args:?}");
        assert!(!fixture.archive_mock.matched(), "{yaml:?} {args:?}");
    }
}

#[test]
fn command_line_runtime_setting_overrides_the_workspace_yaml() {
    let root = tempfile::tempdir().unwrap();
    let workspace = prepare_workspace(&root, "runtime: false\n");
    let mut server = mockito::Server::new();
    let fixture = runtime_fixture(&mut server, "node", "24.0.0", host_platform(), host_arch());
    write_devengines_manifest(&workspace, fixture.version, Some("download"));
    write_runtime_lockfile_for_group(&workspace, std::slice::from_ref(&fixture), "devDependencies");

    command(&workspace)
        .with_args(["install", "--frozen-lockfile", "--config.runtime=true"])
        .assert()
        .success();
    assert_installed(&workspace, std::slice::from_ref(&fixture));
}

/// An `updateConfig` hook reads `runtime` only when a source set it, as on
/// pnpm 11, where `--no-runtime` is the `runtime: false` setting.
#[test]
fn update_config_sees_runtime_only_when_set() {
    let dump_config = "const fs = require('fs');\nconst path = require('path');\nmodule.exports = { hooks: { updateConfig (config) {\n  fs.writeFileSync(path.join(__dirname, 'seen.json'), JSON.stringify(config));\n  return config;\n} } }";
    for (yaml, args, expected) in [
        ("", &[][..], None),
        ("runtime: false\n", &[][..], Some(false)),
        ("", &["--no-runtime"][..], Some(false)),
        ("", &["--config.runtime=true"][..], Some(true)),
    ] {
        let root = tempfile::tempdir().unwrap();
        let workspace = prepare_workspace(&root, yaml);
        let mut server = mockito::Server::new();
        let fixture = runtime_fixture(&mut server, "node", "24.0.0", host_platform(), host_arch());
        write_devengines_manifest(&workspace, fixture.version, Some("download"));
        write_runtime_lockfile_for_group(
            &workspace,
            std::slice::from_ref(&fixture),
            "devDependencies",
        );
        fs::write(workspace.join(".pnpmfile.cjs"), dump_config).unwrap();

        command(&workspace)
            .with_arg("install")
            .with_args(args)
            .assert()
            .success();
        let seen: Value =
            serde_json::from_slice(&fs::read(workspace.join("seen.json")).unwrap()).unwrap();
        assert_eq!(seen.get("runtime").and_then(Value::as_bool), expected, "{yaml:?} {args:?}");
        assert_eq!(seen.get("runtime").is_some(), expected.is_some(), "{yaml:?} {args:?}");
    }
}

#[test]
fn dedupe_and_prune_follow_the_runtime_setting() {
    let cases: [(&str, &[&str]); 3] = [
        ("runtime: false\n", &["dedupe"]),
        ("runtime: false\n", &["prune"]),
        ("", &["dedupe", "--no-runtime"]),
    ];
    for (yaml, args) in cases {
        let root = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new();
        let (workspace, fixture) = locked_runtime_project(&root, &mut server, yaml);

        command(&workspace)
            .with_args(args)
            .assert()
            .success();
        assert!(!workspace.join("node_modules/node").exists(), "{args:?}");
        assert!(!fixture.archive_mock.matched(), "{args:?}");
    }
}

#[test]
fn update_config_hook_cannot_undo_no_runtime() {
    for command_name in ["install", "dedupe"] {
        let root = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new();
        let (workspace, fixture) = locked_runtime_project(&root, &mut server, "");
        fs::write(
            workspace.join(".pnpmfile.cjs"),
            "module.exports = { hooks: { updateConfig (config) { config.runtime = true; return config } } }",
        )
        .unwrap();

        command(&workspace)
            .with_args([command_name, "--no-runtime"])
            .assert()
            .success();
        assert!(!workspace.join("node_modules/node").exists(), "{command_name}");
        assert!(!fixture.archive_mock.matched(), "{command_name}");
    }
}

#[test]
fn update_config_hook_can_skip_the_runtime() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new();
    let (workspace, fixture) = locked_runtime_project(&root, &mut server, "");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { updateConfig (config) { config.runtime = false; return config } } }",
    )
    .unwrap();

    command(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert!(!workspace.join("node_modules/node").exists());
    assert!(!fixture.archive_mock.matched());
}

/// A workspace whose `devEngines.runtime` is locked to a mocked Node.js 24.0.0.
fn locked_runtime_project(
    root: &tempfile::TempDir,
    server: &mut mockito::Server,
    extra_yaml: &str,
) -> (std::path::PathBuf, RuntimeFixture) {
    let workspace = prepare_workspace(root, extra_yaml);
    let fixture = runtime_fixture(server, "node", "24.0.0", host_platform(), host_arch());
    write_devengines_manifest(&workspace, fixture.version, Some("download"));
    write_runtime_lockfile_for_group(&workspace, std::slice::from_ref(&fixture), "devDependencies");
    (workspace, fixture)
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

#[test]
fn engines_are_checked_against_the_locked_node_runtime() {
    for linker in ["isolated", "hoisted"] {
        let root = tempfile::tempdir().unwrap();
        let workspace = prepare_workspace(&root, format!("nodeLinker: {linker}\n").as_str());
        let mut server = mockito::Server::new();
        let fixture = runtime_fixture(&mut server, "node", "24.1.0", host_platform(), host_arch());
        write_range_runtime_project(&workspace, &fixture, "optionalDependencies", ">=24.1.0");

        command(&workspace)
            .with_args(["install", "--frozen-lockfile", "--no-runtime"])
            .assert()
            .success();
        assert!(
            workspace.join("node_modules/dependency/package.json").exists(),
            "{linker}: the locked Node.js 24.1.0 satisfies the optional dependency's engines",
        );
    }
}

#[test]
fn explicit_node_version_takes_priority_over_the_locked_node_runtime() {
    let root = tempfile::tempdir().unwrap();
    let workspace = prepare_workspace(&root, "engineStrict: true\nnodeVersion: 20.0.0\n");
    let mut server = mockito::Server::new();
    let fixture = runtime_fixture(&mut server, "node", "24.1.0", host_platform(), host_arch());
    write_range_runtime_project(&workspace, &fixture, "dependencies", "<21");

    command(&workspace)
        .with_args(["install", "--frozen-lockfile", "--no-runtime"])
        .assert()
        .success();
}

/// A project whose `devEngines.runtime` range is locked to `fixture` and
/// that depends on a local `dependency` requiring `engines_node`.
fn write_range_runtime_project(
    workspace: &Path,
    fixture: &RuntimeFixture,
    dependency_group: &str,
    engines_node: &str,
) {
    write_devengines_manifest(workspace, "^24.0.0", Some("download"));
    write_runtime_lockfile_for_group(workspace, std::slice::from_ref(fixture), "devDependencies");
    let dependency = workspace.join("dependency");
    fs::create_dir(&dependency).unwrap();
    fs::write(
        dependency.join("package.json"),
        json!({ "name": "dependency", "version": "1.0.0", "engines": { "node": engines_node } })
            .to_string(),
    )
    .unwrap();
    let manifest_path = workspace.join("package.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest[dependency_group] = json!({ "dependency": "file:dependency" });
    fs::write(manifest_path, manifest.to_string()).unwrap();
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let mut lockfile: Value =
        serde_saphyr::from_str(&fs::read_to_string(&lockfile_path).unwrap()).unwrap();
    lockfile["importers"]["."]["devDependencies"]["node"]["specifier"] = json!("runtime:^24.0.0");
    lockfile["importers"]["."][dependency_group] = json!({
        "dependency": { "specifier": "file:dependency", "version": "file:dependency" },
    });
    lockfile["packages"]["dependency@file:dependency"] = json!({
        "resolution": { "directory": "dependency", "type": "directory" },
        "engines": { "node": engines_node },
    });
    lockfile["snapshots"]["dependency@file:dependency"] =
        json!({ "optional": dependency_group == "optionalDependencies" });
    fs::write(&lockfile_path, serde_saphyr::to_string(&lockfile).unwrap()).unwrap();
}
