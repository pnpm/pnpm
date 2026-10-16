//! Record in the lockfile the tools that prepare each git-hosted package
//! (`lockBuildTools`).
//!
//! A git-hosted snapshot the previous lockfile already recorded keeps its
//! record, so the tools change only when the package re-resolves. Every
//! other one is read at its locked commit: when it needs a build, the
//! package manager and the Node.js it asks for are resolved and written as
//! `tool:` entries, reached through the snapshot's `buildDependencies`.

pub use errors::RecordBuildToolsError;
pub(crate) use package_read::PackageSources;

mod errors;
mod package_read;
mod tool_resolution;

use pnpm_lockfile::{
    Lockfile, PackageKey, PackageMetadata, PkgName, SnapshotDepRef, SnapshotEntry,
};
use pnpm_resolving_resolver_base::{ResolveOptions, Resolver};
use std::collections::HashMap;

/// The tools one package records: each one's name in `buildDependencies`,
/// its key, and its `packages:` entry.
type BuildToolRecord = HashMap<PkgName, (PackageKey, PackageMetadata)>;

/// The tool entries known so far: the previous lockfile's, and those this
/// pass resolved. Packages that need the same tool share one entry.
type ToolCatalog = HashMap<PackageKey, PackageMetadata>;

/// What records the tools: the resolver chain of the install, kept past
/// resolution only while this runs, and where the packages are read from.
pub(crate) struct BuildToolRecorder<'a> {
    pub resolver: &'a dyn Resolver,
    pub resolve_options: &'a ResolveOptions,
    pub sources: PackageSources<'a>,
}

impl BuildToolRecorder<'_> {
    /// Give every git-hosted snapshot of `lockfile` that needs a build its
    /// `buildDependencies`, carrying records over from `prior`.
    pub(crate) async fn record<Reporter: pnpm_reporter::Reporter>(
        &self,
        lockfile: &mut Lockfile,
        prior: Option<&Lockfile>,
    ) -> Result<(), RecordBuildToolsError> {
        let prior = prior
            .filter(|prior| pnpm_lockfile::recorded_lock_build_tools(prior.settings.as_ref()))
            .map(PriorRecords::of);
        let mut records: HashMap<PackageKey, Option<BuildToolRecord>> = HashMap::new();
        let mut unrecorded = Vec::new();
        for metadata_key in git_hosted_metadata_keys(lockfile) {
            match prior
                .as_ref()
                .and_then(|prior| prior.carried(lockfile, &metadata_key))
            {
                Some(record) => drop(records.insert(metadata_key, record)),
                None => unrecorded.push(metadata_key),
            }
        }
        let lockfile_view = &*lockfile;
        let environments = futures_util::future::try_join_all(
            unrecorded
                .iter()
                .map(|key| self.read_environment::<Reporter>(lockfile_view, key)),
        )
        .await?;
        let mut catalog = prior
            .as_ref()
            .map(PriorRecords::tools)
            .unwrap_or_default();
        for (metadata_key, environment) in unrecorded.into_iter().zip(environments) {
            let record = match environment {
                Some(environment) => Some(self.resolve_tools(&mut catalog, &environment).await?),
                None => None,
            };
            records.insert(metadata_key, record);
        }
        apply_records(lockfile, &records);
        Ok(())
    }

    async fn read_environment<Reporter: pnpm_reporter::Reporter>(
        &self,
        lockfile: &Lockfile,
        metadata_key: &PackageKey,
    ) -> Result<Option<pnpm_git_fetcher::BuildEnvironment>, RecordBuildToolsError> {
        let Some(metadata) = lockfile.packages
            .as_ref()
            .and_then(|packages| packages.get(metadata_key))
        else {
            return Ok(None);
        };
        self.sources.read_build_environment::<Reporter>(metadata_key, metadata).await
    }
}

/// The `packages:` keys of the git-hosted packages that have snapshots.
fn git_hosted_metadata_keys(lockfile: &Lockfile) -> Vec<PackageKey> {
    let (Some(packages), Some(snapshots)) = (&lockfile.packages, &lockfile.snapshots) else {
        return Vec::new();
    };
    let mut keys: Vec<PackageKey> = snapshots
        .keys()
        .map(PackageKey::without_peer)
        .filter(|key| {
            packages.get(key).is_some_and(|metadata| metadata.resolution.is_git_hosted())
        })
        .collect();
    keys.sort_by_key(ToString::to_string);
    keys.dedup();
    keys
}

/// The previous lockfile, with a snapshot of each package at hand.
struct PriorRecords<'a> {
    lockfile: &'a Lockfile,
    snapshots_by_package: HashMap<PackageKey, &'a SnapshotEntry>,
}

impl<'a> PriorRecords<'a> {
    fn of(lockfile: &'a Lockfile) -> Self {
        let snapshots_by_package = lockfile.snapshots
            .iter()
            .flatten()
            .map(|(key, snapshot)| (key.without_peer(), snapshot))
            .collect();
        PriorRecords { lockfile, snapshots_by_package }
    }

    /// The tools the previous lockfile's git-hosted snapshots record, which
    /// a package that needs one of them reuses when its version satisfies
    /// the package. An entry no such snapshot names is not offered.
    fn tools(&self) -> ToolCatalog {
        let Some(packages) = self.lockfile.packages.as_ref() else {
            return ToolCatalog::new();
        };
        self.snapshots_by_package
            .iter()
            .filter(|(package, _)| {
                packages
                    .get(*package)
                    .is_some_and(|metadata| metadata.resolution.is_git_hosted())
            })
            .flat_map(|(_, snapshot)| {
                snapshot.build_dependency_keys().map(|(_, key)| key.without_peer())
            })
            .filter_map(|key| {
                let metadata = packages.get(&key)?;
                pnpm_lockfile::build_tool_artifact(&key, &metadata.resolution)?;
                Some((key, metadata.clone()))
            })
            .collect()
    }

    /// What the previous lockfile recorded for the package at
    /// `metadata_key`, when it holds the same package and every tool it
    /// names is that tool's own artifact. `Some(None)` is a package it
    /// found needs no build.
    fn carried(
        &self,
        lockfile: &Lockfile,
        metadata_key: &PackageKey,
    ) -> Option<Option<BuildToolRecord>> {
        let resolution = |lockfile: &Lockfile| {
            lockfile.packages
                .as_ref()?
                .get(metadata_key)
                .map(|metadata| metadata.resolution.clone())
        };
        if resolution(self.lockfile)? != resolution(lockfile)? {
            return None;
        }
        let snapshot = self.snapshots_by_package.get(metadata_key)?;
        if snapshot.build_dependencies.is_none() {
            return Some(None);
        }
        let packages = self.lockfile.packages.as_ref()?;
        let record = snapshot
            .build_dependency_keys()
            .map(|(alias, key)| {
                let metadata = packages
                    .get(&key.without_peer())?
                    .clone();
                pnpm_lockfile::build_tool_artifact(&key, &metadata.resolution)?;
                Some((alias.clone(), (key, metadata)))
            })
            .collect::<Option<BuildToolRecord>>()?;
        Some(Some(record))
    }
}

/// Write `records` into `lockfile`: each package's snapshots get its
/// `buildDependencies`, and each tool its `packages:` entry.
fn apply_records(lockfile: &mut Lockfile, records: &HashMap<PackageKey, Option<BuildToolRecord>>) {
    let mut tools: HashMap<PackageKey, PackageMetadata> = HashMap::new();
    for (key, snapshot) in lockfile.snapshots.iter_mut().flatten() {
        let Some(record) = records.get(&key.without_peer()) else { continue };
        snapshot.build_dependencies = record
            .as_ref()
            .map(|record| {
                record
                    .iter()
                    .map(|(alias, (tool_key, _))| {
                        (alias.clone(), SnapshotDepRef::Plain(tool_key.suffix.clone()))
                    })
                    .collect()
            });
        for (tool_key, metadata) in record.iter().flat_map(HashMap::values) {
            tools.insert(tool_key.clone(), metadata.clone());
        }
    }
    // A tool gets no `snapshots:` entry: an install materializes every
    // snapshot it holds, and a tool is never installed.
    lockfile.packages.get_or_insert_default().extend(tools);
}

#[cfg(test)]
mod tests;
