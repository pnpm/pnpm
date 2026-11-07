use super::{
    PrefetchedBinLookup, PrefetchedDepBin,
    direct::{PrefetchedBin, prefetched_bin_source, read_dep_manifest},
    existing_commands, read_location_bin_sources,
};
use pnpm_cmd_shim::{
    Host, LinkBinsError, LinkBinsOptions, PackageBinSource, ShimTargetCache,
    collect_packages_in_modules_dir, link_bins_of_packages_outcome,
};
use rayon::prelude::*;
use std::{
    borrow::Cow,
    collections::HashSet,
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
}

impl<'a> BinsDirPass<'a> {
    #[must_use]
    pub fn new(link_options: &'a LinkBinsOptions) -> Self {
        BinsDirPass { skip: SkippedCommands::None, targets: BinTargets::Present, link_options }
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
    let sources: Vec<PackageBinSource> = read_candidates(modules_dir, candidates)?
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
    let skipped = match pass.skip {
        SkippedCommands::None => Cow::Owned(HashSet::new()),
        SkippedCommands::Listed(commands) => Cow::Borrowed(commands),
        SkippedCommands::AlreadyLinked => Cow::Owned(existing_commands(&bins_dir)?),
    };
    let default_cache = ShimTargetCache::default();
    let cache = match candidates {
        BinCandidates::Prefetched(_, lookup) => &lookup.shim_cache,
        _ => &default_cache,
    };
    let outcome = link_bins_of_packages_outcome::<Host>(
        &sources,
        &bins_dir,
        &skipped,
        pass.link_options,
        cache,
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
    }
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
