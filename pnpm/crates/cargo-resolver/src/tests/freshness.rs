use super::{BAR_INDEX, FOO_INDEX, METADATA};
use crate::{registry::CRATES_IO_SOURCE, resolve_lockfile, verify_lockfile};
use std::collections::BTreeMap;

fn locked_foo_and_bar() -> String {
    let files = BTreeMap::from([
        ("bar".to_string(), BAR_INDEX.to_string()),
        ("foo".to_string(), FOO_INDEX.to_string()),
    ]);
    resolve_lockfile(METADATA, &files, CRATES_IO_SOURCE).unwrap()
}

#[test]
fn accepts_a_lockfile_that_satisfies_the_workspace_requirements() {
    verify_lockfile(METADATA, &locked_foo_and_bar()).unwrap();
}

#[test]
fn rejects_a_locked_version_outside_the_workspace_requirement() {
    let metadata = METADATA.replace("^1.0", "=1.0.5");

    let error = verify_lockfile(&metadata, &locked_foo_and_bar()).unwrap_err();

    assert_eq!(error.to_string(), "app depends on foo =1.0.5, but Cargo.lock locks foo 1.1.0");
}

#[test]
fn rejects_a_dependency_the_lockfile_does_not_lock() {
    let metadata = METADATA.replace(r#""name": "foo""#, r#""name": "baz""#);

    let error = verify_lockfile(&metadata, &locked_foo_and_bar()).unwrap_err();

    assert_eq!(error.to_string(), "app depends on baz ^1.0, but Cargo.lock locks no baz for app");
}

#[test]
fn rejects_a_direct_dependency_locked_only_transitively() {
    let metadata = with_app_dependency(&format!(
        r#""name": "bar", "source": "{CRATES_IO_SOURCE}", "req": "^2""#,
    ));

    let error = verify_lockfile(&metadata, &locked_foo_and_bar()).unwrap_err();

    assert_eq!(error.to_string(), "app depends on bar ^2, but Cargo.lock locks no bar for app");
}

#[test]
fn rejects_an_edge_no_dependency_requires() {
    let metadata = METADATA.replace(r#""name": "foo""#, r#""name": "bar""#).replace("^1.0", "^2");
    let lockfile = locked_foo_and_bar()
        .replace("dependencies = [\n \"foo\",\n]", "dependencies = [\n \"bar\",\n \"foo\",\n]");

    let error = verify_lockfile(&metadata, &lockfile).unwrap_err();

    assert_eq!(
        error.to_string(),
        "Cargo.lock locks foo 1.1.0 for app, which no dependency of app accounts for",
    );
}

#[test]
fn rejects_a_lockfile_without_the_workspace_member() {
    let metadata = METADATA.replace(r#""name": "app""#, r#""name": "renamed""#);

    let error = verify_lockfile(&metadata, &locked_foo_and_bar()).unwrap_err();

    assert_eq!(error.to_string(), "Cargo.lock does not lock the workspace member renamed 0.1.0");
}

#[test]
fn reports_a_stale_lockfile_as_outdated() {
    let metadata = METADATA.replace("^1.0", "=1.0.5");

    let error = verify_lockfile(&metadata, &locked_foo_and_bar()).unwrap_err();

    assert_eq!(
        error
            .code()
            .map(|code| code.to_string())
            .as_deref(),
        Some("ERR_PNPM_OUTDATED_LOCKFILE"),
    );
}

#[test]
fn ignores_path_dependencies_the_lockfile_does_not_lock() {
    let metadata = with_app_dependency(r#""name": "baz", "source": null, "req": "*""#);

    verify_lockfile(&metadata, &locked_foo_and_bar()).unwrap();
}

const TWO_FOO_LOCKFILE: &str = r#"version = 4

[[package]]
name = "app"
version = "0.1.0"
dependencies = [
 "foo 1.0.0",
 "foo 2.0.0",
]

[[package]]
name = "foo"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "foo"
version = "2.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;

#[test]
fn accepts_one_declaration_per_locked_edge() {
    let metadata = with_app_dependency(&format!(
        r#""name": "foo", "source": "{CRATES_IO_SOURCE}", "req": "^2", "rename": "foo2""#,
    ));

    verify_lockfile(&metadata, TWO_FOO_LOCKFILE).unwrap();
}

#[test]
fn rejects_two_edges_one_declaration_accounts_for() {
    let metadata = METADATA.replace("^1.0", ">=1, <3");

    let error = verify_lockfile(&metadata, TWO_FOO_LOCKFILE).unwrap_err();

    assert_eq!(
        error.to_string(),
        "Cargo.lock locks foo 2.0.0 for app, which no dependency of app accounts for",
    );
}

#[test]
fn accepts_declarations_that_share_one_edge() {
    let metadata = with_app_dependency(&format!(
        r#""name": "foo", "source": "{CRATES_IO_SOURCE}", "req": "^1.1", "kind": "dev""#,
    ));

    verify_lockfile(&metadata, &locked_foo_and_bar()).unwrap();
}

#[test]
fn rejects_a_registry_edge_for_a_path_dependency() {
    let metadata = METADATA.replace(&format!(r#""{CRATES_IO_SOURCE}""#), "null");

    let error = verify_lockfile(&metadata, &locked_foo_and_bar()).unwrap_err();

    assert_eq!(
        error.to_string(),
        "Cargo.lock locks foo 1.1.0 for app, which no dependency of app accounts for",
    );
}

const PATH_FOO_LOCKFILE: &str = r#"version = 4

[[package]]
name = "app"
version = "0.1.0"
dependencies = [
 "foo",
]

[[package]]
name = "foo"
version = "1.0.0-alpha.1"
"#;

#[test]
fn accepts_an_unversioned_path_dependency_on_a_prerelease() {
    let metadata = METADATA
        .replace(&format!(r#""{CRATES_IO_SOURCE}""#), "null")
        .replace("^1.0", "*");

    verify_lockfile(&metadata, PATH_FOO_LOCKFILE).unwrap();
}

#[test]
fn accepts_an_unversioned_git_dependency_on_a_prerelease() {
    let metadata =
        METADATA.replace(CRATES_IO_SOURCE, "git+https://example.test/foo").replace("^1.0", "*");
    let lockfile = PATH_FOO_LOCKFILE.replace(
        r#"version = "1.0.0-alpha.1""#,
        "version = \"1.0.0-alpha.1\"\nsource = \"git+https://example.test/foo#0123456789abcdef0123456789abcdef01234567\"",
    );

    verify_lockfile(&metadata, &lockfile).unwrap();
}

#[test]
fn rejects_a_path_edge_outside_the_path_dependency_requirement() {
    let metadata = METADATA
        .replace(&format!(r#""{CRATES_IO_SOURCE}""#), "null")
        .replace("^1.0", "^2");

    let error = verify_lockfile(&metadata, PATH_FOO_LOCKFILE).unwrap_err();

    assert_eq!(
        error.to_string(),
        "Cargo.lock locks foo 1.0.0-alpha.1 for app, which no dependency of app accounts for",
    );
}

fn with_app_dependency(dependency: &str) -> String {
    METADATA.replace(
        r#""req": "^1.0"
    }]"#,
        &format!(r#""req": "^1.0" }}, {{ {dependency} }}]"#),
    )
}
