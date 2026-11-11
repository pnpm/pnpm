use crate::current_lockfile::MaterializationClosure;
use pnpm_lockfile::Lockfile;
use pnpm_modules_yaml::IncludedDependencies;
use std::collections::HashSet;

fn closure(yaml: &str) -> MaterializationClosure {
    let lockfile: Lockfile = serde_saphyr::from_str(yaml).expect("parse lockfile");
    MaterializationClosure { lockfile, importer_ids: HashSet::from([".".to_string()]) }
}

fn included(optional_dependencies: bool) -> IncludedDependencies {
    IncludedDependencies { dependencies: true, dev_dependencies: false, optional_dependencies }
}

#[test]
fn finds_runtime_dependencies_in_selected_snapshots() {
    let closure = closure(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      library: 1.0.0\npackages:\n  library@1.0.0: {}\n  node@runtime:22.0.0: {}\nsnapshots:\n  library@1.0.0:\n    dependencies:\n      node: runtime:22.0.0\n  node@runtime:22.0.0: {}\n",
    );

    assert_eq!(closure.runtime_dependency_alias(included(false)), Some("node".to_string()));
}

#[test]
fn ignores_optional_runtime_dependencies_when_optionals_are_excluded() {
    let closure = closure(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      library: 1.0.0\npackages:\n  library@1.0.0: {}\n  node@runtime:22.0.0: {}\nsnapshots:\n  library@1.0.0:\n    optionalDependencies:\n      node: runtime:22.0.0\n  node@runtime:22.0.0: {}\n",
    );

    assert_eq!(closure.runtime_dependency_alias(included(false)), None);
}

#[test]
fn finds_optional_runtime_dependencies_when_optionals_are_included() {
    let closure = closure(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      library: 1.0.0\npackages:\n  library@1.0.0: {}\n  node@runtime:22.0.0: {}\nsnapshots:\n  library@1.0.0:\n    optionalDependencies:\n      node: runtime:22.0.0\n  node@runtime:22.0.0: {}\n",
    );

    assert_eq!(closure.runtime_dependency_alias(included(true)), Some("node".to_string()));
}
