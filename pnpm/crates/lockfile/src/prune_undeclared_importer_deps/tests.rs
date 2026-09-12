use crate::{ProjectSnapshot, prune_undeclared_importer_deps};
use pnpm_package_manifest::PackageManifest;
use serde_json::json;
use std::path::PathBuf;

fn manifest(value: serde_json::Value) -> PackageManifest {
    PackageManifest::from_value(PathBuf::from("/project/package.json"), value)
}

fn importer(source: &str) -> ProjectSnapshot {
    serde_saphyr::from_str(source).expect("parse importer")
}

const FOO_AND_BAR: &str = r"
specifiers:
  foo: ^1.0.0
  bar: ^2.0.0
dependencies:
  foo:
    specifier: ^1.0.0
    version: 1.1.0
  bar:
    specifier: ^2.0.0
    version: 2.0.0
";

#[test]
fn drops_a_dependency_the_manifest_no_longer_declares() {
    let mut importer = importer(FOO_AND_BAR);
    prune_undeclared_importer_deps(
        &mut importer,
        None,
        &manifest(json!({ "dependencies": { "foo": "^1.0.0" } })),
        true,
    );
    let dependencies = importer.dependencies.as_ref().expect("dependencies");
    assert_eq!(dependencies.len(), 1);
    assert!(dependencies.contains_key(&"foo".parse().expect("package name")));
    let specifiers = importer.specifiers.as_ref().expect("specifiers");
    assert_eq!(specifiers.keys().collect::<Vec<_>>(), vec!["foo"]);
}

#[test]
fn keeps_every_declared_dependency() {
    let mut importer = importer(FOO_AND_BAR);
    prune_undeclared_importer_deps(
        &mut importer,
        None,
        &manifest(json!({ "dependencies": { "foo": "^1.0.0", "bar": "^2.0.0" } })),
        true,
    );
    assert_eq!(importer.dependencies.as_ref().expect("dependencies").len(), 2);
}

#[test]
fn drops_the_group_a_dependency_moved_out_of() {
    let mut importer = importer(
        r"
specifiers:
  foo: ^1.0.0
dependencies:
  foo:
    specifier: ^1.0.0
    version: 1.1.0
devDependencies:
  foo:
    specifier: ^1.0.0
    version: 1.1.0
",
    );
    prune_undeclared_importer_deps(
        &mut importer,
        None,
        &manifest(json!({ "dependencies": { "foo": "^1.0.0" } })),
        true,
    );
    assert!(importer.dev_dependencies.is_none());
    assert_eq!(importer.dependencies.as_ref().expect("dependencies").len(), 1);
}

#[test]
fn keeps_an_auto_installed_peer() {
    let mut importer = importer(FOO_AND_BAR);
    prune_undeclared_importer_deps(
        &mut importer,
        None,
        &manifest(json!({
            "dependencies": { "foo": "^1.0.0" },
            "peerDependencies": { "bar": "^2.0.0" },
        })),
        true,
    );
    assert_eq!(importer.dependencies.as_ref().expect("dependencies").len(), 2);
}

#[test]
fn drops_a_peer_that_is_not_auto_installed() {
    let mut importer = importer(FOO_AND_BAR);
    prune_undeclared_importer_deps(
        &mut importer,
        None,
        &manifest(json!({
            "dependencies": { "foo": "^1.0.0" },
            "peerDependencies": { "bar": "^2.0.0" },
        })),
        false,
    );
    assert_eq!(importer.dependencies.as_ref().expect("dependencies").len(), 1);
}

/// Folding every peer into `dependencies` would strip this one from
/// `devDependencies` and leave the freshness check reading a field the
/// manifest disagrees with.
#[test]
fn keeps_a_dev_dependency_that_is_also_declared_as_a_peer() {
    let mut importer = importer(
        r"
specifiers:
  foo: ^1.0.0
devDependencies:
  foo:
    specifier: ^1.0.0
    version: 1.1.0
",
    );
    prune_undeclared_importer_deps(
        &mut importer,
        None,
        &manifest(json!({
            "devDependencies": { "foo": "^1.0.0" },
            "peerDependencies": { "foo": "^1.0.0" },
        })),
        true,
    );
    assert_eq!(importer.dev_dependencies.as_ref().expect("devDependencies").len(), 1);
    assert!(importer.dependencies.is_none());
}

#[test]
fn keeps_an_optional_dependency_under_its_own_group() {
    let mut importer = importer(
        r"
specifiers:
  foo: ^1.0.0
optionalDependencies:
  foo:
    specifier: ^1.0.0
    version: 1.1.0
",
    );
    prune_undeclared_importer_deps(
        &mut importer,
        None,
        &manifest(json!({
            "dependencies": { "foo": "^1.0.0" },
            "optionalDependencies": { "foo": "^1.0.0" },
        })),
        true,
    );
    assert_eq!(importer.optional_dependencies.as_ref().expect("optionalDependencies").len(), 1);
}

#[test]
fn empties_an_importer_the_manifest_declares_nothing_for() {
    let mut importer = importer(FOO_AND_BAR);
    prune_undeclared_importer_deps(&mut importer, None, &manifest(json!({})), true);
    assert!(importer.dependencies.is_none());
    assert!(importer.specifiers.as_ref().expect("specifiers").is_empty());
}

/// An undeclared entry the read file already carried is not the fold's to
/// take back out; leaving it in place keeps the freshness check able to
/// report it.
#[test]
fn keeps_an_undeclared_entry_the_fold_did_not_introduce() {
    let pre_merge = importer(FOO_AND_BAR);
    let mut importer = importer(FOO_AND_BAR);
    prune_undeclared_importer_deps(
        &mut importer,
        Some(&pre_merge),
        &manifest(json!({ "dependencies": { "foo": "^1.0.0" } })),
        true,
    );
    assert_eq!(importer.dependencies.as_ref().expect("dependencies").len(), 2);
}
