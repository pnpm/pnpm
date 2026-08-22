//! Narrow a lockfile to what a subset of its importers reaches.
//!
//! Rust counterpart of `@pnpm/lockfile.filtering`'s
//! `filterLockfileByImporters`: the listed importers keep only the
//! dependency groups asked for, and `packages` / `snapshots` are pruned to
//! the transitive closure of what those importers still depend on. Every
//! other importer entry is carried through untouched, matching the
//! TypeScript, which spreads the original `importers` map and overwrites
//! only the listed ids.
//!
//! The walk mirrors `@pnpm/lockfile.walker`: it starts at the listed
//! importers' direct dependencies, follows each snapshot's `dependencies`
//! (and `optionalDependencies`, unless they were excluded), and treats
//! `skipped` keys as already visited so nothing behind them is retained.
//! `link:` references resolve to no snapshot key and simply end that
//! branch — they are workspace edges, not entries in the package graph.

use std::collections::{HashMap, HashSet, VecDeque};

use derive_more::{Display, Error};
use pnpm_diagnostics::miette::{self, Diagnostic};

use crate::{Lockfile, PackageKey, PkgNameVerPeer, ProjectSnapshot, ResolvedDependencyMap};

/// Dependency groups a filter keeps — the same three flags the modules
/// manifest records, redeclared here so the lockfile crate does not depend
/// on the modules-manifest crate for three booleans.
#[derive(Debug, Clone, Copy)]
pub struct IncludedDependencies {
    pub dependencies: bool,
    pub dev_dependencies: bool,
    pub optional_dependencies: bool,
}

impl Default for IncludedDependencies {
    /// Everything, the default of pnpm's own option.
    fn default() -> Self {
        IncludedDependencies {
            dependencies: true,
            dev_dependencies: true,
            optional_dependencies: true,
        }
    }
}

/// How [`Lockfile::filter_by_importers`] narrows the lockfile.
pub struct FilterByImportersOptions {
    /// Dependency groups the listed importers keep. A group left out is
    /// emptied on those importers and its edges are not walked.
    pub include: IncludedDependencies,
    /// Snapshot keys to treat as already visited — pnpm's `skipped` set,
    /// the optional dependencies this platform did not install. Neither
    /// they nor anything reachable only through them is retained.
    pub skipped: HashSet<PackageKey>,
    /// Whether a dependency reference with no `snapshots` entry is an
    /// error. `false` drops the reference and keeps walking, which is what
    /// a caller inspecting a possibly-stale lockfile wants.
    pub fail_on_missing_dependencies: bool,
}

/// A dependency reference the lockfile resolves to nothing.
#[derive(Debug, Display, Error, Diagnostic)]
#[display("Broken lockfile: no entry for {_0:?}")]
#[diagnostic(code(ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY))]
pub struct LockfileMissingDependencyError(#[error(not(source))] pub String);

impl Lockfile {
    /// The lockfile narrowed to what `importer_ids` reaches. See the
    /// module docs for the traversal.
    pub fn filter_by_importers(
        &self,
        importer_ids: Vec<String>,
        options: &FilterByImportersOptions,
    ) -> Result<Lockfile, LockfileMissingDependencyError> {
        let mut filtered = self.clone();
        // The walk starts at the *filtered* importers, so the seeds are
        // collected in the same pass that narrows them: a group `include`
        // excluded is emptied here and contributes no seed.
        let mut seeds: VecDeque<PackageKey> = VecDeque::new();
        for importer_id in importer_ids {
            let Some(importer) = filtered.importers.get_mut(&importer_id) else { continue };
            *importer = filter_importer(importer, options.include);
            for group in [
                importer.dependencies.as_ref(),
                importer.dev_dependencies.as_ref(),
                importer.optional_dependencies.as_ref(),
            ]
            .into_iter()
            .flatten()
            {
                for (alias, spec) in group {
                    if let Some(key) = spec.version.resolved_key(alias) {
                        seeds.push_back(key);
                    }
                }
            }
        }

        let reachable = collect_reachable(&filtered, seeds, options)?;
        let reachable_metadata: HashSet<_> =
            reachable.iter().map(PkgNameVerPeer::without_peer).collect();
        if let Some(snapshots) = filtered.snapshots.as_mut() {
            snapshots.retain(|key, _| reachable.contains(key));
        }
        if let Some(packages) = filtered.packages.as_mut() {
            packages.retain(|key, _| reachable_metadata.contains(key));
        }
        Ok(filtered)
    }
}

/// Empty the dependency groups `include` excludes. The other fields of the
/// importer entry (`dependenciesMeta`, `publishDirectory`) are dropped the
/// same way the TypeScript `filterImporter` drops them: the filtered
/// lockfile describes a dependency closure, not a publishable project.
fn filter_importer(importer: &ProjectSnapshot, include: IncludedDependencies) -> ProjectSnapshot {
    let pick = |group: Option<&ResolvedDependencyMap>, included: bool| {
        included.then(|| group.cloned()).flatten().unwrap_or_default()
    };
    ProjectSnapshot {
        specifiers: importer.specifiers.clone(),
        dependencies: Some(pick(importer.dependencies.as_ref(), include.dependencies)),
        dev_dependencies: Some(pick(importer.dev_dependencies.as_ref(), include.dev_dependencies)),
        optional_dependencies: Some(pick(
            importer.optional_dependencies.as_ref(),
            include.optional_dependencies,
        )),
        dependencies_meta: None,
        publish_directory: None,
    }
}

fn collect_reachable(
    lockfile: &Lockfile,
    mut queue: VecDeque<PackageKey>,
    options: &FilterByImportersOptions,
) -> Result<HashSet<PackageKey>, LockfileMissingDependencyError> {
    let empty = HashMap::new();
    let snapshots = lockfile.snapshots.as_ref().unwrap_or(&empty);
    // Seeded with `skipped`, as pnpm's walker seeds its `walked` set: a
    // skipped key is never entered, so nothing reachable only through it
    // is retained either.
    let mut walked = options.skipped.clone();
    let mut reachable = HashSet::new();

    while let Some(key) = queue.pop_front() {
        if !walked.insert(key.clone()) {
            continue;
        }
        let Some(snapshot) = snapshots.get(&key) else {
            if options.fail_on_missing_dependencies {
                return Err(LockfileMissingDependencyError(key.to_string()));
            }
            continue;
        };
        reachable.insert(key);
        for group in [
            snapshot.dependencies.as_ref(),
            options
                .include
                .optional_dependencies
                .then_some(snapshot.optional_dependencies.as_ref())
                .flatten(),
        ]
        .into_iter()
        .flatten()
        {
            for (alias, dep_ref) in group {
                if let Some(key) = dep_ref.resolve(alias) {
                    queue.push_back(key);
                }
            }
        }
    }
    Ok(reachable)
}

#[cfg(test)]
mod tests;
