use super::{latest_version, missing_index_names, resolve_inputs, resolve_lockfile};
use crate::registry::CRATES_IO_SOURCE;
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

const SPLIT_DEFAULT_FEATURE_FOO_INDEX: &str = r#"{"name":"foo","vers":"1.0.0","deps":[{"name":"bar","req":"^2","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal","registry":null},{"name":"baz","req":"^1","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{"default":["dep:bar"]},"features2":{"default":["dep:baz"]},"yanked":false,"v":2}"#;

const BAZ_INDEX: &str = r#"{"name":"baz","vers":"1.0.0","deps":[],"cksum":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","features":{},"yanked":false}"#;

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
    assert_eq!(missing_index_names(METADATA, &files, CRATES_IO_SOURCE).unwrap(), ["foo"]);

    files.insert("foo".to_string(), FOO_INDEX.to_string());
    assert_eq!(missing_index_names(METADATA, &files, CRATES_IO_SOURCE).unwrap(), ["bar"]);

    files.insert("bar".to_string(), BAR_INDEX.to_string());
    assert!(missing_index_names(METADATA, &files, CRATES_IO_SOURCE).unwrap().is_empty());
}

#[test]
fn discovers_dependencies_from_every_viable_version() {
    let foo_index = r#"{"name":"foo","vers":"1.0.0","deps":[{"name":"old-dependency","req":"^1","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{},"yanked":false}
{"name":"foo","vers":"1.1.0","deps":[{"name":"new-dependency","req":"^1","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","features":{},"yanked":false}"#;
    let files = BTreeMap::from([("foo".to_string(), foo_index.to_string())]);

    assert_eq!(
        missing_index_names(METADATA, &files, CRATES_IO_SOURCE).unwrap(),
        ["new-dependency", "old-dependency"],
    );
}

#[test]
fn validates_registry_metadata_before_deduplicating_dependencies() {
    let metadata = METADATA.replacen(
        "]\n  }],",
        r#", {
      "name": "foo",
      "source": "registry+https://registry.example.test/index",
      "req": "^1.0"
    }]
  }],"#,
        1,
    );

    let error =
        missing_index_names(&metadata, &BTreeMap::new(), CRATES_IO_SOURCE).unwrap_err().to_string();

    assert!(error.contains("cannot be resolved from"), "{error}");
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
    let encoded = resolve_lockfile(METADATA, &files, CRATES_IO_SOURCE).unwrap();
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
fn writes_the_configured_sparse_registry_source() {
    let files = BTreeMap::from([
        ("foo".to_string(), FOO_INDEX.to_string()),
        ("bar".to_string(), BAR_INDEX.to_string()),
    ]);
    let lockfile =
        resolve_lockfile(METADATA, &files, "sparse+https://registry.example.test/index/").unwrap();

    assert!(lockfile.contains(r#"source = "sparse+https://registry.example.test/index/""#));
}

#[test]
fn resolves_the_feature_unified_lock_graph() {
    let files = BTreeMap::from([("foo".to_string(), OPTIONAL_FOO_INDEX.to_string())]);
    assert!(missing_index_names(METADATA, &files, CRATES_IO_SOURCE).unwrap().is_empty());
    assert_eq!(
        missing_index_names(WORKSPACE_OPTIONAL_METADATA, &BTreeMap::new(), CRATES_IO_SOURCE)
            .unwrap(),
        ["foo"],
    );

    let files = BTreeMap::from([("foo".to_string(), DEFAULT_FEATURE_FOO_INDEX.to_string())]);
    assert_eq!(missing_index_names(METADATA, &files, CRATES_IO_SOURCE).unwrap(), ["bar"]);

    let files = BTreeMap::from([
        ("bar".to_string(), BAR_INDEX.to_string()),
        ("foo".to_string(), DEFAULT_FEATURE_FOO_INDEX.to_string()),
    ]);
    let lockfile =
        Lockfile::from_str(&resolve_lockfile(METADATA, &files, CRATES_IO_SOURCE).unwrap()).unwrap();
    assert_eq!(lockfile.packages.len(), 3);
}

#[test]
fn merges_duplicate_feature_names_across_index_feature_maps() {
    let files = BTreeMap::from([("foo".to_string(), SPLIT_DEFAULT_FEATURE_FOO_INDEX.to_string())]);
    assert_eq!(missing_index_names(METADATA, &files, CRATES_IO_SOURCE).unwrap(), ["bar", "baz"]);

    let files = BTreeMap::from([
        ("bar".to_string(), BAR_INDEX.to_string()),
        ("baz".to_string(), BAZ_INDEX.to_string()),
        ("foo".to_string(), SPLIT_DEFAULT_FEATURE_FOO_INDEX.to_string()),
    ]);
    let lockfile =
        Lockfile::from_str(&resolve_lockfile(METADATA, &files, CRATES_IO_SOURCE).unwrap()).unwrap();
    assert_eq!(lockfile.packages.len(), 4);
}

#[test]
fn propagates_features_from_the_selected_older_candidate() {
    let metadata = r#"{
  "packages": [{
    "id": "path+file:///workspace#app@0.1.0",
    "name": "app",
    "version": "0.1.0",
    "dependencies": [
      {
        "name": "foo",
        "source": "registry+https://github.com/rust-lang/crates.io-index",
        "req": "^1.0"
      },
      {
        "name": "bar",
        "source": "registry+https://github.com/rust-lang/crates.io-index",
        "req": "=1.0.0"
      }
    ]
  }],
  "workspace_members": ["path+file:///workspace#app@0.1.0"]
}"#;
    let foo_index = r#"{"name":"foo","vers":"1.0.0","deps":[{"name":"bar","req":"=1.0.0","features":["extra"],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{},"yanked":false}
{"name":"foo","vers":"1.1.0","deps":[{"name":"bar","req":"=1.1.0","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","features":{},"yanked":false}"#;
    let bar_index = r#"{"name":"bar","vers":"1.0.0","deps":[{"name":"baz","req":"^1","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","features":{"extra":["dep:baz"]},"yanked":false}
{"name":"bar","vers":"1.1.0","deps":[],"cksum":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","features":{},"yanked":false}"#;
    let files = BTreeMap::from([
        ("bar".to_string(), bar_index.to_string()),
        ("baz".to_string(), BAZ_INDEX.to_string()),
        ("foo".to_string(), foo_index.to_string()),
    ]);

    let lockfile =
        Lockfile::from_str(&resolve_lockfile(metadata, &files, CRATES_IO_SOURCE).unwrap()).unwrap();

    assert_eq!(lockfile.packages.len(), 4);
    assert!(lockfile.packages.iter().any(|package| package.name.as_str() == "baz"));
}

#[test]
fn ignores_features_from_an_unselected_newer_candidate() {
    let metadata = r#"{
  "packages": [{
    "id": "path+file:///workspace#app@0.1.0",
    "name": "app",
    "version": "0.1.0",
    "dependencies": [
      {
        "name": "foo",
        "source": "registry+https://github.com/rust-lang/crates.io-index",
        "req": "^1.0"
      },
      {
        "name": "qux",
        "source": "registry+https://github.com/rust-lang/crates.io-index",
        "req": "=1.0.0"
      }
    ]
  }],
  "workspace_members": ["path+file:///workspace#app@0.1.0"]
}"#;
    let foo_index = r#"{"name":"foo","vers":"1.0.0","deps":[{"name":"bar","req":"^1","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null},{"name":"qux","req":"=1.0.0","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{},"yanked":false}
{"name":"foo","vers":"1.1.0","deps":[{"name":"bar","req":"^1","features":["extra"],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null},{"name":"qux","req":"=1.1.0","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","features":{},"yanked":false}"#;
    let bar_index = r#"{"name":"bar","vers":"1.0.0","deps":[{"name":"baz","req":"^1","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","features":{"extra":["dep:baz"]},"yanked":false}"#;
    let qux_index = r#"{"name":"qux","vers":"1.0.0","deps":[],"cksum":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","features":{},"yanked":false}
{"name":"qux","vers":"1.1.0","deps":[],"cksum":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","features":{},"yanked":false}"#;
    let files = BTreeMap::from([
        ("bar".to_string(), bar_index.to_string()),
        ("baz".to_string(), BAZ_INDEX.to_string()),
        ("foo".to_string(), foo_index.to_string()),
        ("qux".to_string(), qux_index.to_string()),
    ]);

    let lockfile =
        Lockfile::from_str(&resolve_lockfile(metadata, &files, CRATES_IO_SOURCE).unwrap()).unwrap();

    assert!(lockfile.packages.iter().any(|package| {
        package.name.as_str() == "foo" && package.version == semver::Version::new(1, 0, 0)
    }));
    assert!(!lockfile.packages.iter().any(|package| package.name.as_str() == "baz"));
}

#[test]
fn propagates_dependency_features_without_default_features() {
    let metadata = METADATA.replacen(
        r#""req": "^1.0""#,
        r#""req": "^1.0", "uses_default_features": false"#,
        1,
    );
    let foo_index = r#"{"name":"foo","vers":"1.0.0","deps":[{"name":"bar","req":"^1","features":["extra"],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{},"yanked":false}"#;
    let bar_index = r#"{"name":"bar","vers":"1.0.0","deps":[{"name":"baz","req":"^1","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","features":{"extra":["dep:baz"]},"yanked":false}"#;
    let files = BTreeMap::from([
        ("bar".to_string(), bar_index.to_string()),
        ("baz".to_string(), BAZ_INDEX.to_string()),
        ("foo".to_string(), foo_index.to_string()),
    ]);

    let lockfile =
        Lockfile::from_str(&resolve_lockfile(&metadata, &files, CRATES_IO_SOURCE).unwrap())
            .unwrap();

    assert!(lockfile.packages.iter().any(|package| package.name.as_str() == "baz"));
}

#[test]
fn backtracks_when_a_candidate_feature_conflicts_with_that_candidate() {
    let foo_index = r#"{"name":"foo","vers":"1.0.0","deps":[{"name":"bar","req":"^1","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{},"yanked":false}
{"name":"foo","vers":"1.1.0","deps":[{"name":"bar","req":"^1","features":["extra"],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null},{"name":"qux","req":"=1.0.0","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","features":{},"yanked":false}"#;
    let bar_index = r#"{"name":"bar","vers":"1.0.0","deps":[{"name":"qux","req":"=1.1.0","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","features":{"extra":["dep:qux"]},"yanked":false}"#;
    let qux_index = r#"{"name":"qux","vers":"1.0.0","deps":[],"cksum":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","features":{},"yanked":false}
{"name":"qux","vers":"1.1.0","deps":[],"cksum":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","features":{},"yanked":false}"#;
    let files = BTreeMap::from([
        ("bar".to_string(), bar_index.to_string()),
        ("foo".to_string(), foo_index.to_string()),
        ("qux".to_string(), qux_index.to_string()),
    ]);

    let lockfile =
        Lockfile::from_str(&resolve_lockfile(METADATA, &files, CRATES_IO_SOURCE).unwrap()).unwrap();

    assert!(lockfile.packages.iter().any(|package| {
        package.name.as_str() == "foo" && package.version == semver::Version::new(1, 0, 0)
    }));
    assert!(!lockfile.packages.iter().any(|package| package.name.as_str() == "qux"));
}

#[test]
fn dep_activation_suppresses_the_implicit_optional_feature() {
    let metadata =
        METADATA.replacen(r#""req": "^1.0""#, r#""req": "^1.0", "features": ["codec"]"#, 1);
    let foo_index = r#"{"name":"foo","vers":"1.0.0","deps":[{"name":"codec","req":"^1","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{"full":["dep:codec"]},"yanked":false}"#;
    let files = BTreeMap::from([("foo".to_string(), foo_index.to_string())]);

    assert!(missing_index_names(&metadata, &files, CRATES_IO_SOURCE).unwrap().is_empty());
    assert!(resolve_lockfile(&metadata, &files, CRATES_IO_SOURCE).is_err());
}

#[test]
fn selects_an_older_candidate_that_provides_a_requested_feature() {
    let metadata =
        METADATA.replacen(r#""req": "^1.0""#, r#""req": "^1.0", "features": ["special"]"#, 1);
    let foo_index = r#"{"name":"foo","vers":"1.0.0","deps":[],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{"special":[]},"yanked":false}
{"name":"foo","vers":"1.1.0","deps":[],"cksum":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","features":{},"yanked":false}"#;
    let files = BTreeMap::from([("foo".to_string(), foo_index.to_string())]);

    let lockfile =
        Lockfile::from_str(&resolve_lockfile(&metadata, &files, CRATES_IO_SOURCE).unwrap())
            .unwrap();

    assert!(lockfile.packages.iter().any(|package| {
        package.name.as_str() == "foo" && package.version == semver::Version::new(1, 0, 0)
    }));
}

#[test]
fn resolve_inputs_drops_everything_but_the_dependency_graph() {
    const FULL_METADATA: &str = r#"{
      "packages": [{
        "id": "path+file:///home/dev/secret-workspace#app@0.1.0",
        "name": "app",
        "version": "0.1.0",
        "license": "MIT",
        "manifest_path": "/home/dev/secret-workspace/Cargo.toml",
        "targets": [{"name": "app", "src_path": "/home/dev/secret-workspace/src/lib.rs"}],
        "dependencies": [{
          "name": "foo",
          "source": "registry+https://github.com/rust-lang/crates.io-index",
          "req": "^1.0",
          "path": "/home/dev/secret-workspace/vendor/foo"
        }]
      }],
      "workspace_root": "/home/dev/secret-workspace",
      "workspace_members": ["path+file:///home/dev/secret-workspace#app@0.1.0"]
    }"#;

    let reduced = resolve_inputs(FULL_METADATA).unwrap();

    assert!(!reduced.contains("secret-workspace"), "{reduced}");
    let index_files = BTreeMap::from([
        ("foo".to_string(), FOO_INDEX.to_string()),
        ("bar".to_string(), BAR_INDEX.to_string()),
    ]);
    assert_eq!(
        resolve_lockfile(&reduced, &index_files, CRATES_IO_SOURCE).unwrap(),
        resolve_lockfile(FULL_METADATA, &index_files, CRATES_IO_SOURCE).unwrap(),
    );
}

#[test]
fn resolve_inputs_keeps_the_features_a_dependency_requests() {
    const FEATURE_GATED_FOO_INDEX: &str = r#"{"name":"foo","vers":"1.0.0","deps":[{"name":"bar","req":"^2","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{"bar-support":["dep:bar"]},"yanked":false}"#;
    const METADATA: &str = r#"{
      "packages": [{
        "id": "path+file:///workspace#app@0.1.0",
        "name": "app",
        "version": "0.1.0",
        "dependencies": [{
          "name": "foo",
          "source": "registry+https://github.com/rust-lang/crates.io-index",
          "req": "^1.0",
          "features": ["bar-support"]
        }]
      }],
      "workspace_members": ["path+file:///workspace#app@0.1.0"]
    }"#;

    let reduced = resolve_inputs(METADATA).unwrap();

    let index_files = BTreeMap::from([
        ("foo".to_string(), FEATURE_GATED_FOO_INDEX.to_string()),
        ("bar".to_string(), BAR_INDEX.to_string()),
    ]);
    let lockfile =
        Lockfile::from_str(&resolve_lockfile(&reduced, &index_files, CRATES_IO_SOURCE).unwrap())
            .unwrap();
    assert!(
        lockfile.packages.iter().any(|package| package.name.as_str() == "bar"),
        "the feature that activates bar survived the reduction: {lockfile:?}",
    );
}

#[test]
fn accepts_a_dependency_that_names_the_registry_being_resolved_from() {
    const INDEX: &str = r#"{"name":"foo","vers":"1.0.0","deps":[{"name":"bar","req":"^2","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":"sparse+https://registry.example.test/index/"}],"cksum":"0000000000000000000000000000000000000000000000000000000000000000","features":{},"yanked":false}
"#;
    let files = BTreeMap::from([
        ("foo".to_string(), INDEX.to_string()),
        ("bar".to_string(), BAR_INDEX.to_string()),
    ]);

    let lockfile =
        resolve_lockfile(METADATA, &files, "sparse+https://registry.example.test/index/").unwrap();

    assert!(lockfile.contains(r#"name = "bar""#), "{lockfile}");
}

#[test]
fn rejects_a_dependency_from_a_third_party_registry() {
    const INDEX: &str = r#"{"name":"foo","vers":"1.0.0","deps":[{"name":"bar","req":"^2","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":"sparse+https://other.example.test/index/"}],"cksum":"0000000000000000000000000000000000000000000000000000000000000000","features":{},"yanked":false}
"#;
    let files = BTreeMap::from([
        ("foo".to_string(), INDEX.to_string()),
        ("bar".to_string(), BAR_INDEX.to_string()),
    ]);

    let error = resolve_lockfile(METADATA, &files, "sparse+https://registry.example.test/index/")
        .unwrap_err()
        .to_string();

    assert!(error.contains("other.example.test"), "{error}");
}
