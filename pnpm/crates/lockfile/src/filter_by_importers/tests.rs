use std::{collections::HashSet, path::Path};

use pretty_assertions::assert_eq;

use super::{FilterByImportersOptions, IncludedDependencies};
use crate::{Lockfile, PackageKey};

/// Two importers with disjoint dependency graphs, plus a dev-only and an
/// optional edge on the first, so a filter can be shown to keep exactly
/// what the selected importer and groups reach.
const LOCKFILE: &str = "lockfileVersion: '9.0'

importers:

  packages/app:
    dependencies:
      prod-dep:
        specifier: 1.0.0
        version: 1.0.0
    devDependencies:
      dev-dep:
        specifier: 1.0.0
        version: 1.0.0
    optionalDependencies:
      opt-dep:
        specifier: 1.0.0
        version: 1.0.0

  packages/other:
    dependencies:
      other-dep:
        specifier: 1.0.0
        version: 1.0.0

packages:

  deep@1.0.0:
    resolution: {integrity: sha512-deep}

  dev-dep@1.0.0:
    resolution: {integrity: sha512-dev}

  opt-dep@1.0.0:
    resolution: {integrity: sha512-opt}

  other-dep@1.0.0:
    resolution: {integrity: sha512-other}

  prod-dep@1.0.0:
    resolution: {integrity: sha512-prod}

snapshots:

  deep@1.0.0: {}

  dev-dep@1.0.0: {}

  opt-dep@1.0.0: {}

  other-dep@1.0.0: {}

  prod-dep@1.0.0:
    dependencies:
      deep: 1.0.0
";

fn lockfile() -> Lockfile {
    Lockfile::parse(LOCKFILE, Path::new("pnpm-lock.yaml"))
        .expect("parse lockfile")
        .expect("lockfile is not empty")
}

fn options(include: IncludedDependencies) -> FilterByImportersOptions {
    FilterByImportersOptions {
        include,
        skipped: HashSet::new(),
        fail_on_missing_dependencies: false,
    }
}

fn key(text: &str) -> PackageKey {
    text.parse().expect("parse package key")
}

fn has_alias(group: Option<&crate::ResolvedDependencyMap>, alias: &str) -> bool {
    group.is_some_and(|group| group.keys().any(|name| name.to_string() == alias))
}

fn snapshot_keys(lockfile: &Lockfile) -> Vec<String> {
    let mut keys: Vec<String> = lockfile
        .snapshots
        .as_ref()
        .map(|snapshots| snapshots.keys().map(ToString::to_string).collect())
        .unwrap_or_default();
    keys.sort();
    keys
}

#[test]
fn keeps_only_what_the_selected_importer_reaches() {
    let filtered = lockfile()
        .filter_by_importers(
            vec!["packages/app".to_string()],
            &options(IncludedDependencies::default()),
        )
        .expect("filter lockfile");

    assert_eq!(
        snapshot_keys(&filtered),
        vec!["deep@1.0.0", "dev-dep@1.0.0", "opt-dep@1.0.0", "prod-dep@1.0.0"],
    );
}

/// The `packages:` metadata map is pruned alongside `snapshots:`, so the
/// filtered lockfile carries no entry for a package it no longer resolves.
#[test]
fn prunes_the_metadata_map_too() {
    let filtered = lockfile()
        .filter_by_importers(
            vec!["packages/app".to_string()],
            &options(IncludedDependencies::default()),
        )
        .expect("filter lockfile");

    let packages = filtered.packages.as_ref().expect("packages survive");
    assert!(!packages.keys().any(|key| key.to_string() == "other-dep@1.0.0"));
    assert!(packages.keys().any(|key| key.to_string() == "prod-dep@1.0.0"));
}

#[test]
fn an_excluded_group_is_emptied_and_its_edges_are_not_walked() {
    let filtered = lockfile()
        .filter_by_importers(
            vec!["packages/app".to_string()],
            &options(IncludedDependencies {
                dependencies: true,
                dev_dependencies: false,
                optional_dependencies: false,
            }),
        )
        .expect("filter lockfile");

    assert_eq!(snapshot_keys(&filtered), vec!["deep@1.0.0", "prod-dep@1.0.0"]);
    let importer = &filtered.importers["packages/app"];
    assert!(importer.dev_dependencies.as_ref().expect("group present").is_empty());
    assert!(has_alias(importer.dependencies.as_ref(), "prod-dep"));
}

/// Importers the caller did not select keep their entries verbatim — the
/// filter narrows the package graph, not the workspace.
#[test]
fn unselected_importers_are_carried_through_untouched() {
    let filtered = lockfile()
        .filter_by_importers(
            vec!["packages/app".to_string()],
            &options(IncludedDependencies::default()),
        )
        .expect("filter lockfile");

    let other = &filtered.importers["packages/other"];
    assert!(has_alias(other.dependencies.as_ref(), "other-dep"));
}

/// A skipped key is never entered, so what only it reaches goes too.
#[test]
fn skipped_keys_and_what_only_they_reach_are_dropped() {
    let mut opts = options(IncludedDependencies::default());
    opts.skipped = HashSet::from([key("prod-dep@1.0.0")]);

    let filtered = lockfile()
        .filter_by_importers(vec!["packages/app".to_string()], &opts)
        .expect("filter lockfile");

    assert_eq!(snapshot_keys(&filtered), vec!["dev-dep@1.0.0", "opt-dep@1.0.0"]);
}

#[test]
fn a_missing_dependency_is_reported_only_when_asked_for() {
    let source = LOCKFILE.replace("  prod-dep@1.0.0:\n    dependencies:\n      deep: 1.0.0\n", "");
    let lockfile = Lockfile::parse(&source, Path::new("pnpm-lock.yaml"))
        .expect("parse lockfile")
        .expect("lockfile is not empty");

    let tolerated = lockfile.filter_by_importers(
        vec!["packages/app".to_string()],
        &options(IncludedDependencies::default()),
    );
    assert!(tolerated.is_ok());

    let mut strict = options(IncludedDependencies::default());
    strict.fail_on_missing_dependencies = true;
    let error = lockfile
        .filter_by_importers(vec!["packages/app".to_string()], &strict)
        .expect_err("the missing snapshot is reported");
    assert!(error.to_string().contains("prod-dep@1.0.0"), "{error}");
}
