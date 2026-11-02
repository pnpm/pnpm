//! Installs that read pnpr's own package document from a registry declared
//! `serverType: pnpr`, and what an undeclared pnpr gets instead.

use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, Ecosystem, declare_pnpr_registries, fs,
    point_npmrc_registry_at, start_pnpr_registry, start_pnpr_registry_with,
};
use assert_cmd::assert::OutputAssertExt;
use std::io::Write as _;

/// A `.tgz` of `split-pkg@<version>`.
fn package_tarball(version: &str) -> Vec<u8> {
    let manifest = format!(r#"{{"name":"split-pkg","version":"{version}","main":"index.js"}}"#);
    let mut tar = tar::Builder::new(Vec::new());
    for (path, body) in [
        ("package/package.json", manifest.as_bytes()),
        ("package/index.js", b"module.exports = 1\n"),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, path, body).expect("append package file");
    }
    let tar = tar.into_inner().expect("finish package tar");
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(&tar).expect("compress package tar");
    gzip.finish().expect("finish package tarball")
}

/// An upstream serving `split-pkg` 1.0.0, 1.1.0, and 2.0.0, with the tarball
/// of 1.0.0.
fn split_pkg_upstream() -> mockito::ServerGuard {
    let mut upstream = mockito::Server::new();
    let tarball = package_tarball("1.0.0");
    let integrity = ssri::IntegrityOpts::new()
        .algorithm(ssri::Algorithm::Sha512)
        .chain(&tarball)
        .result();
    let version = |version: &str| {
        serde_json::json!({
            "name": "split-pkg",
            "version": version,
            "dist": {
                "tarball": format!("{}/split-pkg/-/split-pkg-{version}.tgz", upstream.url()),
                "integrity": if version == "1.0.0" { integrity.to_string() } else { "sha512-AAAA".into() },
            },
        })
    };
    let packument = serde_json::json!({
        "name": "split-pkg",
        "dist-tags": { "latest": "2.0.0" },
        "time": {
            "1.0.0": "2025-01-01T00:00:00.000Z",
            "1.1.0": "2025-06-01T00:00:00.000Z",
            "2.0.0": "2026-01-01T00:00:00.000Z",
        },
        "versions": { "1.0.0": version("1.0.0"), "1.1.0": version("1.1.0"), "2.0.0": version("2.0.0") },
    });
    upstream
        .mock("GET", "/split-pkg")
        .with_body(packument.to_string())
        .create();
    upstream
        .mock("GET", "/split-pkg/-/split-pkg-1.0.0.tgz")
        .with_body(tarball)
        .create();
    upstream
}

/// A project depending on `split-pkg@1.0.0` from `registry`.
fn project_on(registry: &str) -> CommandTempCwd<AddMockedRegistry> {
    let command = CommandTempCwd::init().add_mocked_registry();
    point_npmrc_registry_at(&command.npmrc_info.npmrc_path, registry);
    fs::write(
        command.workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "split-pkg": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");
    command
}

/// 1.0.0 is not a likely pick of a split document, so its manifest and
/// integrity come from its release record.
#[test]
fn a_version_left_out_of_a_split_document_is_read_from_its_release_record() {
    let upstream = split_pkg_upstream();
    let registry = start_pnpr_registry_with(&upstream.url(), Ecosystem::Npm, |config| {
        config.http.package_documents.split_threshold = 0;
    });
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = project_on(&registry);
    declare_pnpr_registries(&workspace, &[&registry]);

    pacquet
        .with_arg("install")
        .assert()
        .success();

    assert!(workspace.join("node_modules/split-pkg/index.js").exists());
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(lockfile.contains("split-pkg@1.0.0:"), "{lockfile}");
    assert!(lockfile.contains("integrity: sha512-"), "{lockfile}");
    let cache_dir = &npmrc_info.cache_dir;
    assert!(cache_dir.join("v12/metadata-pnpr-records").exists(), "the release record is cached");
    assert!(cache_dir.join("v12/metadata-pnpr").exists());
    drop((root, npmrc_info));
}

#[test]
fn an_undeclared_pnpr_installs_from_npms_documents_with_a_warning() {
    let upstream = split_pkg_upstream();
    let registry = start_pnpr_registry(&upstream.url(), Ecosystem::Npm);
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = project_on(&registry);

    let output = pacquet
        .with_arg("install")
        .assert()
        .success()
        .get_output()
        .clone();

    let printed = String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr);
    assert!(printed.contains("is a pnpr registry"), "{printed}");
    assert!(workspace.join("node_modules/split-pkg/index.js").exists());
    assert!(!npmrc_info.cache_dir.join("v12/metadata-pnpr").exists());
    drop((root, npmrc_info));
}

#[test]
fn a_minimum_release_age_longer_than_30_days_is_refused_for_a_pnpr_registry() {
    let upstream = split_pkg_upstream();
    let registry = start_pnpr_registry(&upstream.url(), Ecosystem::Npm);
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = project_on(&registry);
    declare_pnpr_registries(&workspace, &[&registry]);
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    yaml.push_str("minimumReleaseAge: 43201\n");
    fs::write(&workspace_yaml, yaml).expect("write pnpm-workspace.yaml");

    let output = pacquet
        .with_arg("install")
        .assert()
        .failure()
        .get_output()
        .clone();

    let printed = String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr);
    assert!(printed.contains("ERR_PNPM_UNSUPPORTED_MINIMUM_RELEASE_AGE"), "{printed}");
    drop((root, npmrc_info));
}
