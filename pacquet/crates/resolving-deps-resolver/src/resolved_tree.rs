use std::collections::HashMap;

use pacquet_resolving_resolver_base::{ResolutionPolicyViolation, ResolveResult};

/// Output of [`super::resolve_dependency_tree()`]. A flat package map
/// keyed by `name@version` plus the importer's direct entries, so the
/// install pass can traverse the graph without re-resolving and skip
/// duplicates by ID.
///
/// Mirrors upstream's
/// [`ResolveDependencyTreeResult`](https://github.com/pnpm/pnpm/blob/f657b5cb44/installing/deps-resolver/src/resolveDependencyTree.ts#L151-L170)
/// shape — `direct` carries the project's manifest-level entries, the
/// flat map carries every transitively-resolved package.
#[derive(Debug, Default, Clone)]
pub struct ResolvedTree {
    pub direct: Vec<DirectDep>,
    pub packages: HashMap<String, ResolvedPackage>,
    pub policy_violations: Vec<ResolutionPolicyViolation>,
}

/// One edge in the resolved tree: the local install name (`alias`) and
/// the resolved package's ID (`name@version`). Same shape for top-
/// level (project manifest) entries and for transitive (parent
/// package) entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectDep {
    /// Local install name in `node_modules`. For an npm-alias entry
    /// (`"foo": "npm:bar@^1"`) this is `"foo"`; the resolved
    /// package's real name is recoverable from
    /// [`ResolvedPackage::result`].
    pub alias: String,
    /// `name@version` key into [`ResolvedTree::packages`].
    pub id: String,
}

/// A single resolved package and its outgoing edges. Mirrors upstream's
/// [`ResolvedPackage`](https://github.com/pnpm/pnpm/blob/f657b5cb44/installing/deps-resolver/src/resolveDependencies.ts#L168-L189)
/// envelope as far as the npm-shaped install path cares.
#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    pub id: String,
    pub result: ResolveResult,
    pub children: Vec<DirectDep>,
}
