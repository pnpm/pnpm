use super::{
    PrefetchedBinLookup, PrefetchedDepBin,
    direct::{PrefetchedBin, prefetched_bin_source, read_dep_manifest},
    existing_commands, read_location_bin_sources,
};
use pnpm_cmd_shim::{
    BinOrigin, Host, LinkBinsError, LinkBinsOptions, PackageBinSource, PreparedPackageBins,
    ShimTargetCache, collect_packages_in_modules_dir, link_bins_of_packages_outcome,
    link_bins_of_packages_precomputed,
};
use rayon::prelude::*;
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

/// The packages whose bins a pass links into `<modules_dir>/.bin`, in the form
/// the linker knows them.
///
/// A package without a `package.json` contributes nothing. Every other read or
/// parse error fails the pass.
#[derive(Clone, Copy)]
pub enum BinCandidates<'a> {
    /// Names linked under `modules_dir`, read through their links.
    Aliases(&'a [String]),
    /// Names linked under `modules_dir`, each with the directory its link
    /// points at.
    Resolved(&'a [(String, PathBuf)]),
    /// Names linked under `modules_dir`, with the lockfile facts that let a
    /// package skip its manifest read. See [`PrefetchedBinLookup`].
    Prefetched(&'a [PrefetchedDepBin], &'a PrefetchedBinLookup<'a>),
    /// Package directories that have no link under `modules_dir`.
    Locations(&'a [PathBuf]),
    /// Every package in `modules_dir`.
    ModulesDir,
    /// Every package in `modules_dir`. The named ones are the project's
    /// direct dependencies, and their commands win over the others'.
    ProjectModulesDir(&'a [String]),
    /// A project's top-level candidates. See [`TopLevelCandidates`].
    TopLevel(TopLevelCandidates<'a>),
}

/// The packages whose commands a project's `.bin` exposes. A command any
/// direct dependency has goes to it, else to a hoisted package, else to a
/// peer.
#[derive(Clone, Copy)]
pub struct TopLevelCandidates<'a> {
    /// Names linked under `modules_dir` from the project's dependency groups.
    pub direct: &'a [String],
    /// Names publicly hoisted to `modules_dir`.
    pub hoisted: &'a [String],
    /// The directories of auto-installed peers.
    pub peers: &'a [PathBuf],
}

/// The commands a pass leaves to another one.
#[derive(Clone, Copy)]
pub enum SkippedCommands<'a> {
    None,
    Listed(&'a HashSet<String>),
    /// Every command already in `<modules_dir>/.bin`.
    AlreadyLinked,
}

/// Whether builds that run after the pass may still create the candidates'
/// bin targets.
#[derive(Clone, Copy)]
pub enum BinTargets {
    Present,
    /// The pass holds back each bin whose target is missing. See
    /// [`PackageBinSource::build_pending`].
    MayBeBuilt,
}

/// How a pass links one `.bin` directory.
#[derive(Clone, Copy)]
pub struct BinsDirPass<'a> {
    pub skip: SkippedCommands<'a>,
    pub targets: BinTargets,
    pub link_options: &'a LinkBinsOptions,
    /// Bins already worked out for some candidate locations. The pass reads
    /// neither their manifests nor their bin targets again. A pass with it
    /// skips no commands and holds back no bins.
    pub prepared: Option<&'a HashMap<PathBuf, PreparedPackageBins>>,
}

impl<'a> BinsDirPass<'a> {
    #[must_use]
    pub fn new(link_options: &'a LinkBinsOptions) -> Self {
        BinsDirPass {
            skip: SkippedCommands::None,
            targets: BinTargets::Present,
            link_options,
            prepared: None,
        }
    }

    #[must_use]
    pub fn with_prepared(
        mut self,
        prepared: Option<&'a HashMap<PathBuf, PreparedPackageBins>>,
    ) -> Self {
        self.prepared = prepared;
        self
    }

    #[must_use]
    pub fn skipping(mut self, skip: SkippedCommands<'a>) -> Self {
        self.skip = skip;
        self
    }

    #[must_use]
    pub fn with_targets(mut self, targets: BinTargets) -> Self {
        self.targets = targets;
        self
    }
}

/// What a pass linked into one `.bin` directory.
#[derive(Debug, Default)]
pub struct LinkedBinsDir {
    pub linked_commands: HashSet<String>,
    /// Whether the pass held back a bin. See [`BinTargets::MayBeBuilt`].
    pub held_back: bool,
    /// The candidates that have a manifest, in candidate order.
    pub sources: Vec<PackageBinSource>,
}

/// Link the bins of `candidates` into `<modules_dir>/.bin`.
pub fn link_bins_dir(
    modules_dir: &Path,
    candidates: BinCandidates<'_>,
    pass: BinsDirPass<'_>,
) -> Result<LinkedBinsDir, LinkBinsError> {
    let build_pending = match pass.targets {
        BinTargets::Present => false,
        BinTargets::MayBeBuilt => true,
    };
    let sources: Vec<PackageBinSource> = read_candidates(modules_dir, candidates, pass.prepared)?
        .into_iter()
        .map(|source| {
            let build_pending = build_pending || source.build_pending;
            source.with_build_pending(build_pending)
        })
        .collect();
    if sources.is_empty() {
        return Ok(LinkedBinsDir::default());
    }
    let bins_dir = modules_dir.join(".bin");
    if let Some(prepared) = pass.prepared {
        let linked_commands = link_bins_of_packages_precomputed::<Host>(
            &sources,
            prepared,
            &bins_dir,
            pass.link_options,
        )?;
        return Ok(LinkedBinsDir { linked_commands, held_back: false, sources });
    }
    let cache = match candidates {
        BinCandidates::Prefetched(_, lookup) => Some(&lookup.shim_cache),
        _ => None,
    };
    link_sources(sources, &bins_dir, pass, cache)
}

fn link_sources(
    sources: Vec<PackageBinSource>,
    bins_dir: &Path,
    pass: BinsDirPass<'_>,
    cache: Option<&ShimTargetCache>,
) -> Result<LinkedBinsDir, LinkBinsError> {
    let skipped = match pass.skip {
        SkippedCommands::None => Cow::Owned(HashSet::new()),
        SkippedCommands::Listed(commands) => Cow::Borrowed(commands),
        SkippedCommands::AlreadyLinked => Cow::Owned(existing_commands(bins_dir)?),
    };
    let default_cache = ShimTargetCache::default();
    let outcome = link_bins_of_packages_outcome::<Host>(
        &sources,
        bins_dir,
        &skipped,
        pass.link_options,
        cache.unwrap_or(&default_cache),
    )?;
    Ok(LinkedBinsDir {
        linked_commands: outcome.linked_commands,
        held_back: outcome.held_back,
        sources,
    })
}

fn read_candidates(
    modules_dir: &Path,
    candidates: BinCandidates<'_>,
    prepared: Option<&HashMap<PathBuf, PreparedPackageBins>>,
) -> Result<Vec<PackageBinSource>, LinkBinsError> {
    match candidates {
        BinCandidates::Aliases(names) => names
            .par_iter()
            .filter_map(|name| read_alias(modules_dir, name, None))
            .collect(),
        BinCandidates::Resolved(deps) => deps
            .par_iter()
            .filter_map(|(name, target)| read_alias(modules_dir, name, Some(target)))
            .collect(),
        BinCandidates::Prefetched(deps, lookup) => deps
            .par_iter()
            .filter_map(|(name, target, snapshot_key)| {
                let source = match prefetched_bin_source(
                    modules_dir,
                    name,
                    target,
                    snapshot_key.as_ref(),
                    lookup,
                ) {
                    PrefetchedBin::NoBins => return None,
                    PrefetchedBin::Source(source) => Ok(source),
                    PrefetchedBin::ReadFromDisk => read_alias(modules_dir, name, Some(target))?,
                };
                let build_pending = lookup.is_build_pending(snapshot_key.as_ref());
                Some(source.map(|source| source.with_build_pending(build_pending)))
            })
            .collect(),
        BinCandidates::Locations(locations) => read_location_bin_sources(locations),
        BinCandidates::ModulesDir => collect_packages_in_modules_dir::<Host>(modules_dir),
        BinCandidates::ProjectModulesDir(direct) => read_project_modules_dir(modules_dir, direct),
        BinCandidates::TopLevel(top_level) => read_top_level(modules_dir, top_level, prepared),
    }
}

fn read_project_modules_dir(
    modules_dir: &Path,
    direct: &[String],
) -> Result<Vec<PackageBinSource>, LinkBinsError> {
    let direct_locations: HashSet<PathBuf> = direct
        .iter()
        .map(|name| modules_dir.join(name))
        .collect();
    Ok(collect_packages_in_modules_dir::<Host>(modules_dir)?
        .into_iter()
        .map(|source| {
            let origin = if direct_locations.contains(&source.location) {
                BinOrigin::Direct
            } else {
                BinOrigin::Hoisted
            };
            source.with_origin(origin)
        })
        .collect())
}

fn read_top_level(
    modules_dir: &Path,
    candidates: TopLevelCandidates<'_>,
    prepared: Option<&HashMap<PathBuf, PreparedPackageBins>>,
) -> Result<Vec<PackageBinSource>, LinkBinsError> {
    let direct: HashSet<&str> = candidates.direct
        .iter()
        .map(String::as_str)
        .collect();
    let hoisted_only: Vec<String> = candidates.hoisted
        .iter()
        .filter(|name| !direct.contains(name.as_str()))
        .cloned()
        .collect();
    let mut sources = Vec::new();
    for (names, origin) in
        [(candidates.direct, BinOrigin::Direct), (&hoisted_only[..], BinOrigin::Hoisted)]
    {
        let read = names
            .par_iter()
            .filter_map(|name| {
                match prepared.and_then(|prepared| prepared.get(&modules_dir.join(name))) {
                    Some(prepared) => Some(Ok(prepared.source().clone())),
                    None => read_alias(modules_dir, name, None),
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        sources.extend(
            read.into_iter()
                .map(|source| source.with_origin(origin)),
        );
    }
    let peers = read_location_bin_sources(candidates.peers)?;
    sources.extend(
        peers
            .into_iter()
            .map(|source| source.with_origin(BinOrigin::Peer)),
    );
    Ok(sources)
}

pub(super) fn read_alias(
    modules_dir: &Path,
    name: &str,
    target: Option<&Path>,
) -> Option<Result<PackageBinSource, LinkBinsError>> {
    read_dep_manifest(modules_dir, name, target)
        .map(|result| {
            result.map(|(location, manifest)| {
                let source = PackageBinSource::new(location, Arc::new(manifest));
                match target {
                    Some(target) => source.with_resolved_location(target.to_path_buf()),
                    None => source,
                }
            })
        })
}

#[cfg(test)]
mod tests;
