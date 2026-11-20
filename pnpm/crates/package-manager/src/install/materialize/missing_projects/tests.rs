use super::is_local_dependency;
use pnpm_lockfile::ResolvedDependencySpec;

fn dependency(specifier: &str, version: &str) -> ResolvedDependencySpec {
    ResolvedDependencySpec {
        specifier: specifier.to_string(),
        version: version.parse().expect("parse the importer dependency version"),
    }
}

#[test]
fn local_references_are_local() {
    assert!(is_local_dependency(&dependency("workspace:*", "link:../project-2")));
    assert!(is_local_dependency(&dependency("file:../local-pkg", "file:local-pkg")));
    assert!(is_local_dependency(&dependency("file:../local-pkg", "local-pkg@file:local-pkg")));
}

#[test]
fn registry_references_are_not_local() {
    assert!(!is_local_dependency(&dependency("^1.0.0", "1.0.0")));
    assert!(!is_local_dependency(&dependency("npm:is-positive@^1.0.0", "is-positive@1.0.0")));
    assert!(!is_local_dependency(&dependency("repo.tgz", "1.0.0")));
}
