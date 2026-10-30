use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use serde_json::Value;
use std::{fs, path::Path, process::Command};

fn write_loaded_tarball(workspace: &Path, files: &[(&str, &str)]) {
    let mut tarball = tar::Builder::new(Vec::new());
    for (path, body) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        tarball
            .append_data(&mut header, format!("package/{path}"), body.as_bytes())
            .unwrap();
    }
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut encoder, &tarball.into_inner().unwrap()).unwrap();
    fs::write(workspace.join("dep.tgz"), encoder.finish().unwrap()).unwrap();
}

#[test]
fn loaded_cache_miss_builds_once_in_a_temporary_directory_and_loads_from_cas() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let package = serde_json::json!({
        "name": "temporary-build", "version": "1.0.0",
        "dependencies": {"build-helper": "npm:@scoped/exports-function@4.1.1"},
        "bin": {"temporary-command": "cli.js"},
        "scripts": {"postinstall": r#"node -e "const fs = require('node:fs'); fs.appendFileSync(process.env.INIT_CWD + '/build-count', 'x'); fs.writeFileSync('built.json', JSON.stringify({value:typeof require('build-helper')==='function'?42:0,directory:process.cwd()})); fs.writeFileSync('cli.js', 'console.log(42)')""#}
    }).to_string();
    write_loaded_tarball(
        &workspace,
        &[("package.json", &package), ("index.js", "module.exports = require('./built.json')")],
    );
    fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"temporary-build":"file:dep.tgz"}}"#,
    )
    .unwrap();
    let yaml = workspace.join("pnpm-workspace.yaml");
    let mut config = fs::read_to_string(&yaml).unwrap();
    config.push_str("nodeLinker:\n  type: loaded\nsideEffectsCache: true\nallowBuilds:\n  'temporary-build@file:dep.tgz': true\n");
    fs::write(yaml, config).unwrap();
    pacquet
        .with_arg("install")
        .assert()
        .success();
    for frozen in [false, true] {
        let manifest: Value = serde_json::from_slice(
            &fs::read(workspace.join("node_modules/.pnpm/.store-manifest.json")).unwrap(),
        )
        .unwrap();
        let package = manifest["packages"]
            .as_object()
            .unwrap()
            .values()
            .find(|package| package["files"].get("built.json").is_some())
            .expect("built output is in CAS");
        assert!(package["root"].is_null());
        let output = Command::cargo_bin("pnpm")
            .unwrap()
            .with_current_dir(&workspace)
            .with_args([
                "exec",
                "node",
                "-e",
                "console.log(JSON.stringify(require('temporary-build')))",
            ])
            .env_remove("NODE_OPTIONS")
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["value"], 42);
        let directory = Path::new(result["directory"].as_str().unwrap());
        assert!(directory.to_string_lossy().contains("pnpm-loaded-build-"));
        assert!(!directory.exists());
        assert_eq!(fs::read_to_string(workspace.join("build-count")).unwrap(), "x");
        if !frozen {
            Command::cargo_bin("pnpm")
                .unwrap()
                .with_current_dir(&workspace)
                .with_args(["install", "--frozen-lockfile"])
                .assert()
                .success();
        }
    }
    let output = Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_args(["exec", "temporary-command"])
        .env_remove("NODE_OPTIONS")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "42");
    drop((root, mock_instance));
}

#[test]
fn loaded_filesystem_packages_are_materialized_without_an_exclusion() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_loaded_tarball(
        &workspace,
        &[
            ("package.json", r#"{"name":"adjacent-files","version":"1.0.0"}"#),
            (
                "index.js",
                "module.exports = require('node:fs').readFileSync(require('node:path').join(__dirname, 'asset.txt'), 'utf8')",
            ),
            ("asset.txt", "physical asset"),
        ],
    );
    fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"adjacent-files":"file:dep.tgz"}}"#,
    )
    .unwrap();
    let yaml = workspace.join("pnpm-workspace.yaml");
    let mut config = fs::read_to_string(&yaml).unwrap();
    config.push_str("nodeLinker:\n  type: loaded\n");
    fs::write(yaml, config).unwrap();
    pacquet
        .with_arg("install")
        .assert()
        .success();
    let manifest: Value = serde_json::from_slice(
        &fs::read(workspace.join("node_modules/.pnpm/.store-manifest.json")).unwrap(),
    )
    .unwrap();
    assert!(
        manifest["packages"]
            .as_object()
            .unwrap()
            .values()
            .any(|package| package["resolution"] == "node"),
    );
    let output = Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_args(["exec", "node", "-e", "console.log(require('adjacent-files'))"])
        .env_remove("NODE_OPTIONS")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "physical asset");
    drop((root, mock_instance));
}

#[test]
fn loaded_incompatible_build_outputs_are_retained_without_rebuilding() {
    let mut cases = vec![
        (
            "native",
            "fs.writeFileSync('built.json', '42'); fs.writeFileSync('binding.node', 'native')",
        ),
        (
            "filesystem",
            r#"fs.writeFileSync('built.json', '42'); fs.writeFileSync('index.js', "module.exports = JSON.parse(require('node:fs').readFileSync(require('node:path').join(__dirname, 'built.json'), 'utf8'))")"#,
        ),
    ];
    #[cfg(unix)]
    cases.push((
        "symlink",
        "fs.unlinkSync('built.json'); fs.symlinkSync(process.env.INIT_CWD + '/external.json', 'built.json')",
    ));
    for (kind, build) in cases.drain(..) {
        let CommandTempCwd {
            pacquet,
            root,
            workspace,
            npmrc_info,
            ..
        } = CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;
        let script = format!(
            "const fs = require('node:fs'); fs.appendFileSync(process.env.INIT_CWD + '/build-count', 'x'); {build}",
        );
        let package = serde_json::json!({
            "name": "incompatible-build", "version": "1.0.0",
            "scripts": {"postinstall": format!("node -e {}", serde_json::to_string(&script).unwrap())}
        }).to_string();
        write_loaded_tarball(
            &workspace,
            &[
                ("package.json", &package),
                ("index.js", "module.exports = require('./built.json')"),
                ("built.json", "42"),
            ],
        );
        fs::write(workspace.join("external.json"), "42").unwrap();
        fs::write(
            workspace.join("package.json"),
            r#"{"dependencies":{"incompatible-build":"file:dep.tgz"}}"#,
        )
        .unwrap();
        let yaml = workspace.join("pnpm-workspace.yaml");
        let mut config = fs::read_to_string(&yaml).unwrap();
        config.push_str("nodeLinker:\n  type: loaded\nsideEffectsCache: true\nallowBuilds:\n  'incompatible-build@file:dep.tgz': true\n");
        fs::write(yaml, config).unwrap();
        pacquet
            .with_arg("install")
            .assert()
            .success();
        for frozen in [false, true] {
            let manifest: Value = serde_json::from_slice(
                &fs::read(workspace.join("node_modules/.pnpm/.store-manifest.json")).unwrap(),
            )
            .unwrap();
            let package = manifest["packages"]
                .as_object()
                .unwrap()
                .iter()
                .find(|(key, _)| key.starts_with("incompatible-build@"))
                .unwrap()
                .1;
            let directory = Path::new(package["root"].as_str().expect(kind));
            assert!(directory.is_dir(), "{kind}: {directory:?}");
            assert!(package["files"].is_null(), "{kind}: {package:?}");
            let output = Command::cargo_bin("pnpm")
                .unwrap()
                .with_current_dir(&workspace)
                .with_args(["exec", "node", "-e", "console.log(require('incompatible-build'))"])
                .env_remove("NODE_OPTIONS")
                .output()
                .unwrap();
            assert!(output.status.success(), "{kind}: {}", String::from_utf8_lossy(&output.stderr));
            assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "42", "{kind}");
            assert_eq!(fs::read_to_string(workspace.join("build-count")).unwrap(), "x", "{kind}");
            if !frozen {
                Command::cargo_bin("pnpm")
                    .unwrap()
                    .with_current_dir(&workspace)
                    .with_args(["install", "--frozen-lockfile"])
                    .assert()
                    .success();
            }
        }
        drop((root, mock_instance));
    }
}

fn loaded_build_fixture(
    package: &Value,
    project: &Value,
    settings: &str,
) -> CommandTempCwd<AddMockedRegistry> {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    write_loaded_tarball(
        &fixture.workspace,
        &[("package.json", &package.to_string()), ("index.js", "module.exports = 42")],
    );
    fs::write(fixture.workspace.join("package.json"), project.to_string()).unwrap();
    let yaml = fixture.workspace.join("pnpm-workspace.yaml");
    let mut config = fs::read_to_string(&yaml).unwrap();
    config.push_str("nodeLinker:\n  type: loaded\nsideEffectsCache: true\n");
    config.push_str(settings);
    fs::write(yaml, config).unwrap();
    fixture
}

#[test]
fn loaded_temporary_builds_preserve_script_authorization_and_ignore_scripts() {
    for settings in [
        "allowBuilds:\n  'guarded-build@file:dep.tgz': false\n",
        "allowBuilds:\n  'guarded-build@file:dep.tgz': true\nignoreScripts: true\n",
    ] {
        let fixture = loaded_build_fixture(
            &serde_json::json!({
                "name": "guarded-build", "version": "1.0.0",
                "scripts": {"postinstall": r#"node -e "require('node:fs').writeFileSync(process.env.INIT_CWD + '/unexpected-build', 'ran')""#}
            }),
            &serde_json::json!({"dependencies": {"guarded-build": "file:dep.tgz"}}),
            settings,
        );
        fixture.pacquet
            .with_arg("install")
            .assert()
            .success();
        assert!(!fixture.workspace.join("unexpected-build").exists(), "{settings}");
        let manifest: Value = serde_json::from_slice(
            &fs::read(fixture.workspace.join("node_modules/.pnpm/.store-manifest.json")).unwrap(),
        )
        .unwrap();
        assert!(
            manifest["packages"]
                .as_object()
                .unwrap()
                .iter()
                .any(|(key, package)| key.starts_with("guarded-build@")
                    && package["root"].is_string()),
        );
    }
}

#[test]
fn loaded_optional_temporary_build_failure_is_omitted_from_the_manifest() {
    let fixture = loaded_build_fixture(
        &serde_json::json!({
            "name": "optional-build", "version": "1.0.0",
            "scripts": {"postinstall": r#"node -e "require('node:fs').appendFileSync(process.env.INIT_CWD + '/build-count', 'x'); process.exit(1)""#}
        }),
        &serde_json::json!({"optionalDependencies": {"optional-build": "file:dep.tgz"}}),
        "allowBuilds:\n  'optional-build@file:dep.tgz': true\n",
    );
    fixture.pacquet
        .with_arg("install")
        .assert()
        .success();
    assert_eq!(fs::read_to_string(fixture.workspace.join("build-count")).unwrap(), "x");
    let manifest: Value = serde_json::from_slice(
        &fs::read(fixture.workspace.join("node_modules/.pnpm/.store-manifest.json")).unwrap(),
    )
    .unwrap();
    assert!(
        !manifest["packages"]
            .as_object()
            .unwrap()
            .keys()
            .any(|key| key.starts_with("optional-build@")),
    );
}

#[test]
fn loaded_temporary_build_keeps_excluded_packages_materialized() {
    let fixture = loaded_build_fixture(
        &serde_json::json!({
            "name": "temporary-build", "version": "1.0.0",
            "scripts": {"postinstall": r#"node -e "require('node:fs').writeFileSync('built.txt', 'built')""#}
        }),
        &serde_json::json!({"dependencies": {
            "temporary-build": "file:dep.tgz",
            "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
        }}),
        "allowBuilds:\n  'temporary-build@file:dep.tgz': true\n",
    );
    let yaml = fixture.workspace.join("pnpm-workspace.yaml");
    let config = fs::read_to_string(&yaml)
        .unwrap()
        .replace(
            "  type: loaded\n",
            "  type: loaded\n  excluded:\n    - '@pnpm.e2e/pkg-with-1-dep'\n",
        );
    fs::write(yaml, config).unwrap();
    fixture.pacquet
        .with_arg("install")
        .assert()
        .success();
    let manifest: Value = serde_json::from_slice(
        &fs::read(fixture.workspace.join("node_modules/.pnpm/.store-manifest.json")).unwrap(),
    )
    .unwrap();
    let packages = manifest["packages"].as_object().unwrap();
    assert!(
        packages
            .values()
            .any(|package| package["files"].get("built.txt").is_some()),
    );
    let excluded = &packages["@pnpm.e2e/pkg-with-1-dep@100.0.0"];
    assert!(Path::new(excluded["root"].as_str().unwrap()).is_dir());
}

#[test]
fn loaded_build_without_package_changes_is_shared_between_projects_unless_excluded() {
    for exclude in [false, true] {
        let fixture = loaded_build_fixture(
            &serde_json::json!({
                "name": "outside-build", "version": "1.0.0",
                "scripts": {"postinstall": r#"node -e "require('node:fs').appendFileSync(process.env.INIT_CWD + '/build-count', 'x')""#}
            }),
            &serde_json::json!({"dependencies": {"outside-build": "file:dep.tgz"}}),
            "allowBuilds:\n  'outside-build@file:dep.tgz': true\n",
        );
        if exclude {
            let yaml = fixture.workspace.join("pnpm-workspace.yaml");
            let config = fs::read_to_string(&yaml).unwrap().replace(
                "sideEffectsCache: true\n",
                "sideEffectsCache:\n  read: true\n  write: true\n  exclude:\n    - outside-build\n",
            );
            fs::write(yaml, config).unwrap();
        }
        let second = fixture.root.path().join("second");
        fs::create_dir(&second).unwrap();
        for file in [".npmrc", "pnpm-workspace.yaml", "package.json", "dep.tgz"] {
            fs::copy(fixture.workspace.join(file), second.join(file)).unwrap();
        }
        fixture.pacquet
            .with_arg("install")
            .assert()
            .success();
        Command::cargo_bin("pnpm")
            .unwrap()
            .with_current_dir(&second)
            .with_arg("install")
            .assert()
            .success();
        for project in [&fixture.workspace, &second] {
            let manifest: Value = serde_json::from_slice(
                &fs::read(project.join("node_modules/.pnpm/.store-manifest.json")).unwrap(),
            )
            .unwrap();
            let (_, package) = manifest["packages"]
                .as_object()
                .unwrap()
                .iter()
                .find(|(key, _)| key.starts_with("outside-build@"))
                .unwrap();
            dbg!(exclude, package);
            assert_eq!(package["root"].is_string(), exclude);
            assert_eq!(package["files"].get("index.js").is_some(), !exclude);
        }
        assert_eq!(fs::read_to_string(fixture.workspace.join("build-count")).unwrap(), "x");
        assert_eq!(
            fs::read_to_string(second.join("build-count")).ok().as_deref(),
            exclude.then_some("x"),
        );
    }
}

#[test]
fn loaded_temporary_build_reuses_a_dependency_build_without_package_changes() {
    let fixture = loaded_build_fixture(
        &serde_json::json!({
            "name": "outside-build", "version": "1.0.0",
            "scripts": {"postinstall": r#"node -e "require('node:fs').appendFileSync(process.env.INIT_CWD + '/build-count', 'x')""#}
        }),
        &serde_json::json!({"dependencies": {"outside-build": "file:../outside.tgz"}}),
        "allowBuilds:\n  'outside-build@file:../outside.tgz': true\n  'writes-file@file:../writes.tgz': true\noverrides:\n  outside-build: file:../outside.tgz\n",
    );
    let root = fixture.root.path();
    fs::rename(fixture.workspace.join("dep.tgz"), root.join("outside.tgz")).unwrap();
    let second = root.join("second");
    fs::create_dir(&second).unwrap();
    write_loaded_tarball(
        &second,
        &[
            (
                "package.json",
                &serde_json::json!({
                    "name": "writes-file", "version": "1.0.0",
                    "dependencies": {"outside-build": "1.0.0"},
                    "scripts": {"postinstall": r#"node -e "require('node:fs').writeFileSync('built.json', '42')""#}
                })
                .to_string(),
            ),
            ("index.js", "module.exports = require('./built.json')"),
        ],
    );
    fs::rename(second.join("dep.tgz"), root.join("writes.tgz")).unwrap();
    for file in [".npmrc", "pnpm-workspace.yaml"] {
        fs::copy(fixture.workspace.join(file), second.join(file)).unwrap();
    }
    fs::write(
        second.join("package.json"),
        r#"{"dependencies":{"writes-file":"file:../writes.tgz"}}"#,
    )
    .unwrap();
    fixture.pacquet
        .with_arg("install")
        .assert()
        .success();
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&second)
        .with_arg("install")
        .assert()
        .success();
    assert_eq!(fs::read_to_string(fixture.workspace.join("build-count")).unwrap(), "x");
    assert!(!second.join("build-count").exists());
    let output = Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&second)
        .with_args(["exec", "node", "-e", "console.log(require('writes-file'))"])
        .env_remove("NODE_OPTIONS")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "42");
}
