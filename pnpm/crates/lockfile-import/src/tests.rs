use std::path::Path;

use miette::Diagnostic;
use pnpm_resolving_resolver_base::{VersionSelectorEntry, VersionSelectorType};
use pretty_assertions::assert_eq;

use super::{
    ImportLockfileError, VersionsByPackageName, read_foreign_lockfile_versions,
    to_preferred_versions,
};

fn write(dir: &Path, name: &str, contents: &str) {
    std::fs::write(dir.join(name), contents).expect("write lockfile");
}

fn code_of(error: &ImportLockfileError) -> String {
    error.code().expect("error carries a code").to_string()
}

fn names(versions: &VersionsByPackageName) -> Vec<&str> {
    versions.keys().map(String::as_str).collect()
}

#[test]
fn yarn_lock_wins_over_npm_lockfiles() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write(tmp.path(), "yarn.lock", "from-yarn@^1.0.0:\n  version \"1.0.0\"\n");
    write(
        tmp.path(),
        "package-lock.json",
        r#"{"lockfileVersion":1,"dependencies":{"from-npm":{"version":"1.0.0"}}}"#,
    );

    let versions = read_foreign_lockfile_versions(tmp.path()).expect("read");
    assert_eq!(names(&versions), vec!["from-yarn"]);
}

#[test]
fn package_lock_json_wins_over_npm_shrinkwrap_json() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write(
        tmp.path(),
        "package-lock.json",
        r#"{"lockfileVersion":1,"dependencies":{"from-package-lock":{"version":"1.0.0"}}}"#,
    );
    write(
        tmp.path(),
        "npm-shrinkwrap.json",
        r#"{"lockfileVersion":1,"dependencies":{"from-shrinkwrap":{"version":"1.0.0"}}}"#,
    );

    let versions = read_foreign_lockfile_versions(tmp.path()).expect("read");
    assert_eq!(names(&versions), vec!["from-package-lock"]);
}

#[test]
fn npm_shrinkwrap_json_is_read_on_its_own() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write(
        tmp.path(),
        "npm-shrinkwrap.json",
        r#"{"lockfileVersion":1,"dependencies":{"from-shrinkwrap":{"version":"1.0.0"}}}"#,
    );

    let versions = read_foreign_lockfile_versions(tmp.path()).expect("read");
    assert_eq!(names(&versions), vec!["from-shrinkwrap"]);
}

#[test]
fn a_directory_without_a_foreign_lockfile_is_an_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let error = read_foreign_lockfile_versions(tmp.path()).expect_err("no lockfile");
    assert_eq!(error.to_string(), "No lockfile found");
    assert_eq!(code_of(&error), "ERR_PNPM_LOCKFILE_NOT_FOUND");
}

#[test]
fn a_conflicted_yarn_lock_is_an_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write(
        tmp.path(),
        "yarn.lock",
        "<<<<<<< HEAD\nis-positive@^1.0.0:\n  version \"1.0.0\"\n=======\nis-positive@^2.0.0:\n  version \"2.0.0\"\n>>>>>>> other\n",
    );

    let error = read_foreign_lockfile_versions(tmp.path()).expect_err("conflicted yarn.lock");
    assert_eq!(error.to_string(), "Yarn.lock file was conflict");
    assert_eq!(code_of(&error), "ERR_PNPM_YARN_LOCKFILE_PARSE_FAILED");
}

/// A `yarn.lock` pacquet cannot parse must fail the import rather than
/// yield no preferences, which would silently re-resolve every range.
#[test]
fn an_unparsable_yarn_lock_is_an_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write(tmp.path(), "yarn.lock", "is-positive@^1.0.0\n  version \"1.0.0\"\n");

    let error = read_foreign_lockfile_versions(tmp.path()).expect_err("malformed yarn.lock");
    assert_eq!(code_of(&error), "ERR_PNPM_YARN_LOCKFILE_PARSE_FAILED");
    assert!(error.to_string().contains("yarn.lock"), "got {error}");
}

#[test]
fn an_unparsable_npm_lockfile_names_the_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write(tmp.path(), "package-lock.json", "{ not json");

    let error = read_foreign_lockfile_versions(tmp.path()).expect_err("invalid json");
    assert!(error.to_string().contains("package-lock.json"), "got {error}");
}

#[test]
fn every_collected_version_becomes_a_plain_version_selector() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write(
        tmp.path(),
        "package-lock.json",
        r#"{"lockfileVersion":1,"dependencies":{"is-positive":{"version":"1.0.0"},"is-negative":{"version":"2.1.0"}}}"#,
    );

    let versions = read_foreign_lockfile_versions(tmp.path()).expect("read");
    let preferred_versions = to_preferred_versions(&versions);

    assert_eq!(
        preferred_versions.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["is-negative", "is-positive"],
    );
    assert_eq!(
        preferred_versions["is-positive"].get("1.0.0"),
        Some(&VersionSelectorEntry::Plain(VersionSelectorType::Version)),
    );
}
