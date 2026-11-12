use crate::SkippedSnapshots;
use pnpm_lockfile::{Lockfile, PackageKey};
use pnpm_modules_yaml::IncludedDependencies;
use std::collections::HashSet;

fn lockfile(yaml: &str) -> Lockfile {
    serde_saphyr::from_str(yaml).expect("parse lockfile")
}

fn included(optional_dependencies: bool) -> IncludedDependencies {
    IncludedDependencies { dependencies: true, dev_dependencies: false, optional_dependencies }
}

fn dev_only_included() -> IncludedDependencies {
    IncludedDependencies {
        dependencies: false,
        dev_dependencies: true,
        optional_dependencies: true,
    }
}

#[test]
fn finds_runtime_dependencies_in_selected_snapshots() {
    let lockfile = lockfile(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      library:\n        specifier: 1.0.0\n        version: 1.0.0\npackages:\n  library@1.0.0:\n    resolution: {integrity: sha512-aaa}\n  node@runtime:22.0.0:\n    resolution: {integrity: sha512-bbb}\nsnapshots:\n  library@1.0.0:\n    dependencies:\n      node: runtime:22.0.0\n  node@runtime:22.0.0: {}\n",
    );

    assert_eq!(
        crate::runtime_dependency_alias(&lockfile, included(false), &SkippedSnapshots::new()),
        Some("node".to_string()),
    );
}

#[test]
fn ignores_optional_runtime_dependencies_when_optionals_are_excluded() {
    let lockfile = lockfile(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      library:\n        specifier: 1.0.0\n        version: 1.0.0\npackages:\n  library@1.0.0:\n    resolution: {integrity: sha512-aaa}\n  node@runtime:22.0.0:\n    resolution: {integrity: sha512-bbb}\nsnapshots:\n  library@1.0.0:\n    optionalDependencies:\n      node: runtime:22.0.0\n  node@runtime:22.0.0: {}\n",
    );

    assert_eq!(
        crate::runtime_dependency_alias(&lockfile, included(false), &SkippedSnapshots::new()),
        None,
    );
}

#[test]
fn ignores_project_optional_runtime_dependencies_in_dev_only_selection() {
    let lockfile = lockfile(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    optionalDependencies:\n      node:\n        specifier: runtime:22.0.0\n        version: runtime:22.0.0\npackages:\n  node@runtime:22.0.0:\n    resolution: {integrity: sha512-bbb}\nsnapshots:\n  node@runtime:22.0.0: {}\n",
    );

    assert_eq!(
        crate::runtime_dependency_alias(&lockfile, dev_only_included(), &SkippedSnapshots::new()),
        None,
    );
}

#[test]
fn finds_optional_runtime_dependencies_when_optionals_are_included() {
    let lockfile = lockfile(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      library:\n        specifier: 1.0.0\n        version: 1.0.0\npackages:\n  library@1.0.0:\n    resolution: {integrity: sha512-aaa}\n  node@runtime:22.0.0:\n    resolution: {integrity: sha512-bbb}\nsnapshots:\n  library@1.0.0:\n    optionalDependencies:\n      node: runtime:22.0.0\n  node@runtime:22.0.0: {}\n",
    );

    assert_eq!(
        crate::runtime_dependency_alias(&lockfile, included(true), &SkippedSnapshots::new()),
        Some("node".to_string()),
    );
}

#[test]
fn ignores_runtime_dependencies_of_installability_skipped_snapshots() {
    let lockfile = lockfile(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      library:\n        specifier: 1.0.0\n        version: 1.0.0\npackages:\n  library@1.0.0:\n    resolution: {integrity: sha512-aaa}\n  node@runtime:22.0.0:\n    resolution: {integrity: sha512-bbb}\nsnapshots:\n  library@1.0.0:\n    dependencies:\n      node: runtime:22.0.0\n  node@runtime:22.0.0: {}\n",
    );
    let skipped = SkippedSnapshots::from_set(HashSet::from(["library@1.0.0"
        .parse::<PackageKey>()
        .expect("parse package key")]));

    assert_eq!(crate::runtime_dependency_alias(&lockfile, included(false), &skipped), None);
}

#[test]
fn ignores_direct_runtime_dependencies_skipped_for_installability() {
    let lockfile = lockfile(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      node:\n        specifier: runtime:22.0.0\n        version: runtime:22.0.0\npackages:\n  node@runtime:22.0.0:\n    resolution: {integrity: sha512-bbb}\nsnapshots:\n  node@runtime:22.0.0: {}\n",
    );
    let skipped = SkippedSnapshots::from_set(HashSet::from(["node@runtime:22.0.0"
        .parse::<PackageKey>()
        .expect("parse package key")]));

    assert_eq!(crate::runtime_dependency_alias(&lockfile, included(false), &skipped), None);
}

#[test]
fn reports_transitive_runtimes_left_by_direct_runtime_skips() {
    let lockfile = lockfile(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      library:\n        specifier: 1.0.0\n        version: 1.0.0\n      node:\n        specifier: runtime:22.0.0\n        version: runtime:22.0.0\npackages:\n  library@1.0.0:\n    resolution: {integrity: sha512-aaa}\n  node@runtime:22.0.0:\n    resolution: {integrity: sha512-bbb}\n  deno@runtime:2.0.0:\n    resolution: {integrity: sha512-ccc}\nsnapshots:\n  library@1.0.0:\n    dependencies:\n      deno: runtime:2.0.0\n  node@runtime:22.0.0: {}\n  deno@runtime:2.0.0: {}\n",
    );
    let mut skipped = SkippedSnapshots::new();
    skipped.add_optional_excluded("node@runtime:22.0.0".parse().expect("parse package key"));

    assert_eq!(
        crate::runtime_dependency_alias(&lockfile, included(false), &skipped),
        Some("deno".to_string()),
    );
}
