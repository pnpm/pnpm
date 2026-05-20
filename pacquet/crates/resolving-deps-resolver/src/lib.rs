//! Minimum-slice port of pnpm's
//! [`resolveDependencyTree`](https://github.com/pnpm/pnpm/blob/f657b5cb44/installing/deps-resolver/src/resolveDependencyTree.ts).
//!
//! Walks a project manifest's direct dependencies through a
//! [`Resolver`](pacquet_resolving_resolver_base::Resolver) chain,
//! then recurses on every resolved package's own manifest
//! dependencies, producing a flat package map and a tree of parent-
//! child edges. Pacquet's install layer uses this as the resolve
//! pass before the tarball-fetch + install pass.
//!
//! This is intentionally a thin slice of upstream:
//!
//! - **Single importer.** Pacquet's install doesn't expose workspaces
//!   to the resolver yet; the entry point takes one manifest at a
//!   time. The flat-map shape ports cleanly when multi-importer
//!   lands.
//! - **No peers resolution.** Upstream's
//!   [`resolveRootDependencies`](https://github.com/pnpm/pnpm/blob/f657b5cb44/installing/deps-resolver/src/resolveDependencies.ts#L327)
//!   walks peer dependencies as a postponed phase; this port relies
//!   on `auto_install_peers` to fold peer-deps into the regular
//!   dependency walk.
//! - **No catalog / hook / lockfile-pinned-version bias.** The
//!   resolver is fed each child's manifest range verbatim. Lockfile-
//!   driven `preferred_versions` is a follow-up.

mod resolve_dependency_tree;
mod resolved_tree;

pub use resolve_dependency_tree::{
    ResolveDependencyTreeError, ResolveDependencyTreeOptions, resolve_dependency_tree,
};
pub use resolved_tree::{DirectDep, ResolvedPackage, ResolvedTree};

#[cfg(test)]
mod tests;
