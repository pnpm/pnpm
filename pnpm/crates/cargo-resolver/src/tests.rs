use super::{latest_version, missing_index_names, resolve_lockfile};
use cargo_lock::Lockfile;
use std::{collections::BTreeMap, str::FromStr};

const METADATA: &str = r#"{
  "packages": [{
    "id": "path+file:///workspace#app@0.1.0",
    "name": "app",
    "version": "0.1.0",
    "dependencies": [{
      "name": "foo",
      "source": "registry+https://github.com/rust-lang/crates.io-index",
      "req": "^1.0"
    }]
  }],
  "workspace_members": ["path+file:///workspace#app@0.1.0"]
}"#;

const FOO_INDEX: &str = r#"{"name":"foo","vers":"1.0.0","deps":[{"name":"bar","req":"^2","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{},"yanked":false}
{"name":"foo","vers":"1.1.0","deps":[{"name":"bar","req":"^2","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","features":{},"yanked":false}"#;

const BAR_INDEX: &str = r#"{"name":"bar","vers":"2.0.0","deps":[],"cksum":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","features":{},"yanked":false}
{"name":"bar","vers":"2.1.0","deps":[],"cksum":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","features":{},"yanked":true}"#;

const OPTIONAL_FOO_INDEX: &str = r#"{"name":"foo","vers":"1.0.0","deps":[{"name":"bar","req":"^2","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{},"yanked":false}"#;

const DEFAULT_FEATURE_FOO_INDEX: &str = r#"{"name":"foo","vers":"1.0.0","deps":[{"name":"bar","req":"^2","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{},"features2":{"default":["dep:bar"]},"yanked":false,"v":2}"#;

const WORKSPACE_OPTIONAL_METADATA: &str = r#"{
  "packages": [{
    "id": "path+file:///workspace#app@0.1.0",
    "name": "app",
    "version": "0.1.0",
    "dependencies": [{
      "name": "foo",
      "source": "registry+https://github.com/rust-lang/crates.io-index",
      "req": "^1.0",
      "optional": true
    }],
    "features": {"foo": ["dep:foo"]}
  }],
  "workspace_members": ["path+file:///workspace#app@0.1.0"]
}"#;

#[test]
fn discovers_transitive_sparse_index_files() {
    let mut files = BTreeMap::new();
    assert_eq!(missing_index_names(METADATA, &files).unwrap(), ["foo"]);

    files.insert("foo".to_string(), FOO_INDEX.to_string());
    assert_eq!(missing_index_names(METADATA, &files).unwrap(), ["bar"]);

    files.insert("bar".to_string(), BAR_INDEX.to_string());
    assert!(missing_index_names(METADATA, &files).unwrap().is_empty());
}

#[test]
fn discovers_dependencies_from_every_viable_version() {
    let foo_index = r#"{"name":"foo","vers":"1.0.0","deps":[{"name":"old-dependency","req":"^1","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{},"yanked":false}
{"name":"foo","vers":"1.1.0","deps":[{"name":"new-dependency","req":"^1","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","features":{},"yanked":false}"#;
    let files = BTreeMap::from([("foo".to_string(), foo_index.to_string())]);

    assert_eq!(
        missing_index_names(METADATA, &files).unwrap(),
        ["new-dependency", "old-dependency"],
    );
}

#[test]
fn selects_the_latest_stable_non_yanked_version() {
    let index = r#"{"name":"foo","vers":"2.0.0-alpha.1","deps":[],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{},"yanked":false}
{"name":"foo","vers":"1.1.0","deps":[],"cksum":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","features":{},"yanked":false}
{"name":"foo","vers":"1.2.0","deps":[],"cksum":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","features":{},"yanked":true}"#;

    assert_eq!(latest_version("foo", index).unwrap(), "1.1.0");
}

#[test]
fn resolves_newest_non_yanked_versions_into_a_cargo_lockfile() {
    let files = BTreeMap::from([
        ("bar".to_string(), BAR_INDEX.to_string()),
        ("foo".to_string(), FOO_INDEX.to_string()),
    ]);
    let encoded = resolve_lockfile(METADATA, &files).unwrap();
    let lockfile = Lockfile::from_str(&encoded).unwrap();

    assert_eq!(lockfile.version, cargo_lock::ResolveVersion::V4);
    assert_eq!(lockfile.packages.len(), 3);
    assert!(lockfile.packages.iter().any(|package| {
        package.name.as_str() == "foo" && package.version == semver::Version::new(1, 1, 0)
    }));
    assert!(lockfile.packages.iter().any(|package| {
        package.name.as_str() == "bar" && package.version == semver::Version::new(2, 0, 0)
    }));
    assert!(
        lockfile
            .packages
            .iter()
            .any(|package| package.name.as_str() == "app" && package.source.is_none()),
    );
}

#[test]
fn resolves_the_feature_unified_lock_graph() {
    let files = BTreeMap::from([("foo".to_string(), OPTIONAL_FOO_INDEX.to_string())]);
    assert!(missing_index_names(METADATA, &files).unwrap().is_empty());
    assert_eq!(
        missing_index_names(WORKSPACE_OPTIONAL_METADATA, &BTreeMap::new()).unwrap(),
        ["foo"],
    );

    let files = BTreeMap::from([("foo".to_string(), DEFAULT_FEATURE_FOO_INDEX.to_string())]);
    assert_eq!(missing_index_names(METADATA, &files).unwrap(), ["bar"]);

    let files = BTreeMap::from([
        ("bar".to_string(), BAR_INDEX.to_string()),
        ("foo".to_string(), DEFAULT_FEATURE_FOO_INDEX.to_string()),
    ]);
    let lockfile = Lockfile::from_str(&resolve_lockfile(METADATA, &files).unwrap()).unwrap();
    assert_eq!(lockfile.packages.len(), 3);
}
