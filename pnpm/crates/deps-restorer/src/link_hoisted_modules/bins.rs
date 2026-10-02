use super::{DepHierarchy, HeldBackBinsDir, LinkHoistedModulesError, LinkHoistedModulesOpts};
use pnpm_cmd_shim::{
    Host, PackageBinSource, ShimTargetCache, collect_packages_in_modules_dir,
    link_bins_of_packages_cached,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Ordered manifests already read by the initial hoisted bin pass.
pub type HoistedBinSources = HashMap<PathBuf, Arc<[PackageBinSource]>>;

#[derive(Debug, Default)]
pub(crate) struct HoistedBinLinkOutput {
    pub(crate) held_back_bins_dirs: Vec<HeldBackBinsDir>,
    pub(crate) sources: HoistedBinSources,
}

impl HoistedBinLinkOutput {
    pub(super) fn merge(&mut self, other: Self) {
        self.held_back_bins_dirs.extend(other.held_back_bins_dirs);
        self.sources.extend(other.sources);
    }
}

// Bundled packages are absent from the graph, so their bins need a separate filesystem pass.
///
/// Every `.bin` of the hoisted tree is linked again after the builds, so it
/// holds back the bins that a build may still create, and returns the directory
/// when it did.
pub(super) fn link_hierarchy_bins(
    hierarchy: &DepHierarchy,
    parent_dir: &Path,
    opts: &LinkHoistedModulesOpts<'_>,
) -> Result<HoistedBinLinkOutput, LinkHoistedModulesError> {
    let modules_dir = parent_dir.join("node_modules");
    let dep_names: Vec<String> = hierarchy.0
        .keys()
        .filter_map(|child_dir| opts.graph.get(child_dir))
        .filter_map(|node| node.alias.clone())
        .collect();
    if dep_names.is_empty() {
        return Ok(HoistedBinLinkOutput::default());
    }
    let deps = dep_names
        .iter()
        .map(|name| (name.as_str(), None))
        .collect::<Vec<_>>();
    let (held_back, packages) = crate::link_bins::link_named_dep_bins_with_sources(
        &modules_dir,
        &deps,
        opts.link_options,
        true,
    )
    .map_err(LinkHoistedModulesError::LinkBins)?;
    let mut output = HoistedBinLinkOutput::default();
    output.sources.insert(modules_dir.clone(), packages.into());
    if held_back {
        output.held_back_bins_dirs.push(HeldBackBinsDir { modules_dir, dep_names });
    }
    Ok(output)
}

/// Packages the tarball ships in its own `node_modules` are not graph nodes,
/// so [`link_hierarchy_bins`] never sees them; their bins are reachable only
/// from inside the bundling package. They are held back like graph packages'
/// bins, and the directories that held one back are returned for the build
/// phase to link again.
pub(super) fn link_bundled_bins(
    hierarchy: &DepHierarchy,
    opts: &LinkHoistedModulesOpts<'_>,
) -> Result<HoistedBinLinkOutput, LinkHoistedModulesError> {
    let mut output = HoistedBinLinkOutput::default();
    for child_dir in hierarchy.0.keys() {
        let bundles =
            opts.graph.get(child_dir).is_some_and(|node| node.package.has_bundled_dependencies);
        if !bundles {
            continue;
        }
        let modules_dir = child_dir.join("node_modules");
        let packages: Vec<PackageBinSource> = collect_packages_in_modules_dir::<Host>(&modules_dir)
            .map_err(LinkHoistedModulesError::LinkBins)?
            .into_iter()
            .map(|package| package.with_build_pending(true))
            .collect();
        let held_back = link_bins_of_packages_cached::<Host>(
            &packages,
            &modules_dir.join(".bin"),
            opts.link_options,
            &ShimTargetCache::default(),
        )
        .map_err(LinkHoistedModulesError::LinkBins)?;
        if held_back {
            let dep_names = packages
                .iter()
                .filter_map(|package| package.location.strip_prefix(&modules_dir).ok())
                .map(|name| name.to_string_lossy().into_owned())
                .collect();
            output.held_back_bins_dirs.push(HeldBackBinsDir {
                modules_dir: modules_dir.clone(),
                dep_names,
            });
        }
        output.sources.insert(modules_dir, packages.into());
    }
    Ok(output)
}
