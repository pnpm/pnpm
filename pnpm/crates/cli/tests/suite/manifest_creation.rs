use assert_cmd::prelude::*;
use pnpm_testing_utils::{bin::CommandTempCwd, command_env::CommandTestExt};
use serde_json::Value;
use std::{env, fs, process::Command};

fn fixture() -> CommandTempCwd<()> {
    let mut fixture = CommandTempCwd::init();
    fixture.pacquet
        .env("PNPM_CONFIG_STORE_DIR", fixture.root.path().join("store"))
        .env("PNPM_CONFIG_CACHE_DIR", fixture.root.path().join("cache"));
    fixture
}

const LOCKFILE: &str = "lockfileVersion: '9.0'\nsettings:\n  autoInstallPeers: true\n  excludeLinksFromLockfile: false\nimporters:\n  .: {}\n";

#[test]
fn queries_do_not_create_a_manifest() {
    for args in [&["why", "foo"][..], &["outdated"]] {
        let CommandTempCwd { mut pacquet, workspace, root, .. } = fixture();
        pacquet.args(args).assert().success();
        assert!(!workspace.join("package.json").exists(), "{args:?} created a manifest");
        drop(root);
    }
}

#[test]
fn rejected_commands_do_not_create_a_manifest() {
    let cases: &[(&[&str], &str)] = &[
        (&["rt", "ls"], "ERR_PNPM_RUNTIME_UNKNOWN_SUBCOMMAND"),
        (&["rt"], "ERR_PNPM_RUNTIME_NO_SUBCOMMAND"),
        (&["rt", "set"], "ERR_PNPM_MISSING_RUNTIME_NAME"),
        (&["rt", "set", "python"], "ERR_PNPM_INVALID_RUNTIME_NAME"),
        (&["audit"], "ERR_PNPM_AUDIT_NO_LOCKFILE"),
        (&["sbom", "--sbom-format", "cyclonedx"], "ERR_PNPM_SBOM_NO_LOCKFILE"),
        (&["import"], "ERR_PNPM_LOCKFILE_NOT_FOUND"),
        (&["patch"], "ERR_PNPM_MISSING_PACKAGE_NAME"),
        (&["patch-commit", "./missing"], "ERR_PNPM_INVALID_PATCH_DIR"),
        (&["patch-remove"], "ERR_PNPM_NO_PATCHES_TO_REMOVE"),
        (&["remove"], "ERR_PNPM_MUST_REMOVE_SOMETHING"),
        (&["remove", "foo"], "ERR_PNPM_CANNOT_REMOVE_MISSING_DEPS"),
    ];
    for (args, error) in cases {
        let CommandTempCwd { mut pacquet, workspace, root, .. } = fixture();
        let output = pacquet
            .args(*args)
            .assert()
            .failure()
            .get_output()
            .stderr
            .clone();
        let stderr = String::from_utf8_lossy(&output);
        assert!(stderr.contains(error), "stderr: {stderr}");
        assert!(!workspace.join("package.json").exists(), "{args:?} created a manifest");
        drop(root);
    }
}

#[test]
fn dedupe_check_does_not_create_a_manifest() {
    let CommandTempCwd { mut pacquet, workspace, root, .. } = fixture();
    let output = pacquet
        .args(["dedupe", "--check", "--offline"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&output);
    assert!(stdout.contains("ERR_PNPM_DEDUPE_CHECK_ISSUES"), "stdout: {stdout}");
    assert!(!workspace.join("package.json").exists(), "dedupe --check created a manifest");
    assert!(!workspace.join("pnpm-lock.yaml").exists(), "dedupe --check left a lockfile");
    drop(root);
}

#[test]
fn missing_link_target_does_not_create_a_manifest() {
    for in_workspace in [false, true] {
        let CommandTempCwd { mut pacquet, workspace, root, .. } = fixture();
        if in_workspace {
            fs::write(workspace.join("pnpm-workspace.yaml"), "packages: []\n")
                .expect("write rootless workspace");
        }
        let output = pacquet
            .args(["link", "./missing"])
            .assert()
            .failure()
            .get_output()
            .stderr
            .clone();
        let stderr = String::from_utf8_lossy(&output);
        assert!(stderr.contains("missing"), "stderr: {stderr}");
        assert!(!workspace.join("package.json").exists(), "failed link created a manifest");
        drop(root);
    }
}

#[test]
fn fetch_from_a_lockfile_does_not_create_a_manifest() {
    let CommandTempCwd { mut pacquet, workspace, root, .. } = fixture();
    fs::write(workspace.join("pnpm-lock.yaml"), LOCKFILE).expect("write lockfile");
    pacquet
        .args(["fetch", "--offline"])
        .assert()
        .success();
    assert!(!workspace.join("package.json").exists(), "fetch created a manifest");
    drop(root);
}

#[test]
fn sbom_from_a_lockfile_does_not_invent_manifest_metadata() {
    let CommandTempCwd { mut pacquet, workspace, root, .. } = fixture();
    fs::write(workspace.join("pnpm-lock.yaml"), LOCKFILE).expect("write lockfile");
    let output = pacquet
        .args(["sbom", "--sbom-format", "cyclonedx"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let sbom: Value = serde_json::from_slice(&output).expect("parse SBOM");
    let component = &sbom["metadata"]["component"];
    assert_eq!(component["name"], "unknown");
    assert_eq!(component["version"], "0.0.0");
    assert!(component.get("licenses").is_none(), "invented license: {component}");
    assert!(!workspace.join("package.json").exists(), "sbom created a manifest");
    drop(root);
}

#[test]
fn install_still_initializes_a_missing_manifest() {
    let CommandTempCwd { mut pacquet, workspace, root, .. } = fixture();
    pacquet
        .args(["install", "--offline", "--ignore-scripts"])
        .assert()
        .success();
    let manifest: Value = serde_json::from_slice(
        &fs::read(workspace.join("package.json")).expect("install wrote manifest"),
    )
    .expect("parse manifest");
    assert_eq!(manifest["version"], "1.0.0");
    assert_eq!(manifest["license"], "ISC");
    drop(root);
}

#[test]
fn queries_preserve_existing_manifest_formats() {
    for (name, contents) in [
        ("package.json", "{\"name\":\"existing\",\"private\":true}\n"),
        ("package.json5", "{name: 'existing', private: true}\n"),
        ("package.yaml", "name: existing\nprivate: true\n"),
    ] {
        let CommandTempCwd { mut pacquet, workspace, root, .. } = fixture();
        fs::write(workspace.join(name), contents).expect("write manifest");
        pacquet
            .args(["why", "foo"])
            .assert()
            .success();
        assert_eq!(fs::read_to_string(workspace.join(name)).unwrap(), contents);
        if name != "package.json" {
            assert!(!workspace.join("package.json").exists(), "query shadowed {name}");
        }
        drop(root);
    }
}

const NATIVE_COMMANDS: &[&[&str]] = &[
    &["install", "--offline"],
    &[
        "install",
        "--offline",
        "--frozen-lockfile",
        "--trust-lockfile",
        "--pnpr-server",
        "http://127.0.0.1:9",
    ],
    &["update", "--offline"],
    &["update", "--offline", "--no-save"],
    &["update", "--offline", "--patches"],
    &["dedupe", "--offline"],
    &["prune"],
];

/// Substitute only the native compiler, leaving lifecycle dispatch and its
/// error handling to the real CLI. The empty trusted lockfile needs no server.
fn native_fixture(exit_code: i32) -> CommandTempCwd<()> {
    let mut fixture = fixture();
    let bin = fixture.root.path().join("bin");
    fs::create_dir(&bin).expect("create bin directory");
    fs::write(
        bin.join("gyp.cjs"),
        format!(
            "require('fs').writeFileSync('native-marker', JSON.stringify({{args: process.argv.slice(2), version: process.env.npm_package_version}})); process.exit({exit_code});",
        ),
    )
    .expect("write native build substitute");
    #[cfg(unix)]
    crate::_utils::write_executable(
        &bin.join("node-gyp"),
        "#!/bin/sh\nexec node \"$(dirname \"$0\")/gyp.cjs\" \"$@\"\n",
    );
    #[cfg(windows)]
    fs::write(bin.join("node-gyp.cmd"), "@node \"%~dp0gyp.cjs\" %*\r\n")
        .expect("write native build launcher");
    let path = env::join_paths(
        std::iter::once(bin).chain(env::split_paths(&env::var_os("PATH").unwrap_or_default())),
    )
    .expect("join PATH");
    fixture.pacquet.env("PATH", path);
    fs::write(fixture.workspace.join("binding.gyp"), "{}").expect("write binding.gyp");
    fs::write(fixture.workspace.join("pnpm-lock.yaml"), LOCKFILE).expect("write lockfile");
    fixture
}

fn assert_native_build(args: &[&str], exit_code: i32, existing: bool, after_resolution: bool) {
    let CommandTempCwd { mut pacquet, workspace, root, .. } = native_fixture(exit_code);
    if existing {
        fs::write(workspace.join("package.json"), r#"{"version":"2.0.0"}"#)
            .expect("write existing manifest");
    }
    if after_resolution {
        fs::remove_file(workspace.join("binding.gyp")).expect("remove initial native trigger");
        fs::write(
            workspace.join(".pnpmfile.cjs"),
            "exports.hooks = { afterAllResolved(lockfile) { require('fs').writeFileSync(require('path').join(__dirname, 'binding.gyp'), '{}'); return lockfile; } };",
        )
        .expect("write native trigger hook");
    }
    let result = pacquet.args(args).assert();
    if exit_code == 0 {
        result.success();
    } else {
        let result = result.failure();
        let stderr = String::from_utf8_lossy(&result.get_output().stderr);
        assert!(stderr.contains("ERR_PNPM_EXECUTOR_LIFECYCLE_SCRIPT_FAILED"), "{stderr}");
        assert!(stderr.contains("17"), "{stderr}");
    }
    let marker: Value = serde_json::from_slice(
        &fs::read(workspace.join("native-marker")).expect("native build ran"),
    )
    .expect("parse native marker");
    assert_eq!(marker["args"], serde_json::json!(["rebuild"]), "{args:?}");
    assert_eq!(marker["version"], if existing { "2.0.0" } else { "1.0.0" });
    drop(root);
}

#[test]
fn native_build_runs_with_initialized_manifest_data() {
    for args in NATIVE_COMMANDS {
        assert_native_build(args, 0, false, false);
        assert_native_build(args, 0, true, false);
    }
}

#[test]
fn native_build_failure_is_propagated() {
    for args in NATIVE_COMMANDS {
        assert_native_build(args, 17, false, false);
        assert_native_build(args, 17, true, false);
    }
}

#[test]
fn native_build_runs_after_resolution_creates_trigger() {
    let commands: &[&[&str]] = &[
        &["install", "--offline", "--no-frozen-lockfile", "--no-prefer-frozen-lockfile"],
        &["install", "--offline", "--pnpr-server", "http://127.0.0.1:9"],
        &["update", "--offline"],
        &["update", "--offline", "--no-save"],
        &["update", "--offline", "--patches"],
        &["update", "--offline", "--patches", "--pnpr-server", "http://127.0.0.1:9"],
        &["dedupe", "--offline"],
        &["prune", "--offline"],
    ];
    for args in commands {
        for exit_code in [0, 17] {
            assert_native_build(args, exit_code, false, true);
            assert_native_build(args, exit_code, true, true);
        }
    }
}

#[test]
fn native_build_respects_ignore_scripts() {
    for args in NATIVE_COMMANDS {
        let CommandTempCwd { mut pacquet, workspace, root, .. } = native_fixture(17);
        pacquet
            .arg("--ignore-scripts")
            .args(*args)
            .assert()
            .success();
        assert!(!workspace.join("native-marker").exists(), "{args:?} ran a native build");
        if args[0] != "install" {
            assert!(!workspace.join("package.json").exists(), "{args:?} created a manifest");
        }
        drop(root);
    }
}

#[test]
fn native_build_keeps_rootless_workspace_behavior() {
    for args in NATIVE_COMMANDS {
        let CommandTempCwd { mut pacquet, workspace, root, .. } = native_fixture(17);
        fs::write(workspace.join("pnpm-workspace.yaml"), "packages: []\n")
            .expect("write rootless workspace");
        pacquet.args(*args).assert().success();
        assert!(!workspace.join("native-marker").exists(), "{args:?} ran a native build");
        assert!(!workspace.join("package.json").exists(), "{args:?} created a manifest");
        drop(root);
    }
}

#[test]
fn native_build_is_skipped_when_not_materializing() {
    for args in [
        &["install", "--offline", "--dry-run"][..],
        &["install", "--offline", "--lockfile-only"],
        &["update", "--offline", "--lockfile-only"],
        &["update", "--offline", "--latest"],
        &["update", "--offline", "missing"],
        &["rebuild"],
        &["rebuild", "--pending"],
        &["dedupe", "--offline", "--lockfile-only"],
        &["dedupe", "--offline", "--check"],
    ] {
        let CommandTempCwd { mut pacquet, workspace, root, .. } = native_fixture(17);
        let result = pacquet.args(args).assert();
        if args.contains(&"--check") {
            let result = result.failure();
            let stdout = String::from_utf8_lossy(&result.get_output().stdout);
            assert!(stdout.contains("ERR_PNPM_DEDUPE_CHECK_ISSUES"), "{stdout}");
        } else {
            result.success();
        }
        assert!(!workspace.join("native-marker").exists(), "{args:?} ran a native build");
        if args != ["install", "--offline", "--lockfile-only"] {
            assert!(!workspace.join("package.json").exists(), "{args:?} created a manifest");
        }
        drop(root);
    }
}

#[test]
fn pending_native_build_recovers_after_ignored_scripts() {
    for prepare in ["dedupe", "prune"] {
        for exit_code in [0, 17] {
            let CommandTempCwd { mut pacquet, workspace, root, .. } = native_fixture(exit_code);
            Command::cargo_bin("pnpm")
                .expect("find pnpm binary")
                .without_ambient_pnpm_config()
                .current_dir(&workspace)
                .env("PNPM_CONFIG_STORE_DIR", root.path().join("store"))
                .env("PNPM_CONFIG_CACHE_DIR", root.path().join("cache"))
                .args([prepare, "--offline", "--ignore-scripts"])
                .assert()
                .success();
            assert!(!workspace.join("package.json").exists(), "{prepare} created a manifest");
            assert!(!workspace.join("native-marker").exists(), "{prepare} ran a native build");
            let modules_dir = workspace.join("node_modules");
            let pending = || {
                pnpm_modules_yaml::read_modules_manifest::<pnpm_modules_yaml::Host>(&modules_dir)
                    .expect("read modules manifest")
                    .expect("modules manifest exists")
                    .pending_builds
            };
            assert_eq!(pending(), ["."], "{prepare} must record the pending project");
            let result = pacquet
                .args(["rebuild", "--pending"])
                .assert();
            if exit_code == 0 {
                result.success();
                assert_eq!(pending(), Vec::<String>::new());
            } else {
                let result = result.failure();
                let stderr = String::from_utf8_lossy(&result.get_output().stderr);
                assert!(stderr.contains("ERR_PNPM_EXECUTOR_LIFECYCLE_SCRIPT_FAILED"), "{stderr}");
                assert!(stderr.contains("17"), "{stderr}");
                assert_eq!(pending(), ["."]);
            }
            let marker: Value = serde_json::from_slice(
                &fs::read(workspace.join("native-marker")).expect("pending native build ran"),
            )
            .expect("parse native marker");
            assert_eq!(marker["args"], serde_json::json!(["rebuild"]));
            assert_eq!(marker["version"], "1.0.0");
            drop(root);
        }
    }
}
