use std::collections::HashMap;

use pacquet_lockfile::{
    ImporterDepVersion, PkgName, PkgVerPeer, ProjectSnapshot, ResolvedDependencySpec,
};

use super::reusable_importer_dep;

fn single_dep_importer(alias: &str, resolved: &str) -> HashMap<String, ProjectSnapshot> {
    let mut deps = HashMap::new();
    deps.insert(
        alias.parse::<PkgName>().expect("parse alias"),
        ResolvedDependencySpec {
            specifier: resolved.to_string(),
            version: ImporterDepVersion::Regular(
                resolved.parse::<PkgVerPeer>().expect("parse version"),
            ),
        },
    );
    HashMap::from([(
        ".".to_string(),
        ProjectSnapshot { dependencies: Some(deps), ..ProjectSnapshot::default() },
    )])
}

#[test]
fn reuses_when_locked_version_satisfies_the_manifest_range() {
    let importers = single_dep_importer("react", "18.2.0");
    let key = reusable_importer_dep(&importers, ".", "react", "^18.0.0")
        .expect("locked 18.2.0 satisfies ^18.0.0");
    assert_eq!(key.to_string(), "react@18.2.0");
}

#[test]
fn reuses_across_a_widened_but_still_satisfied_range() {
    let importers = single_dep_importer("react", "18.2.0");
    assert!(reusable_importer_dep(&importers, ".", "react", ">=17").is_some());
}

#[test]
fn fresh_resolves_when_range_no_longer_satisfies_locked_version() {
    let importers = single_dep_importer("react", "18.2.0");
    assert!(reusable_importer_dep(&importers, ".", "react", "^19.0.0").is_none());
}

#[test]
fn fresh_resolves_a_new_dependency_absent_from_the_lockfile() {
    let importers = single_dep_importer("react", "18.2.0");
    assert!(reusable_importer_dep(&importers, ".", "left-pad", "^1.0.0").is_none());
}
