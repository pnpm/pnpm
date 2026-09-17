use super::{
    git_dependency_sources, latest_version, missing_index_names, resolve_inputs, resolve_lockfile,
};
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

#[test]
fn git_dependency_sources_preserve_the_requested_revision() {
    let source = "git+https://example.test/repository?rev=release";
    let metadata = METADATA.replace(CRATES_IO_SOURCE, source);

    let sources = git_dependency_sources(&metadata).unwrap();

    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].to_string(), source);
}

#[test]
fn registry_and_path_dependencies_do_not_require_git_resolution() {
    for metadata in
        [METADATA.to_string(), METADATA.replace(&format!(r#""{CRATES_IO_SOURCE}""#), "null")]
    {
        let sources = git_dependency_sources(&metadata).unwrap();

        eprintln!("Registry and path dependencies must stay in the native resolver: {sources:?}");
        assert!(sources.is_empty());
    }
}

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
    assert!(
        lockfile.packages
            .iter()
            .any(|package| {
                package.name.as_str() == "foo" && package.version == semver::Version::new(1, 1, 0)
            }),
    );
    assert!(
        lockfile.packages
            .iter()
            .any(|package| {
                package.name.as_str() == "bar" && package.version == semver::Version::new(2, 0, 0)
            }),
    );
    assert!(
        lockfile.packages
            .iter()
            .any(|package| package.name.as_str() == "app" && package.source.is_none()),
    );
}

/// The workspace names no registry, so its crates belong to crates.io even
/// though a configured registry served the index. `cargo` records the source
/// a dependency named, not the replacement that served it.
#[test]
fn locks_a_dependency_naming_no_registry_against_crates_io() {
    let files = BTreeMap::from([
        ("foo".to_string(), FOO_INDEX.to_string()),
        ("bar".to_string(), BAR_INDEX.to_string()),
    ]);
    let lockfile =
        resolve_lockfile(METADATA, &files, "sparse+https://registry.example.test/index/").unwrap();

    eprintln!("LOCKFILE:\n{lockfile}");
    assert!(lockfile.contains(&format!(r#"source = "{CRATES_IO_SOURCE}""#)));
    assert!(!lockfile.contains("registry.example.test"));
}

/// A workspace dependency that names the configured registry keeps it, and
/// the crates it pulls in through entries naming no registry inherit it.
#[test]
fn locks_a_dependency_naming_the_configured_registry_against_it() {
    const REGISTRY: &str = "sparse+https://registry.example.test/index/";
    let metadata = METADATA.replace(
        r#""source": "registry+https://github.com/rust-lang/crates.io-index""#,
        &format!(r#""source": "{REGISTRY}""#),
    );
    let files = BTreeMap::from([
        ("foo".to_string(), FOO_INDEX.to_string()),
        ("bar".to_string(), BAR_INDEX.to_string()),
    ]);

    let lockfile = resolve_lockfile(&metadata, &files, REGISTRY).unwrap();

    eprintln!("LOCKFILE:\n{lockfile}");
    assert_eq!(
        lockfile
            .matches(&format!(r#"source = "{REGISTRY}""#))
            .count(),
        2,
    );
    assert!(!lockfile.contains(CRATES_IO_SOURCE));
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
    assert!(
        lockfile.packages
            .iter()
            .any(|package| package.name.as_str() == "baz"),
    );
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

    assert!(
        lockfile.packages
            .iter()
            .any(|package| {
                package.name.as_str() == "foo" && package.version == semver::Version::new(1, 0, 0)
            }),
    );
    assert!(
        !lockfile.packages
            .iter()
            .any(|package| package.name.as_str() == "baz"),
    );
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

    assert!(
        lockfile.packages
            .iter()
            .any(|package| package.name.as_str() == "baz"),
    );
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

    assert!(
        lockfile.packages
            .iter()
            .any(|package| {
                package.name.as_str() == "foo" && package.version == semver::Version::new(1, 0, 0)
            }),
    );
    assert!(
        !lockfile.packages
            .iter()
            .any(|package| package.name.as_str() == "qux"),
    );
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

    assert!(
        lockfile.packages
            .iter()
            .any(|package| {
                package.name.as_str() == "foo" && package.version == semver::Version::new(1, 0, 0)
            }),
    );
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
        lockfile.packages
            .iter()
            .any(|package| package.name.as_str() == "bar"),
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

/// A workspace asking for `foo` across two compatibility lines.
const SPANNING_METADATA: &str = r#"{
  "packages": [{
    "id": "path+file:///workspace#app@0.1.0",
    "name": "app",
    "version": "0.1.0",
    "dependencies": [{
      "name": "foo",
      "source": "registry+https://github.com/rust-lang/crates.io-index",
      "req": ">=1, <3"
    }]
  }],
  "workspace_members": ["path+file:///workspace#app@0.1.0"]
}"#;

/// `foo` 2.0.0 needs a `bar` that does not exist, so only `foo` 1.0.0 and
/// the `bar ^2` it needs can resolve.
const SPANNING_FOO_INDEX: &str = r#"{"name":"foo","vers":"1.0.0","deps":[{"name":"bar","req":"^2","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{},"yanked":false}
{"name":"foo","vers":"2.0.0","deps":[{"name":"bar","req":"^9","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","features":{},"yanked":false}"#;

/// `bar` 2.0.0 resolves, while the whole 3 compatibility line is yanked.
const PARTLY_YANKED_BAR_INDEX: &str = r#"{"name":"bar","vers":"2.0.0","deps":[],"cksum":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","features":{},"yanked":false}
{"name":"bar","vers":"3.0.0","deps":[],"cksum":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","features":{},"yanked":true}"#;

#[test]
fn resolves_past_a_prerelease_candidate_whose_dependency_is_yanked() {
    let foo_index = r#"{"name":"foo","vers":"1.0.0-beta.1","deps":[{"name":"bar","req":"^3","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{},"yanked":false}
{"name":"foo","vers":"1.1.0","deps":[{"name":"bar","req":"^2","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","features":{},"yanked":false}"#;
    let files = BTreeMap::from([
        ("bar".to_string(), PARTLY_YANKED_BAR_INDEX.to_string()),
        ("foo".to_string(), foo_index.to_string()),
    ]);

    let lockfile =
        Lockfile::from_str(&resolve_lockfile(METADATA, &files, CRATES_IO_SOURCE).unwrap()).unwrap();

    dbg!(&lockfile.packages);
    assert!(
        lockfile.packages
            .iter()
            .any(|package| {
                package.name.as_str() == "foo" && package.version == semver::Version::new(1, 1, 0)
            }),
    );
    assert!(
        lockfile.packages
            .iter()
            .any(|package| {
                package.name.as_str() == "bar" && package.version == semver::Version::new(2, 0, 0)
            }),
    );
}

#[test]
fn backtracks_to_a_candidate_whose_dependency_is_not_yanked() {
    let foo_index = r#"{"name":"foo","vers":"1.0.0","deps":[{"name":"bar","req":"^2","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{},"yanked":false}
{"name":"foo","vers":"1.1.0","deps":[{"name":"bar","req":"^3","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","features":{},"yanked":false}"#;
    let files = BTreeMap::from([
        ("bar".to_string(), PARTLY_YANKED_BAR_INDEX.to_string()),
        ("foo".to_string(), foo_index.to_string()),
    ]);

    let lockfile =
        Lockfile::from_str(&resolve_lockfile(METADATA, &files, CRATES_IO_SOURCE).unwrap()).unwrap();

    dbg!(&lockfile.packages);
    assert!(
        lockfile.packages
            .iter()
            .any(|package| {
                package.name.as_str() == "foo" && package.version == semver::Version::new(1, 0, 0)
            }),
    );
}

#[test]
fn backtracks_when_a_unified_feature_activates_a_yanked_dependency() {
    let metadata = METADATA.replacen(
        "]\n  }],",
        r#", {
      "name": "qux",
      "source": "registry+https://github.com/rust-lang/crates.io-index",
      "req": "^1.0"
    }]
  }],"#,
        1,
    );
    let foo_index = r#"{"name":"foo","vers":"1.0.0","deps":[{"name":"baz","req":"^1","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{"extra":["dep:baz"]},"yanked":false}
{"name":"foo","vers":"1.1.0","deps":[{"name":"bar","req":"^3","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","features":{"extra":["dep:bar"]},"yanked":false}"#;
    let qux_index = r#"{"name":"qux","vers":"1.0.0","deps":[{"name":"foo","req":"^1","features":["extra"],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","features":{},"yanked":false}"#;
    let files = BTreeMap::from([
        ("bar".to_string(), PARTLY_YANKED_BAR_INDEX.to_string()),
        ("baz".to_string(), BAZ_INDEX.to_string()),
        ("foo".to_string(), foo_index.to_string()),
        ("qux".to_string(), qux_index.to_string()),
    ]);

    let lockfile =
        Lockfile::from_str(&resolve_lockfile(&metadata, &files, CRATES_IO_SOURCE).unwrap())
            .unwrap();

    dbg!(&lockfile.packages);
    assert!(
        lockfile.packages
            .iter()
            .any(|package| {
                package.name.as_str() == "foo" && package.version == semver::Version::new(1, 0, 0)
            }),
    );
    assert!(
        lockfile.packages
            .iter()
            .any(|package| package.name.as_str() == "baz"),
    );
}

#[test]
fn reports_a_requirement_the_index_cannot_meet() {
    let foo_index = r#"{"name":"foo","vers":"1.0.0","deps":[],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{},"yanked":true}"#;
    let files = BTreeMap::from([("foo".to_string(), foo_index.to_string())]);

    let error = resolve_lockfile(METADATA, &files, CRATES_IO_SOURCE).unwrap_err().to_string();

    assert!(error.contains("foo ^1.0 (no version available)"), "{error}");
}

#[test]
fn reports_a_transitive_requirement_the_index_cannot_meet() {
    let foo_index = r#"{"name":"foo","vers":"1.0.0","deps":[{"name":"bar","req":"^3","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{},"yanked":false}"#;
    let files = BTreeMap::from([
        ("bar".to_string(), PARTLY_YANKED_BAR_INDEX.to_string()),
        ("foo".to_string(), foo_index.to_string()),
    ]);

    let error = resolve_lockfile(METADATA, &files, CRATES_IO_SOURCE).unwrap_err().to_string();

    assert!(error.contains("foo@1 1.0.0 depends on bar ^3 (no version available)"), "{error}");
}

/// `a` asks for the weak `c?/deep`, which does nothing on its own, and `b`
/// activates `c` without asking for `deep`. Only the union of the two edges
/// activates `d`.
#[test]
fn discovers_a_crate_only_unified_features_activate() {
    const METADATA: &str = r#"{
  "packages": [{
    "id": "path+file:///workspace#app@0.1.0",
    "name": "app",
    "version": "0.1.0",
    "dependencies": [
      {
        "name": "a",
        "source": "registry+https://github.com/rust-lang/crates.io-index",
        "req": "^1.0",
        "features": ["x"]
      },
      {
        "name": "b",
        "source": "registry+https://github.com/rust-lang/crates.io-index",
        "req": "^1.0"
      }
    ]
  }],
  "workspace_members": ["path+file:///workspace#app@0.1.0"]
}"#;
    let index = BTreeMap::from([
        (
            "a",
            r#"{"name":"a","vers":"1.0.0","deps":[{"name":"c","req":"^1","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{"x":["c?/deep"]},"yanked":false}"#,
        ),
        (
            "b",
            r#"{"name":"b","vers":"1.0.0","deps":[{"name":"a","req":"^1","features":["c"],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","features":{},"yanked":false}"#,
        ),
        (
            "c",
            r#"{"name":"c","vers":"1.0.0","deps":[{"name":"d","req":"^1","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","features":{"deep":["dep:d"]},"yanked":false}"#,
        ),
        (
            "d",
            r#"{"name":"d","vers":"1.0.0","deps":[],"cksum":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","features":{},"yanked":false}"#,
        ),
    ]);

    let mut files = BTreeMap::new();
    loop {
        let missing = missing_index_names(METADATA, &files, CRATES_IO_SOURCE).unwrap();
        if missing.is_empty() {
            break;
        }
        for name in missing {
            let contents = index[name.as_str()];
            files.insert(name, contents.to_string());
        }
    }

    dbg!(files.keys().collect::<Vec<_>>());
    assert!(files.contains_key("d"));
    let lockfile =
        Lockfile::from_str(&resolve_lockfile(METADATA, &files, CRATES_IO_SOURCE).unwrap()).unwrap();
    dbg!(&lockfile.packages);
    assert!(
        lockfile.packages
            .iter()
            .any(|package| package.name.as_str() == "d"),
    );
}

#[test]
fn fetches_what_every_admissible_line_needs() {
    // Each line needs a crate of its own, so walking only the newest would
    // leave the other unfetched.
    let foo_index = r#"{"name":"foo","vers":"1.0.0","deps":[{"name":"legacy","req":"^1","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{},"yanked":false}
{"name":"foo","vers":"2.0.0","deps":[{"name":"modern","req":"^1","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","features":{},"yanked":false}"#;
    let files = BTreeMap::from([("foo".to_string(), foo_index.to_string())]);

    assert_eq!(
        missing_index_names(SPANNING_METADATA, &files, CRATES_IO_SOURCE).unwrap(),
        ["legacy", "modern"],
    );
}

#[test]
fn backtracks_to_an_older_compatibility_line() {
    let files = BTreeMap::from([
        ("bar".to_string(), BAR_INDEX.to_string()),
        ("foo".to_string(), SPANNING_FOO_INDEX.to_string()),
    ]);

    let lockfile =
        Lockfile::from_str(&resolve_lockfile(SPANNING_METADATA, &files, CRATES_IO_SOURCE).unwrap())
            .unwrap();

    dbg!(&lockfile.packages);
    assert!(
        lockfile.packages
            .iter()
            .any(|package| {
                package.name.as_str() == "foo" && package.version == semver::Version::new(1, 0, 0)
            }),
    );
    assert!(
        lockfile.packages
            .iter()
            .any(|package| {
                package.name.as_str() == "bar" && package.version == semver::Version::new(2, 0, 0)
            }),
    );
}

#[test]
fn prefers_the_newest_compatibility_line_that_resolves() {
    let foo_index = SPANNING_FOO_INDEX.replace(r#""req":"^9""#, r#""req":"^2""#);
    let files = BTreeMap::from([
        ("bar".to_string(), BAR_INDEX.to_string()),
        ("foo".to_string(), foo_index),
    ]);

    let lockfile =
        Lockfile::from_str(&resolve_lockfile(SPANNING_METADATA, &files, CRATES_IO_SOURCE).unwrap())
            .unwrap();

    dbg!(&lockfile.packages);
    assert!(
        lockfile.packages
            .iter()
            .any(|package| {
                package.name.as_str() == "foo" && package.version == semver::Version::new(2, 0, 0)
            }),
    );
}

/// Both lines of `foo` carry an `extra` feature, so only the selection each
/// line was actually asked for decides whether `baz` is activated there.
#[test]
fn keeps_requested_features_on_the_line_that_asked_for_them() {
    const METADATA: &str = r#"{
  "packages": [
    {
      "id": "path+file:///workspace#wide@0.1.0",
      "name": "wide",
      "version": "0.1.0",
      "dependencies": [{
        "name": "foo",
        "source": "registry+https://github.com/rust-lang/crates.io-index",
        "req": ">=0.9",
        "features": ["extra"]
      }]
    },
    {
      "id": "path+file:///workspace#narrow@0.1.0",
      "name": "narrow",
      "version": "0.1.0",
      "dependencies": [{
        "name": "foo",
        "source": "registry+https://github.com/rust-lang/crates.io-index",
        "req": "^0.9"
      }]
    }
  ],
  "workspace_members": [
    "path+file:///workspace#wide@0.1.0",
    "path+file:///workspace#narrow@0.1.0"
  ]
}"#;
    let foo_index = r#"{"name":"foo","vers":"0.9.0","deps":[{"name":"baz","req":"^1","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{"extra":["dep:baz"]},"yanked":false}
{"name":"foo","vers":"1.0.0","deps":[{"name":"baz","req":"^1","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","features":{"extra":["dep:baz"]},"yanked":false}"#;
    let files = BTreeMap::from([
        ("baz".to_string(), BAZ_INDEX.to_string()),
        ("foo".to_string(), foo_index.to_string()),
    ]);

    let lockfile =
        Lockfile::from_str(&resolve_lockfile(METADATA, &files, CRATES_IO_SOURCE).unwrap()).unwrap();

    dbg!(&lockfile.packages);
    let dependencies = |version: semver::Version| {
        lockfile.packages
            .iter()
            .find(|package| package.name.as_str() == "foo" && package.version == version)
            .map(|package| {
                package.dependencies
                    .iter()
                    .map(|dependency| dependency.name.to_string())
                    .collect::<Vec<_>>()
            })
    };
    assert_eq!(dependencies(semver::Version::new(1, 0, 0)), Some(vec!["baz".to_string()]));
    assert_eq!(dependencies(semver::Version::new(0, 9, 0)), Some(Vec::new()));
}

/// The `rand` example from the Cargo book: a requirement spanning two lines
/// and one pinned to the older line do not unify, and `cargo` builds both.
#[test]
fn keeps_two_compatibility_lines_of_one_crate_apart() {
    const METADATA: &str = r#"{
  "packages": [
    {
      "id": "path+file:///workspace#wide@0.1.0",
      "name": "wide",
      "version": "0.1.0",
      "dependencies": [{
        "name": "rand",
        "source": "registry+https://github.com/rust-lang/crates.io-index",
        "req": ">=0.6, <0.8.0"
      }]
    },
    {
      "id": "path+file:///workspace#narrow@0.1.0",
      "name": "narrow",
      "version": "0.1.0",
      "dependencies": [{
        "name": "rand",
        "source": "registry+https://github.com/rust-lang/crates.io-index",
        "req": "^0.6"
      }]
    }
  ],
  "workspace_members": [
    "path+file:///workspace#wide@0.1.0",
    "path+file:///workspace#narrow@0.1.0"
  ]
}"#;
    let rand_index = r#"{"name":"rand","vers":"0.6.5","deps":[],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{},"yanked":false}
{"name":"rand","vers":"0.7.3","deps":[],"cksum":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","features":{},"yanked":false}"#;
    let files = BTreeMap::from([("rand".to_string(), rand_index.to_string())]);

    let lockfile =
        Lockfile::from_str(&resolve_lockfile(METADATA, &files, CRATES_IO_SOURCE).unwrap()).unwrap();

    dbg!(&lockfile.packages);
    let selected = lockfile.packages
        .iter()
        .filter(|package| package.name.as_str() == "rand")
        .map(|package| package.version.to_string())
        .collect::<Vec<_>>();
    assert_eq!(selected, ["0.6.5", "0.7.3"]);
}

/// Only the older line carries `extra`, so the newer one is not a choice
/// this requirement can take, and `baz` comes with the line that is.
#[test]
fn selects_the_older_line_when_the_newer_lacks_a_requested_feature() {
    const METADATA: &str = r#"{
  "packages": [{
    "id": "path+file:///workspace#app@0.1.0",
    "name": "app",
    "version": "0.1.0",
    "dependencies": [{
      "name": "foo",
      "source": "registry+https://github.com/rust-lang/crates.io-index",
      "req": ">=0.9",
      "features": ["extra"]
    }]
  }],
  "workspace_members": ["path+file:///workspace#app@0.1.0"]
}"#;
    let foo_index = r#"{"name":"foo","vers":"0.9.0","deps":[{"name":"baz","req":"^1","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{"extra":["dep:baz"]},"yanked":false}
{"name":"foo","vers":"1.0.0","deps":[],"cksum":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","features":{},"yanked":false}"#;
    let files = BTreeMap::from([
        ("baz".to_string(), BAZ_INDEX.to_string()),
        ("foo".to_string(), foo_index.to_string()),
    ]);

    let lockfile =
        Lockfile::from_str(&resolve_lockfile(METADATA, &files, CRATES_IO_SOURCE).unwrap()).unwrap();

    dbg!(&lockfile.packages);
    assert!(
        lockfile.packages
            .iter()
            .any(|package| {
                package.name.as_str() == "foo" && package.version == semver::Version::new(0, 9, 0)
            }),
    );
    assert!(
        lockfile.packages
            .iter()
            .any(|package| package.name.as_str() == "baz"),
    );
}

/// No line supports both features, so one shared choice would rule both
/// lines out.
#[test]
fn lets_requirements_asking_for_different_features_take_different_lines() {
    const METADATA: &str = r#"{
  "packages": [
    {
      "id": "path+file:///workspace#reads@0.1.0",
      "name": "reads",
      "version": "0.1.0",
      "dependencies": [{
        "name": "foo",
        "source": "registry+https://github.com/rust-lang/crates.io-index",
        "req": ">=1, <3",
        "features": ["read"]
      }]
    },
    {
      "id": "path+file:///workspace#writes@0.1.0",
      "name": "writes",
      "version": "0.1.0",
      "dependencies": [{
        "name": "foo",
        "source": "registry+https://github.com/rust-lang/crates.io-index",
        "req": ">=1, <3",
        "features": ["write"]
      }]
    }
  ],
  "workspace_members": [
    "path+file:///workspace#reads@0.1.0",
    "path+file:///workspace#writes@0.1.0"
  ]
}"#;
    let foo_index = r#"{"name":"foo","vers":"1.0.0","deps":[],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{"read":[]},"yanked":false}
{"name":"foo","vers":"2.0.0","deps":[],"cksum":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","features":{"write":[]},"yanked":false}"#;
    let files = BTreeMap::from([("foo".to_string(), foo_index.to_string())]);

    let lockfile =
        Lockfile::from_str(&resolve_lockfile(METADATA, &files, CRATES_IO_SOURCE).unwrap()).unwrap();

    dbg!(&lockfile.packages);
    let selected = lockfile.packages
        .iter()
        .filter(|package| package.name.as_str() == "foo")
        .map(|package| package.version.to_string())
        .collect::<Vec<_>>();
    assert_eq!(selected, ["1.0.0", "2.0.0"]);
}

/// A line walked only under the features of every requirement reaching it
/// would reach neither crate.
#[test]
fn fetches_what_each_requirement_activates_on_its_own_line() {
    const METADATA: &str = r#"{
  "packages": [
    {
      "id": "path+file:///workspace#reads@0.1.0",
      "name": "reads",
      "version": "0.1.0",
      "dependencies": [{
        "name": "foo",
        "source": "registry+https://github.com/rust-lang/crates.io-index",
        "req": ">=1, <3",
        "features": ["read"]
      }]
    },
    {
      "id": "path+file:///workspace#writes@0.1.0",
      "name": "writes",
      "version": "0.1.0",
      "dependencies": [{
        "name": "foo",
        "source": "registry+https://github.com/rust-lang/crates.io-index",
        "req": ">=1, <3",
        "features": ["write"]
      }]
    }
  ],
  "workspace_members": [
    "path+file:///workspace#reads@0.1.0",
    "path+file:///workspace#writes@0.1.0"
  ]
}"#;
    let foo_index = r#"{"name":"foo","vers":"1.0.0","deps":[{"name":"reader","req":"^1","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{"read":["dep:reader"]},"yanked":false}
{"name":"foo","vers":"2.0.0","deps":[{"name":"writer","req":"^1","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","features":{"write":["dep:writer"]},"yanked":false}"#;
    let files = BTreeMap::from([("foo".to_string(), foo_index.to_string())]);

    assert_eq!(
        missing_index_names(METADATA, &files, CRATES_IO_SOURCE).unwrap(),
        ["reader", "writer"],
    );
}

mod lockfile_features;
