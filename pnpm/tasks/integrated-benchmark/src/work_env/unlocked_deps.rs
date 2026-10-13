//! The lockfile the `store-only.unlocked-direct-deps.*` scenario seeds.

use pnpm_lockfile::{ImporterDepVersion, Lockfile, ResolvedDependencyMap, ResolvedDependencySpec};
use std::path::Path;

/// `lockfile` with every direct dependency of the root importer whose
/// locked version carries a peer suffix left out of the importer. Each
/// such dependency stays in `package.json`, and its subtree stays in the
/// lockfile's package graph, so an install resolves the dependency afresh,
/// lands on the version the graph holds, and takes the subtree from the
/// lockfile. A dependency without peers would not do: the walk takes its
/// version from the lockfile's package list and never asks the registry.
pub(super) fn unlock_direct_deps(lockfile: &str) -> String {
    let mut lockfile = Lockfile::parse(lockfile, Path::new(Lockfile::FILE_NAME))
        .expect("parse the fixture lockfile")
        .expect("the fixture lockfile holds a document");
    let importer =
        lockfile.importers.get_mut(".").expect("the fixture lockfile has a root importer");
    let groups = [
        &mut importer.dependencies,
        &mut importer.dev_dependencies,
        &mut importer.optional_dependencies,
    ];
    for deps in groups.into_iter().flatten() {
        drop_peered(deps);
    }
    lockfile.to_yaml_string().expect("serialize the unlocked lockfile")
}

fn drop_peered(deps: &mut ResolvedDependencyMap) {
    deps.retain(|_, spec| !has_peer_suffix(spec));
}

/// Whether an importer dependency's locked version names the peers it
/// was resolved with, like `17.0.2(react@17.0.2)`.
pub(super) fn has_peer_suffix(spec: &ResolvedDependencySpec) -> bool {
    match &spec.version {
        ImporterDepVersion::Regular(version) => !version.peer().is_empty(),
        ImporterDepVersion::Alias(alias) => !alias.suffix.peer().is_empty(),
        _ => false,
    }
}
