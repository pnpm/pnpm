use super::{
    BAR_INDEX,
    FOO_INDEX,
    METADATA,
};
use crate::{
    registry::CRATES_IO_SOURCE,
    resolve_lockfile,
    verify_lockfile,
};
use std::{
    collections::BTreeMap,
    str::FromStr,
};

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
fn rejects_a_path_dependency_the_lockfile_does_not_lock() {
    let metadata = with_app_dependency(r#""name": "baz", "source": null, "req": "*""#);

    let error = verify_lockfile(&metadata, &locked_foo_and_bar()).unwrap_err();

    assert_eq!(error.to_string(), "app depends on baz *, but Cargo.lock locks no baz for app");
}

/// `app` depends on `libfoo`, a path dependency outside the workspace, asking
/// for its `x` feature. `x` activates `foo`, and `libfoo`'s dev-dependency on
/// `bar` is not locked, as Cargo locks no dev-dependency outside the
/// workspace.
const PATH_PACKAGE_METADATA: &str = r#"{
  "packages": [
    {
      "id": "path+file:///workspace#app@0.1.0",
      "name": "app",
      "version": "0.1.0",
      "dependencies": [{
        "name": "libfoo",
        "source": null,
        "req": "*",
        "features": ["x"]
      }]
    },
    {
      "id": "path+file:///libfoo#0.2.0",
      "name": "libfoo",
      "version": "0.2.0",
      "dependencies": [
        {
          "name": "foo",
          "source": "registry+https://github.com/rust-lang/crates.io-index",
          "req": "^1.0",
          "optional": true
        },
        {
          "name": "bar",
          "source": "registry+https://github.com/rust-lang/crates.io-index",
          "req": "^2",
          "kind": "dev"
        }
      ],
      "features": {"x": ["dep:foo"]}
    }
  ],
  "workspace_members": ["path+file:///workspace#app@0.1.0"]
}"#;

fn locked_path_package() -> String {
    let files = BTreeMap::from([
        ("bar".to_string(), BAR_INDEX.to_string()),
        ("foo".to_string(), FOO_INDEX.to_string()),
    ]);
    resolve_lockfile(PATH_PACKAGE_METADATA, &files, CRATES_IO_SOURCE).unwrap()
}

#[test]
fn locks_a_path_dependency_outside_the_workspace() {
    let lockfile = cargo_lock::Lockfile::from_str(&locked_path_package()).unwrap();
    let libfoo = lockfile.packages
        .iter()
        .find(|package| package.name.as_str() == "libfoo")
        .expect("libfoo is locked");
    let edges = libfoo.dependencies
        .iter()
        .map(|edge| edge.name.as_str())
        .collect::<Vec<_>>();

    assert!(libfoo.source.is_none());
    assert_eq!(edges, ["foo"]);
    verify_lockfile(PATH_PACKAGE_METADATA, &locked_path_package()).unwrap();
}

#[test]
fn rejects_an_edge_a_path_dependency_no_longer_activates() {
    let metadata = PATH_PACKAGE_METADATA.replace(r#""features": ["x"]"#, r#""features": []"#);

    let error = verify_lockfile(&metadata, &locked_path_package()).unwrap_err();

    assert_eq!(
        error.to_string(),
        "Cargo.lock locks foo 1.1.0 for libfoo, which no dependency of libfoo accounts for",
    );
}

#[test]
fn rejects_a_lockfile_without_a_feature_activated_path_dependency_edge() {
    let metadata = PATH_PACKAGE_METADATA.replace(r#""features": ["x"]"#, r#""features": []"#);
    let unlocked = resolve_lockfile(
        &metadata,
        &BTreeMap::from([("foo".to_string(), FOO_INDEX.to_string())]),
        CRATES_IO_SOURCE,
    )
    .unwrap();

    let error = verify_lockfile(PATH_PACKAGE_METADATA, &unlocked).unwrap_err();

    assert_eq!(
        error.to_string(),
        "libfoo depends on foo ^1.0, but Cargo.lock locks no foo for libfoo",
    );
}

#[test]
fn rejects_a_lockfile_with_another_version_of_a_path_dependency() {
    let metadata = PATH_PACKAGE_METADATA.replace(r#""version": "0.2.0""#, r#""version": "0.3.0""#);

    let error = verify_lockfile(&metadata, &locked_path_package()).unwrap_err();

    assert_eq!(
        error.to_string(),
        "app depends on libfoo =0.3.0, but Cargo.lock locks libfoo 0.2.0",
    );
}

const GIT_FOO_LOCKFILE: &str = r#"version = 4

[[package]]
name = "app"
version = "0.1.0"
dependencies = [
 "foo",
]

[[package]]
name = "foo"
version = "1.0.0"
source = "git+https://example.test/foo?branch=main#0123456789abcdef0123456789abcdef01234567"
"#;

#[test]
fn accepts_a_git_dependency_locked_at_its_reference() {
    let metadata = METADATA.replace(CRATES_IO_SOURCE, "git+https://example.test/foo?branch=main");

    verify_lockfile(&metadata, GIT_FOO_LOCKFILE).unwrap();
}

#[test]
fn rejects_a_git_dependency_locked_at_another_reference() {
    let metadata = METADATA.replace(CRATES_IO_SOURCE, "git+https://example.test/foo?rev=abc123");

    let error = verify_lockfile(&metadata, GIT_FOO_LOCKFILE).unwrap_err();

    assert_eq!(
        error.to_string(),
        "app depends on foo ^1.0 from git+https://example.test/foo?rev=abc123, but Cargo.lock locks foo 1.0.0 from git+https://example.test/foo?branch=main#0123456789abcdef0123456789abcdef01234567",
    );
}

#[test]
fn accepts_a_git_dependency_a_patch_locks_from_another_repository() {
    let metadata = METADATA.replace(CRATES_IO_SOURCE, "git+https://example.test/upstream");

    verify_lockfile(&metadata, GIT_FOO_LOCKFILE).unwrap();
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

    assert_eq!(error.to_string(), "app depends on foo ^1.0, but Cargo.lock locks foo 1.1.0");
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

    assert_eq!(error.to_string(), "app depends on foo ^2, but Cargo.lock locks foo 1.0.0-alpha.1");
}

fn with_app_dependency(dependency: &str) -> String {
    METADATA.replace(
        r#""req": "^1.0"
    }]"#,
        &format!(r#""req": "^1.0" }}, {{ {dependency} }}]"#),
    )
}

/// `app` depends on two path packages named `foo`, renamed and without a
/// version requirement, so only their directories tell them apart.
const TWO_LOCAL_FOO_METADATA: &str = r#"{
  "packages": [
    {
      "id": "path+file:///workspace#app@0.1.0",
      "name": "app",
      "version": "0.1.0",
      "manifest_path": "/workspace/Cargo.toml",
      "dependencies": [
        {"name": "foo", "source": null, "req": "*", "rename": "foo1", "path": "/libs/foo1"},
        {"name": "foo", "source": null, "req": "*", "rename": "foo2", "path": "/libs/foo2"}
      ]
    },
    {
      "id": "path+file:///libs/foo1#foo@1.0.0",
      "name": "foo",
      "version": "1.0.0",
      "manifest_path": "/libs/foo1/Cargo.toml",
      "dependencies": []
    },
    {
      "id": "path+file:///libs/foo2#foo@2.0.0",
      "name": "foo",
      "version": "2.0.0",
      "manifest_path": "/libs/foo2/Cargo.toml",
      "dependencies": []
    }
  ],
  "workspace_members": ["path+file:///workspace#app@0.1.0"]
}"#;

#[test]
fn locks_each_path_dependency_against_the_package_in_its_directory() {
    let lockfile =
        resolve_lockfile(TWO_LOCAL_FOO_METADATA, &BTreeMap::new(), CRATES_IO_SOURCE).unwrap();
    let parsed = cargo_lock::Lockfile::from_str(&lockfile).unwrap();
    let app = parsed.packages
        .iter()
        .find(|package| package.name.as_str() == "app")
        .expect("app is locked");
    let edges = app.dependencies
        .iter()
        .map(|edge| edge.version.to_string())
        .collect::<Vec<_>>();
    let reduced = crate::resolve_inputs(TWO_LOCAL_FOO_METADATA).unwrap();

    assert_eq!(edges, ["1.0.0", "2.0.0"]);
    assert_eq!(parsed.packages.len(), 3);
    verify_lockfile(TWO_LOCAL_FOO_METADATA, &lockfile).unwrap();
    assert!(!reduced.contains("/libs"), "{reduced}");
    assert_eq!(resolve_lockfile(&reduced, &BTreeMap::new(), CRATES_IO_SOURCE).unwrap(), lockfile);
}

#[test]
fn rejects_an_edge_to_an_older_version_of_a_path_dependency() {
    let metadata =
        TWO_LOCAL_FOO_METADATA.replace(r#""path": "/libs/foo1""#, r#""path": "/libs/foo2""#);
    let lockfile =
        resolve_lockfile(TWO_LOCAL_FOO_METADATA, &BTreeMap::new(), CRATES_IO_SOURCE).unwrap();

    let error = verify_lockfile(&metadata, &lockfile).unwrap_err();

    assert_eq!(
        error.to_string(),
        "Cargo.lock locks foo 1.0.0 for app, which no dependency of app accounts for",
    );
}

#[test]
fn rejects_a_path_package_outside_the_declared_requirement() {
    let metadata = TWO_LOCAL_FOO_METADATA.replace(
        r#""req": "*", "rename": "foo2""#,
        r#""req": "^1", "rename": "foo2""#,
    );

    let error = resolve_lockfile(&metadata, &BTreeMap::new(), CRATES_IO_SOURCE).unwrap_err();

    assert_eq!(error.to_string(), "path dependency foo ^1 does not match the package at its path");
}

#[test]
fn ignores_the_version_of_a_dev_dependency_outside_the_workspace() {
    let metadata = TWO_LOCAL_FOO_METADATA.replace(
        r#""manifest_path": "/libs/foo1/Cargo.toml",
      "dependencies": []"#,
        r#""manifest_path": "/libs/foo1/Cargo.toml",
      "dependencies": [
        {"name": "foo", "source": null, "req": "^3", "kind": "dev", "path": "/libs/foo2"}
      ]"#,
    );
    assert_ne!(metadata, TWO_LOCAL_FOO_METADATA);

    let lockfile = resolve_lockfile(&metadata, &BTreeMap::new(), CRATES_IO_SOURCE).unwrap();

    verify_lockfile(&metadata, &lockfile).unwrap();
}
