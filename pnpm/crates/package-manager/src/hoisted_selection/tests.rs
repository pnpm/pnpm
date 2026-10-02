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
