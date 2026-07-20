//! Classify every depPath in a lockfile as dev-only, prod-only, or
//! both. Rust counterpart of `@pnpm/lockfile.detect-dep-types`.

use std::collections::{HashMap, HashSet};

use pacquet_lockfile::{Lockfile, PkgNameVerPeer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DepType {
    DevOnly,
    DevAndProd,
    ProdOnly,
}

pub(crate) type DepTypes = HashMap<PkgNameVerPeer, DepType>;

pub(crate) fn detect_dep_types(lockfile: &Lockfile) -> DepTypes {
    let mut ctx = Ctx {
        lockfile,
        walked: HashSet::new(),
        not_prod_only: HashSet::new(),
        dep_types: HashMap::new(),
    };

    let group_dep_paths = |group: fn(
        &pacquet_lockfile::ProjectSnapshot,
    ) -> Option<&pacquet_lockfile::ResolvedDependencyMap>| {
        lockfile
            .importers
            .values()
            .filter_map(group)
            .flat_map(|deps| {
                deps.iter().filter_map(|(alias, spec)| spec.version.resolved_key(alias))
            })
            .collect::<Vec<_>>()
    };

    let dev_dep_paths = group_dep_paths(|importer| importer.dev_dependencies.as_ref());
    let optional_dep_paths = group_dep_paths(|importer| importer.optional_dependencies.as_ref());
    let prod_dep_paths = group_dep_paths(|importer| importer.dependencies.as_ref());

    detect_in_subgraph(&mut ctx, &dev_dep_paths, true);
    detect_in_subgraph(&mut ctx, &optional_dep_paths, false);
    detect_in_subgraph(&mut ctx, &prod_dep_paths, false);
    ctx.dep_types
}

struct Ctx<'a> {
    lockfile: &'a Lockfile,
    walked: HashSet<(PkgNameVerPeer, bool)>,
    not_prod_only: HashSet<PkgNameVerPeer>,
    dep_types: DepTypes,
}

fn detect_in_subgraph(ctx: &mut Ctx<'_>, dep_paths: &[PkgNameVerPeer], dev: bool) {
    for dep_path in dep_paths {
        if !ctx.walked.insert((dep_path.clone(), dev)) {
            continue;
        }
        let Some(snapshot) =
            ctx.lockfile.snapshots.as_ref().and_then(|snapshots| snapshots.get(dep_path))
        else {
            continue;
        };
        if dev {
            ctx.not_prod_only.insert(dep_path.clone());
            ctx.dep_types.insert(dep_path.clone(), DepType::DevOnly);
        } else if ctx.dep_types.get(dep_path) == Some(&DepType::DevOnly) {
            ctx.dep_types.insert(dep_path.clone(), DepType::DevAndProd);
        } else if !ctx.dep_types.contains_key(dep_path) && !ctx.not_prod_only.contains(dep_path) {
            ctx.dep_types.insert(dep_path.clone(), DepType::ProdOnly);
        }
        for group in [&snapshot.dependencies, &snapshot.optional_dependencies] {
            let child_paths: Vec<PkgNameVerPeer> = group
                .iter()
                .flatten()
                .filter_map(|(alias, dep_ref)| dep_ref.resolve(alias))
                .collect();
            detect_in_subgraph(ctx, &child_paths, dev);
        }
    }
}
