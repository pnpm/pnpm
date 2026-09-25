use super::{PublishArgs, parse_new_version, set_package_version};
use crate::cli_args::publish::tests::{publish_args, publish_args_with, publish_flags};
use pnpm_config::Config;
use serde_json::{Value, json};

fn args_with_new_version(new_version: &str) -> PublishArgs {
    publish_args_with(crate::cli_args::publish::PublishFlags {
        manifest: crate::cli_args::publish::PublishManifestArgs {
            new_version: Some(new_version.to_owned()),
            ..publish_flags().manifest
        },
        ..publish_flags()
    })
}

fn error_code(error: &miette::Report) -> Option<String> {
    error.code().map(|code| code.to_string())
}

#[test]
fn validate_new_version_rejects_an_invalid_semver() {
    let error = args_with_new_version("not-a-version")
        .validate_new_version()
        .expect_err("an invalid semver must be rejected");
    assert_eq!(error_code(&error).as_deref(), Some("ERR_PNPM_INVALID_VERSION_BUMP"));
}

#[test]
fn validate_new_version_rejects_a_tarball_argument() {
    let mut args = args_with_new_version("1.2.3");
    args.package = Some("pkg-1.0.0.tgz".to_owned());
    let error =
        args.validate_new_version().expect_err("--new-version with a tarball must be rejected");
    assert_eq!(error_code(&error).as_deref(), Some("ERR_PNPM_NEW_VERSION_WITH_TARBALL"));
}

#[test]
fn validate_new_version_accepts_a_directory_argument() {
    let mut args = args_with_new_version("1.2.3");
    args.package = Some("subpkg".to_owned());
    args.validate_new_version().expect("a directory argument is fine");
}

#[test]
fn parse_new_version_strips_a_leading_v() {
    assert_eq!(parse_new_version("v1.2.3").expect("v-prefixed semver parses"), "1.2.3");
}

#[test]
fn no_new_version_flag_validates_and_applies_as_a_noop() {
    let args = publish_args();
    args.validate_new_version().expect("no flag validates");
    let dir = tempfile::tempdir().expect("a project dir");
    args.apply_new_version(dir.path(), &Config::default(), false)
        .expect("no flag applies as a no-op");
    assert!(!dir.path().join("package.json").exists());
}

#[test]
fn apply_new_version_rewrites_the_project_manifest() {
    let dir = tempfile::tempdir().expect("a project dir");
    let manifest_path = dir.path().join("package.json");
    std::fs::write(&manifest_path, r#"{"name":"pkg","version":"1.0.0"}"#)
        .expect("write package.json");

    args_with_new_version("2.0.0-alpha.1")
        .apply_new_version(dir.path(), &Config::default(), false)
        .expect("the version is applied");

    let written: Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).expect("read package.json"))
            .expect("parse package.json");
    assert_eq!(written["version"], "2.0.0-alpha.1");
    assert_eq!(written["name"], "pkg");
}

#[test]
fn apply_new_version_resolves_a_relative_package_argument() {
    let dir = tempfile::tempdir().expect("a command dir");
    let package_dir = dir.path().join("subpkg");
    std::fs::create_dir(&package_dir).expect("create the package dir");
    std::fs::write(package_dir.join("package.json"), r#"{"name":"pkg","version":"1.0.0"}"#)
        .expect("write package.json");
    let mut args = args_with_new_version("3.0.0");
    args.package = Some("subpkg".to_owned());

    args.apply_new_version(dir.path(), &Config::default(), false)
        .expect("the version is applied to the argument directory");

    let written: Value = serde_json::from_str(
        &std::fs::read_to_string(package_dir.join("package.json")).expect("read package.json"),
    )
    .expect("parse package.json");
    assert_eq!(written["version"], "3.0.0");
}

#[test]
fn set_package_version_skips_a_nameless_manifest() {
    let dir = tempfile::tempdir().expect("a project dir");
    let manifest_path = dir.path().join("package.json");
    std::fs::write(&manifest_path, r#"{"version":"1.0.0"}"#).expect("write package.json");

    set_package_version(dir.path(), "2.0.0").expect("a nameless manifest is skipped");

    let written: Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).expect("read package.json"))
            .expect("parse package.json");
    assert_eq!(written["version"], "1.0.0");
}

#[test]
fn set_package_version_ignores_a_missing_manifest() {
    let dir = tempfile::tempdir().expect("an empty dir");
    set_package_version(dir.path(), "2.0.0").expect("a missing manifest is left to the pack");
    assert!(!dir.path().join("package.json").exists());
}

#[test]
fn apply_new_version_recursive_bumps_every_selected_package() {
    let dir = tempfile::tempdir().expect("a workspace dir");
    std::fs::write(dir.path().join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    for (name, manifest) in [
        ("pkg-a", json!({ "name": "pkg-a", "version": "1.0.0" })),
        ("pkg-b", json!({ "name": "pkg-b", "version": "2.0.0", "private": true })),
    ] {
        let package_dir = dir.path().join("packages").join(name);
        std::fs::create_dir_all(&package_dir).expect("create the package dir");
        std::fs::write(package_dir.join("package.json"), manifest.to_string())
            .expect("write package.json");
    }

    args_with_new_version("3.0.0")
        .apply_new_version(dir.path(), &Config::default(), true)
        .expect("the version is applied recursively");

    for name in ["pkg-a", "pkg-b"] {
        let written: Value = serde_json::from_str(
            &std::fs::read_to_string(
                dir.path()
                    .join("packages")
                    .join(name)
                    .join("package.json"),
            )
            .expect("read package.json"),
        )
        .expect("parse package.json");
        assert_eq!(written["version"], "3.0.0", "{name} must be bumped");
    }
}
