use super::selected_materialization_ids;
use pnpm_lockfile::Lockfile;
use std::{collections::HashSet, fmt::Write};

const ONE_DEPENDENCY: &str =
    "\n    dependencies:\n      foo:\n        specifier: 1.0.0\n        version: 1.0.0";

/// A lockfile whose importers each record one dependency, except the ones in
/// `empty`.
fn lockfile(importer_ids: &[&str], empty: &[&str]) -> Lockfile {
    let mut yaml = String::from("lockfileVersion: '9.0'\nimporters:\n");
    for importer_id in importer_ids {
        let snapshot = if empty.contains(importer_id) { " {}" } else { ONE_DEPENDENCY };
        writeln!(yaml, "  {importer_id}:{snapshot}").expect("write to a String");
    }
    serde_saphyr::from_str(&yaml).expect("parse lockfile")
}

fn ids(importer_ids: &[&str]) -> HashSet<String> {
    importer_ids
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn a_layout_without_a_hoisted_prior_materializes_the_selection() {
    let wanted = lockfile(&[".", "packages/a", "packages/b"], &[]);
    assert_eq!(
        selected_materialization_ids(&wanted, &ids(&["packages/a"]), None),
        ids(&["packages/a"]),
    );
}

#[test]
fn a_hoisted_install_keeps_the_importers_the_previous_install_materialized() {
    let wanted = lockfile(&[".", "packages/a", "packages/b", "packages/c"], &[]);
    let prior = lockfile(&[".", "packages/b"], &[]);
    assert_eq!(
        selected_materialization_ids(&wanted, &ids(&["packages/a"]), Some(&prior)),
        ids(&[".", "packages/a", "packages/b"]),
    );
}

#[test]
fn a_hoisted_install_leaves_out_prior_importers_without_dependencies() {
    let wanted = lockfile(&[".", "packages/a", "packages/b"], &[]);
    let prior = lockfile(&[".", "packages/b"], &["packages/b"]);
    assert_eq!(
        selected_materialization_ids(&wanted, &ids(&["packages/a"]), Some(&prior)),
        ids(&[".", "packages/a"]),
    );
}

#[test]
fn a_hoisted_install_drops_prior_importers_the_wanted_lockfile_no_longer_has() {
    let wanted = lockfile(&[".", "packages/a"], &[]);
    let prior = lockfile(&[".", "packages/a", "packages/removed"], &[]);
    assert_eq!(
        selected_materialization_ids(&wanted, &ids(&["packages/a"]), Some(&prior)),
        ids(&[".", "packages/a"]),
    );
}

#[test]
fn a_filtered_hoisted_prod_install_strips_dev_from_selected_importers_and_keeps_prior_groups_on_unselected()
 {
    let wanted: Lockfile = serde_saphyr::from_str(
        r"
lockfileVersion: '9.0'
importers:
  .:
    devDependencies:
      root-dev:
        specifier: 1.0.0
        version: 1.0.0
  packages/a:
    dependencies:
      a-prod:
        specifier: 1.0.0
        version: 1.0.0
    devDependencies:
      a-dev:
        specifier: 1.0.0
        version: 1.0.0
  packages/b:
    dependencies:
      b-prod:
        specifier: 1.0.0
        version: 1.0.0
    devDependencies:
      b-dev:
        specifier: 1.0.0
        version: 1.0.0
snapshots:
  root-dev@1.0.0: {}
  a-prod@1.0.0: {}
  a-dev@1.0.0: {}
  b-prod@1.0.0: {}
  b-dev@1.0.0: {}
",
    )
    .expect("parse wanted lockfile");
    let prior = wanted.clone();
    let closure = super::selected_materialization_closure(
        &wanted,
        std::path::Path::new("/ws"),
        &ids(&["packages/a"]),
        Some(&prior),
        &crate::GroupSelection::following_every_edge(pnpm_modules_yaml::IncludedDependencies {
            dependencies: true,
            dev_dependencies: false,
            optional_dependencies: true,
        }),
        &crate::SkippedSnapshots::new(),
    );
    let pkg_a = closure.lockfile.importers.get("packages/a").expect("packages/a");
    assert!(pkg_a.dependencies.is_some(), "selected importer keeps prod dependencies");
    assert!(
        pkg_a.dev_dependencies.is_none(),
        "selected importer drops devDependencies under --prod",
    );

    let pkg_b = closure.lockfile.importers.get("packages/b").expect("packages/b");
    assert!(pkg_b.dependencies.is_some(), "unselected prior importer keeps prod dependencies");
    assert!(
        pkg_b.dev_dependencies.is_some(),
        "unselected prior importer keeps prior devDependencies",
    );

    let root = closure.lockfile.importers.get(".").expect("root importer");
    assert!(
        root.dev_dependencies.is_some(),
        "unselected root importer keeps prior devDependencies",
    );

    let snapshots = closure.lockfile.snapshots.as_ref().expect("snapshots");
    assert!(snapshots.contains_key(&"a-prod@1.0.0".parse().unwrap()));
    assert!(!snapshots.contains_key(&"a-dev@1.0.0".parse().unwrap()));
    assert!(snapshots.contains_key(&"b-prod@1.0.0".parse().unwrap()));
    assert!(snapshots.contains_key(&"b-dev@1.0.0".parse().unwrap()));
    assert!(snapshots.contains_key(&"root-dev@1.0.0".parse().unwrap()));
}

#[test]
fn a_filtered_hoisted_dev_install_strips_prod_from_selected_importers_and_keeps_prior_groups_on_unselected()
 {
    let wanted: Lockfile = serde_saphyr::from_str(
        r"
lockfileVersion: '9.0'
importers:
  packages/a:
    dependencies:
      a-prod:
        specifier: 1.0.0
        version: 1.0.0
    devDependencies:
      a-dev:
        specifier: 1.0.0
        version: 1.0.0
  packages/b:
    dependencies:
      b-prod:
        specifier: 1.0.0
        version: 1.0.0
    devDependencies:
      b-dev:
        specifier: 1.0.0
        version: 1.0.0
snapshots:
  a-prod@1.0.0: {}
  a-dev@1.0.0: {}
  b-prod@1.0.0: {}
  b-dev@1.0.0: {}
",
    )
    .expect("parse wanted lockfile");
    let prior = wanted.clone();
    let closure = super::selected_materialization_closure(
        &wanted,
        std::path::Path::new("/ws"),
        &ids(&["packages/a"]),
        Some(&prior),
        &crate::GroupSelection::following_every_edge(pnpm_modules_yaml::IncludedDependencies {
            dependencies: false,
            dev_dependencies: true,
            optional_dependencies: true,
        }),
        &crate::SkippedSnapshots::new(),
    );
    let pkg_a = closure.lockfile.importers.get("packages/a").expect("packages/a");
    assert!(pkg_a.dependencies.is_none(), "selected importer drops dependencies under --dev");
    assert!(pkg_a.dev_dependencies.is_some(), "selected importer keeps devDependencies");

    let pkg_b = closure.lockfile.importers.get("packages/b").expect("packages/b");
    assert!(pkg_b.dependencies.is_some(), "unselected prior importer keeps dependencies");
    assert!(pkg_b.dev_dependencies.is_some(), "unselected prior importer keeps devDependencies");

    let snapshots = closure.lockfile.snapshots.as_ref().expect("snapshots");
    assert!(!snapshots.contains_key(&"a-prod@1.0.0".parse().unwrap()));
    assert!(snapshots.contains_key(&"a-dev@1.0.0".parse().unwrap()));
    assert!(snapshots.contains_key(&"b-prod@1.0.0".parse().unwrap()));
    assert!(snapshots.contains_key(&"b-dev@1.0.0".parse().unwrap()));
}

#[test]
fn a_filtered_hoisted_no_optional_install_strips_optional_from_selected_and_preserves_optional_flag_on_unselected()
 {
    let wanted: Lockfile = serde_saphyr::from_str(
        r"
lockfileVersion: '9.0'
importers:
  packages/a:
    dependencies:
      a-prod:
        specifier: 1.0.0
        version: 1.0.0
    optionalDependencies:
      a-opt:
        specifier: 1.0.0
        version: 1.0.0
  packages/b:
    dependencies:
      b-prod:
        specifier: 1.0.0
        version: 1.0.0
    optionalDependencies:
      b-opt:
        specifier: 1.0.0
        version: 1.0.0
snapshots:
  a-prod@1.0.0: {}
  a-opt@1.0.0:
    optional: true
  b-prod@1.0.0: {}
  b-opt@1.0.0:
    optional: true
",
    )
    .expect("parse wanted lockfile");
    let prior = wanted.clone();
    let closure = super::selected_materialization_closure(
        &wanted,
        std::path::Path::new("/ws"),
        &ids(&["packages/a"]),
        Some(&prior),
        &crate::GroupSelection::following_every_edge(pnpm_modules_yaml::IncludedDependencies {
            dependencies: true,
            dev_dependencies: true,
            optional_dependencies: false,
        }),
        &crate::SkippedSnapshots::new(),
    );
    let pkg_a = closure.lockfile.importers.get("packages/a").expect("packages/a");
    assert!(
        pkg_a.optional_dependencies.is_none(),
        "selected importer drops optionalDependencies under --no-optional",
    );

    let pkg_b = closure.lockfile.importers.get("packages/b").expect("packages/b");
    assert!(
        pkg_b.optional_dependencies.is_some(),
        "unselected prior importer keeps optionalDependencies",
    );

    let snapshots = closure.lockfile.snapshots.as_ref().expect("snapshots");
    assert!(!snapshots.contains_key(&"a-opt@1.0.0".parse().unwrap()));
    let retained_opt = snapshots
        .get(&"b-opt@1.0.0".parse().unwrap())
        .expect("retained optional snapshot");
    assert!(retained_opt.optional, "retained optional snapshot keeps optional: true");
}
