use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_graph_hasher::{host_arch, host_platform};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::TempDir;

struct RuntimeFixture {
    name: &'static str,
    version: &'static str,
    archive_mock: mockito::Mock,
    resolution: Value,
}

#[test]
fn installs_node_deno_and_bun_then_reinstalls_them_offline() {
    let root = tempfile::tempdir().unwrap();
    let workspace = prepare_workspace(&root, "");
    let mut server = mockito::Server::new();
    let fixtures = [
        runtime_fixture(&mut server, "node", "22.0.0", host_platform(), host_arch()),
        runtime_fixture(&mut server, "deno", "2.4.2", host_platform(), host_arch()),
        runtime_fixture(&mut server, "bun", "1.2.19", host_platform(), host_arch()),
    ];
    write_runtime_manifest(&workspace, &fixtures);
    write_runtime_lockfile(&workspace, &fixtures);

    command(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert_installed(&workspace, &fixtures);
    let node_root = workspace.join("node_modules/node");
    let node_extras = if host_platform() == "win32" {
        [
            "node_modules/npm/package.json",
            "node_modules/corepack/package.json",
            "npm.cmd",
            "npx.cmd",
            "corepack.cmd",
        ]
    } else {
        [
            "lib/node_modules/npm/package.json",
            "lib/node_modules/corepack/package.json",
            "bin/npm",
            "bin/npx",
            "bin/corepack",
        ]
    };
    for relative in node_extras {
        assert!(!node_root.join(relative).exists(), "Node extra was not stripped: {relative}");
    }

    fs::remove_dir_all(workspace.join("node_modules")).unwrap();
    command(&workspace).with_args(["install", "--frozen-lockfile", "--offline"]).assert().success();
    assert_installed(&workspace, &fixtures);
    for fixture in &fixtures {
        fixture.archive_mock.assert();
    }
}

#[test]
fn node_runtime_fails_when_missing_offline() {
    assert_runtime_missing_offline("node", "22.0.0");
}

#[test]
fn deno_runtime_fails_when_missing_offline() {
    assert_runtime_missing_offline("deno", "2.4.2");
}

#[test]
fn bun_runtime_fails_when_missing_offline() {
    assert_runtime_missing_offline("bun", "1.2.19");
}

#[test]
fn node_runtime_fails_on_bad_integrity() {
    assert_runtime_bad_integrity("node", "22.0.0");
}

#[test]
fn deno_runtime_fails_on_bad_integrity() {
    assert_runtime_bad_integrity("deno", "2.4.2");
}

#[test]
fn bun_runtime_fails_on_bad_integrity() {
    assert_runtime_bad_integrity("bun", "1.2.19");
}

#[test]
fn installs_node_runtime_for_the_requested_target_architecture() {
    let root = tempfile::tempdir().unwrap();
    let workspace = prepare_workspace(&root, "");
    let mut server = mockito::Server::new();
    let target_os = if host_platform() == "win32" { "linux" } else { "win32" };
    let fixture = runtime_fixture(&mut server, "node", "22.0.0", target_os, "x64");
    write_runtime_manifest(&workspace, std::slice::from_ref(&fixture));
    write_runtime_lockfile(&workspace, std::slice::from_ref(&fixture));

    command(&workspace)
        .with_args(["install", "--frozen-lockfile", "--os", target_os, "--cpu", "x64"])
        .assert()
        .success();
    let expected_bin = if target_os == "win32" { "node.exe" } else { "bin/node" };
    assert!(workspace.join("node_modules/node").join(expected_bin).exists());
}

#[test]
fn installs_node_runtime_from_the_rc_channel() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new();
    let version = "24.0.0-rc.4";
    let _mocks = mock_node_release(&mut server, version);
    let workspace = prepare_workspace(
        &root,
        format!("nodeDownloadMirrors:\n  rc: '{}/'\n", server.url()).as_str(),
    );
    fs::write(
        workspace.join("package.json"),
        json!({ "dependencies": { "node": format!("runtime:{version}") } }).to_string(),
    )
    .unwrap();

    command(&workspace).with_arg("install").assert().success();
    assert!(workspace.join("node_modules/node/package.json").exists());
    assert!(fs::read_to_string(workspace.join("pnpm-lock.yaml")).unwrap().contains(version));
}

#[test]
fn installs_node_runtime_declared_by_a_dependency_engine() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new();
    let version = "22.19.0-rc.1";
    let _mocks = mock_node_release(&mut server, version);
    let workspace = prepare_workspace(
        &root,
        format!("nodeDownloadMirrors:\n  rc: '{}/'\n", server.url()).as_str(),
    );
    let dependency = workspace.join("dependency");
    fs::create_dir(&dependency).unwrap();
    fs::write(
        dependency.join("package.json"),
        json!({
            "name": "dependency",
            "version": "1.0.0",
            "engines": {
                "runtime": {
                    "name": "node",
                    "version": version,
                    "onFail": "download",
                },
            },
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        workspace.join("package.json"),
        json!({ "dependencies": { "dependency": "file:dependency" } }).to_string(),
    )
    .unwrap();

    command(&workspace).with_arg("install").assert().success();
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).unwrap();
    assert!(
        lockfile.contains(format!("node@runtime:{version}").as_str()),
        "lockfile was:\n{lockfile}",
    );
}

#[test]
fn runtime_on_fail_download_reifies_the_manifest_runtime() {
    let root = tempfile::tempdir().unwrap();
    let workspace = prepare_workspace(&root, "runtimeOnFail: download\n");
    let mut server = mockito::Server::new();
    let fixture = runtime_fixture(&mut server, "node", "24.0.0", host_platform(), host_arch());
    write_devengines_manifest(&workspace, fixture.version, None);
    write_runtime_lockfile_for_group(&workspace, std::slice::from_ref(&fixture), "devDependencies");

    command(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert_installed(&workspace, std::slice::from_ref(&fixture));
}

#[test]
fn devengines_runtime_with_download_is_installed() {
    let root = tempfile::tempdir().unwrap();
    let workspace = prepare_workspace(&root, "");
    let mut server = mockito::Server::new();
    let fixture = runtime_fixture(&mut server, "node", "24.0.0", host_platform(), host_arch());
    write_devengines_manifest(&workspace, fixture.version, Some("download"));
    write_runtime_lockfile_for_group(&workspace, std::slice::from_ref(&fixture), "devDependencies");

    command(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert_installed(&workspace, std::slice::from_ref(&fixture));
}

#[test]
fn devengines_runtime_without_download_is_not_installed() {
    let root = tempfile::tempdir().unwrap();
    let workspace = prepare_workspace(&root, "");
    write_devengines_manifest(&workspace, "24.0.0", None);

    command(&workspace).with_args(["install", "--lockfile-only"]).assert().success();
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).unwrap();
    assert!(!lockfile.contains("node@runtime:"), "lockfile was:\n{lockfile}");
}

#[test]
fn runtime_on_fail_ignore_removes_the_synthesized_runtime_dependency() {
    let root = tempfile::tempdir().unwrap();
    let workspace = prepare_workspace(&root, "runtimeOnFail: ignore\n");
    fs::write(
        workspace.join("package.json"),
        json!({
            "devEngines": {
                "runtime": {
                    "name": "node",
                    "version": "24.0.0",
                    "onFail": "download",
                },
            },
        })
        .to_string(),
    )
    .unwrap();
    command(&workspace).with_args(["install", "--lockfile-only"]).assert().success();
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).unwrap();
    assert!(!lockfile.contains("node@runtime:"), "lockfile was:\n{lockfile}");
}

#[test]
fn explicit_node_version_takes_priority_over_the_manifest_runtime() {
    let root = tempfile::tempdir().unwrap();
    let workspace = prepare_workspace(&root, "engineStrict: true\nnodeVersion: 20.0.0\n");
    let dependency = workspace.join("dependency");
    fs::create_dir(&dependency).unwrap();
    fs::write(
        dependency.join("package.json"),
        json!({ "name": "dependency", "version": "1.0.0", "engines": { "node": "<21" } })
            .to_string(),
    )
    .unwrap();
    fs::write(
        workspace.join("package.json"),
        json!({
            "dependencies": { "dependency": "file:dependency" },
            "devEngines": {
                "runtime": { "name": "node", "version": "^22.0.0", "onFail": "ignore" },
            },
        })
        .to_string(),
    )
    .unwrap();
    command(&workspace).with_arg("install").assert().success();
}

fn assert_runtime_missing_offline(name: &'static str, version: &'static str) {
    let root = tempfile::tempdir().unwrap();
    let workspace = prepare_workspace(&root, "");
    fs::write(
        workspace.join("package.json"),
        json!({ "dependencies": { name: format!("runtime:{version}") } }).to_string(),
    )
    .unwrap();
    let output = command(&workspace).with_args(["install", "--offline"]).assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(
        stderr.to_ascii_lowercase().contains(name),
        "stderr did not identify the missing {name} runtime:\n{stderr}",
    );
}

fn assert_runtime_bad_integrity(name: &'static str, version: &'static str) {
    let root = tempfile::tempdir().unwrap();
    let workspace = prepare_workspace(&root, "");
    let mut server = mockito::Server::new();
    let mut fixture = runtime_fixture(&mut server, name, version, host_platform(), host_arch());
    let bad_integrity = ssri::IntegrityOpts::new()
        .algorithm(ssri::Algorithm::Sha512)
        .chain(b"different runtime archive")
        .result()
        .to_string();
    fixture.resolution["variants"][0]["resolution"]["integrity"] = Value::String(bad_integrity);
    write_runtime_manifest(&workspace, std::slice::from_ref(&fixture));
    write_runtime_lockfile(&workspace, std::slice::from_ref(&fixture));
    let output = command(&workspace).with_args(["install", "--frozen-lockfile"]).assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(stderr.contains("Integrity check failed"), "stderr was:\n{stderr}");
}

fn prepare_workspace(root: &TempDir, extra_yaml: &str) -> PathBuf {
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "storeDir: ../store\ncacheDir: ../cache\nenableGlobalVirtualStore: false\n{extra_yaml}",
        ),
    )
    .unwrap();
    workspace
}

fn runtime_fixture(
    server: &mut mockito::Server,
    name: &'static str,
    version: &'static str,
    target_os: &str,
    target_cpu: &str,
) -> RuntimeFixture {
    let is_zip = name != "node" || target_os == "win32";
    let bin_path = runtime_bin_path(name, target_os);
    let prefix = (name == "bun" || (name == "node" && is_zip)).then(|| format!("{name}-fixture"));
    let bytes = if is_zip {
        build_zip(name, target_os, prefix.as_deref(), name == "node")
    } else {
        build_tarball(name, version, name == "node")
    };
    let archive_path = format!("/{name}-{target_os}-{target_cpu}.archive");
    let archive_mock = server
        .mock("GET", archive_path.as_str())
        .with_status(200)
        .with_body(bytes.clone())
        .expect(1)
        .create();
    let integrity = ssri::IntegrityOpts::new()
        .algorithm(ssri::Algorithm::Sha512)
        .chain(&bytes)
        .result()
        .to_string();
    let mut resolution = json!({
        "type": "binary",
        "archive": if is_zip { "zip" } else { "tarball" },
        "url": format!("{}{archive_path}", server.url()),
        "integrity": integrity,
        "bin": if name == "node" { json!({ "node": bin_path }) } else { json!(bin_path) },
    });
    if let Some(prefix) = prefix {
        resolution["prefix"] = Value::String(prefix);
    }
    RuntimeFixture {
        name,
        version,
        archive_mock,
        resolution: json!({
            "type": "variations",
            "variants": [{
                "targets": [{ "os": target_os, "cpu": target_cpu }],
                "resolution": resolution,
            }],
        }),
    }
}

fn write_runtime_manifest(workspace: &Path, fixtures: &[RuntimeFixture]) {
    let dependencies = fixtures
        .iter()
        .map(|fixture| {
            (fixture.name.to_string(), Value::String(format!("runtime:{}", fixture.version)))
        })
        .collect::<Map<_, _>>();
    fs::write(workspace.join("package.json"), json!({ "dependencies": dependencies }).to_string())
        .unwrap();
}

fn write_runtime_lockfile(workspace: &Path, fixtures: &[RuntimeFixture]) {
    write_runtime_lockfile_for_group(workspace, fixtures, "dependencies");
}

fn write_runtime_lockfile_for_group(
    workspace: &Path,
    fixtures: &[RuntimeFixture],
    dependency_group: &str,
) {
    let mut importer_dependencies = Map::new();
    let mut packages = Map::new();
    let mut snapshots = Map::new();
    for fixture in fixtures {
        let version = format!("runtime:{}", fixture.version);
        let key = format!("{}@{version}", fixture.name);
        importer_dependencies
            .insert(fixture.name.to_string(), json!({ "specifier": version, "version": version }));
        packages.insert(
            key.clone(),
            json!({
                "hasBin": true,
                "resolution": fixture.resolution,
                "version": fixture.version,
            }),
        );
        snapshots.insert(key, json!({}));
    }
    let mut importer = Map::new();
    importer.insert(dependency_group.to_string(), Value::Object(importer_dependencies));
    let lockfile = json!({
        "lockfileVersion": "9.0",
        "importers": { ".": importer },
        "packages": packages,
        "snapshots": snapshots,
    });
    fs::write(workspace.join("pnpm-lock.yaml"), serde_saphyr::to_string(&lockfile).unwrap())
        .unwrap();
}

fn write_devengines_manifest(workspace: &Path, version: &str, on_fail: Option<&str>) {
    let mut runtime = json!({ "name": "node", "version": version });
    if let Some(on_fail) = on_fail {
        runtime["onFail"] = Value::String(on_fail.to_string());
    }
    fs::write(
        workspace.join("package.json"),
        json!({ "devEngines": { "runtime": runtime } }).to_string(),
    )
    .unwrap();
}

fn assert_installed(workspace: &Path, fixtures: &[RuntimeFixture]) {
    for fixture in fixtures {
        assert!(workspace.join("node_modules").join(fixture.name).join("package.json").exists());
        let bin_dir = workspace.join("node_modules/.bin");
        assert!(
            [
                bin_dir.join(fixture.name),
                bin_dir.join(format!("{}.exe", fixture.name)),
                bin_dir.join(format!("{}.cmd", fixture.name)),
            ]
            .iter()
            .any(|path| path.exists()),
            "runtime bin was not linked for {}",
            fixture.name,
        );
    }
}

fn runtime_bin_path(name: &str, target_os: &str) -> String {
    if target_os == "win32" {
        format!("{name}.exe")
    } else if name == "node" {
        "bin/node".to_string()
    } else {
        name.to_string()
    }
}

fn build_tarball(name: &str, version: &str, node_extras: bool) -> Vec<u8> {
    let prefix = format!("{name}-v{version}-fixture");
    let mut tar = tar::Builder::new(Vec::new());
    append_tar(
        &mut tar,
        format!("{prefix}/{}", runtime_bin_path(name, "linux")).as_str(),
        b"#!/bin/sh\nexit 0\n",
        0o755,
    );
    if node_extras {
        for path in [
            "lib/node_modules/npm/package.json",
            "lib/node_modules/corepack/package.json",
            "bin/npm",
            "bin/npx",
            "bin/corepack",
        ] {
            append_tar(&mut tar, format!("{prefix}/{path}").as_str(), b"extra", 0o755);
        }
    }
    let tar = tar.into_inner().unwrap();
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&tar).unwrap();
    encoder.finish().unwrap()
}

fn append_tar(tar: &mut tar::Builder<Vec<u8>>, path: &str, body: &[u8], mode: u32) {
    let mut header = tar::Header::new_gnu();
    header.set_size(body.len() as u64);
    header.set_mode(mode);
    tar.append_data(&mut header, path, body).unwrap();
}

fn build_zip(name: &str, target_os: &str, prefix: Option<&str>, node_extras: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(Cursor::new(&mut bytes));
        let options: zip::write::FileOptions<()> = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .unix_permissions(0o100755);
        let path = |relative: &str| {
            prefix.map_or_else(|| relative.to_string(), |p| format!("{p}/{relative}"))
        };
        writer.start_file(path(runtime_bin_path(name, target_os).as_str()), options).unwrap();
        writer.write_all(b"runtime fixture").unwrap();
        if node_extras {
            for relative in [
                "node_modules/npm/package.json",
                "node_modules/corepack/package.json",
                "npm.cmd",
                "npx.cmd",
                "corepack.cmd",
            ] {
                writer.start_file(path(relative), options).unwrap();
                writer.write_all(b"extra").unwrap();
            }
        }
        writer.finish().unwrap();
    }
    bytes
}

fn node_archive_name(version: &str, platform: &str, arch: &str) -> String {
    let platform = if platform == "win32" { "win" } else { platform };
    let extension = if platform == "win" { "zip" } else { "tar.gz" };
    format!("node-v{version}-{platform}-{arch}.{extension}")
}

fn mock_node_release(server: &mut mockito::Server, version: &str) -> [mockito::Mock; 3] {
    let archive_name = node_archive_name(version, host_platform(), host_arch());
    let archive = if host_platform() == "win32" {
        let prefix = archive_name.strip_suffix(".zip").unwrap();
        build_zip("node", "win32", Some(prefix), true)
    } else {
        build_tarball("node", version, true)
    };
    let digest = format!("{:x}", Sha256::digest(&archive));
    let index = server
        .mock("GET", "/index.json")
        .with_status(200)
        .with_body(format!(r#"[{{"version":"v{version}","lts":false}}]"#))
        .create();
    let shasums = server
        .mock("GET", format!("/v{version}/SHASUMS256.txt").as_str())
        .with_status(200)
        .with_body(format!("{digest}  {archive_name}\n"))
        .create();
    let archive = server
        .mock("GET", format!("/v{version}/{archive_name}").as_str())
        .with_status(200)
        .with_body(archive)
        .create();
    [index, shasums, archive]
}

fn command(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").unwrap().with_current_dir(workspace)
}
