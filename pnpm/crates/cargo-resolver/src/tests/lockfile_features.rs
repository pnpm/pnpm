use super::{BAR_INDEX, BAZ_INDEX, METADATA, OPTIONAL_FOO_INDEX};
use crate::{
    features::{DependencyOptions, dependencies_from_parts},
    missing_index_names,
    model::FeatureSelection,
    registry::{CRATES_IO_SOURCE, Registry},
    resolve_lockfile,
};
use cargo_lock::Lockfile;
use std::{collections::BTreeMap, str::FromStr};

const WEAK_FOO_INDEX: &str = r#"{"name":"foo","vers":"1.0.0","deps":[{"name":"bar","req":"^2","features":[],"optional":true,"default_features":false,"target":"cfg(windows)","kind":"normal","registry":null},{"name":"unused","req":"^1","features":[],"optional":true,"default_features":true,"target":null,"kind":"normal","registry":null}],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{"default":["bar?/extra"],"unused":["dep:unused"]},"yanked":false}"#;
const WEAK_BAR_INDEX: &str = r#"{"name":"bar","vers":"2.0.0","deps":[{"name":"baz","req":"^1","features":[],"optional":true,"default_features":false,"target":"cfg(unix)","kind":"normal","registry":null}],"cksum":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","features":{"extra":["baz?/leaf"]},"yanked":false}"#;

#[test]
fn discovers_and_locks_weak_dependencies_recursively_across_targets() {
    let mut files = BTreeMap::from([("foo".to_string(), WEAK_FOO_INDEX.to_string())]);
    assert_eq!(missing_index_names(METADATA, &files, CRATES_IO_SOURCE).unwrap(), ["bar"]);
    files.insert("bar".to_string(), WEAK_BAR_INDEX.to_string());
    assert_eq!(missing_index_names(METADATA, &files, CRATES_IO_SOURCE).unwrap(), ["baz"]);
    files.insert(
        "baz".to_string(),
        BAZ_INDEX.replace(r#""features":{}"#, r#""features":{"leaf":[]}"#),
    );
    assert_eq!(
        missing_index_names(METADATA, &files, CRATES_IO_SOURCE).unwrap(),
        Vec::<String>::new()
    );

    let encoded = resolve_lockfile(METADATA, &files, CRATES_IO_SOURCE).unwrap();
    eprintln!("LOCKFILE:\n{encoded}");
    let lockfile = Lockfile::from_str(&encoded).unwrap();
    let names = lockfile.packages
        .iter()
        .map(|package| package.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["app", "bar", "baz", "foo"]);
    let foo = lockfile.packages
        .iter()
        .find(|package| package.name.as_str() == "foo")
        .unwrap();
    assert_eq!(foo.dependencies[0].name.as_str(), "bar");
    let bar = lockfile.packages
        .iter()
        .find(|package| package.name.as_str() == "bar")
        .unwrap();
    assert_eq!(bar.dependencies[0].name.as_str(), "baz");
}

#[test]
fn weak_features_leave_unactivated_build_dependencies_inactive() {
    let files = BTreeMap::from([("foo".to_string(), WEAK_FOO_INDEX.to_string())]);
    let registry = Registry::new(&files, CRATES_IO_SOURCE).unwrap();
    let foo = &registry.package("foo").unwrap()[0];
    let dependencies = dependencies_from_parts(
        &foo.dependencies,
        &foo.features,
        &FeatureSelection { default_features: true, ..FeatureSelection::default() },
        DependencyOptions::default(),
    )
    .unwrap();
    assert_eq!(dependencies.len(), 0);
}

#[test]
fn unselected_weak_features_do_not_add_lock_entries() {
    let metadata =
        METADATA.replace(r#""req": "^1.0""#, r#""req": "^1.0", "uses_default_features": false"#);
    let files = BTreeMap::from([("foo".to_string(), WEAK_FOO_INDEX.to_string())]);
    assert_eq!(
        missing_index_names(&metadata, &files, CRATES_IO_SOURCE).unwrap(),
        Vec::<String>::new()
    );
    let encoded = resolve_lockfile(&metadata, &files, CRATES_IO_SOURCE).unwrap();
    eprintln!("LOCKFILE:\n{encoded}");
    let lockfile = Lockfile::from_str(&encoded).unwrap();
    assert_eq!(lockfile.packages.len(), 2);
}

#[test]
fn ordinary_optional_dependencies_are_not_locked_without_activation() {
    let files = BTreeMap::from([
        ("foo".to_string(), OPTIONAL_FOO_INDEX.to_string()),
        ("bar".to_string(), BAR_INDEX.to_string()),
    ]);
    let encoded = resolve_lockfile(METADATA, &files, CRATES_IO_SOURCE).unwrap();
    eprintln!("LOCKFILE:\n{encoded}");
    let lockfile = Lockfile::from_str(&encoded).unwrap();
    assert_eq!(lockfile.packages.len(), 2);
}
