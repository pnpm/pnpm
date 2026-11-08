use super::{DepHierarchy, HeldBackBinsDir, LinkHoistedModulesError, LinkHoistedModulesOpts};
use crate::{BinCandidates, BinTargets, BinsDirPass};
use pnpm_cmd_shim::PackageBinSource;
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
    pub(crate) claimed_commands: crate::ClaimedCommands,
}

impl HoistedBinLinkOutput {
    pub(super) fn merge(&mut self, other: Self) {
        self.held_back_bins_dirs.extend(other.held_back_bins_dirs);
        self.sources.extend(other.sources);
        self.claimed_commands.merge(other.claimed_commands);
    }
}

// Bundled packages are absent from the graph, so their bins need a separate filesystem pass.
///
/// Every `.bin` of the hoisted tree is linked again after the builds, so it
/// holds back the bins that a build may still create, and returns the directory
/// when it did.
pub(super) fn link_hierarchy_bins(
    hierarchy: &DepHierarchy,
    modules_dir: &Path,
    opts: &LinkHoistedModulesOpts<'_>,
) -> Result<HoistedBinLinkOutput, LinkHoistedModulesError> {
    let dep_names: Vec<String> = hierarchy.0
        .keys()
        .filter_map(|child_dir| opts.graph.get(child_dir))
        .filter_map(|node| node.alias.clone())
        .collect();
    if dep_names.is_empty() {
        return Ok(HoistedBinLinkOutput::default());
    }
    let linked = crate::link_bins_dir(
        modules_dir,
        BinCandidates::Aliases(&dep_names),
        BinsDirPass::new(opts.link_options).with_targets(BinTargets::MayBeBuilt),
    )
    .map_err(LinkHoistedModulesError::LinkBins)?;
    let mut output = HoistedBinLinkOutput::default();
    output.claimed_commands.claim(modules_dir, linked.linked_commands);
    output.sources.insert(modules_dir.to_path_buf(), linked.sources.into());
    if linked.held_back {
        output.held_back_bins_dirs.push(HeldBackBinsDir {
            modules_dir: modules_dir.to_path_buf(),
            dep_names,
        });
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
        let linked = crate::link_bins_dir(
            &modules_dir,
            BinCandidates::ModulesDir,
            BinsDirPass::new(opts.link_options).with_targets(BinTargets::MayBeBuilt),
        )
        .map_err(LinkHoistedModulesError::LinkBins)?;
        output.claimed_commands.claim(&modules_dir, linked.linked_commands);
        if linked.held_back {
            let dep_names = linked.sources
                .iter()
                .filter_map(|package| package.location.strip_prefix(&modules_dir).ok())
                .map(|name| name.to_string_lossy().into_owned())
                .collect();
            output.held_back_bins_dirs.push(HeldBackBinsDir {
                modules_dir: modules_dir.clone(),
                dep_names,
            });
        }
        output.sources.insert(modules_dir, linked.sources.into());
    }
    Ok(output)
}
