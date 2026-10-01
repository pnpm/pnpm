use super::{
    HoistedLinkerError, HoistedLinkerInputs, Lockfile, PathBuf, Reporter, SkippedSnapshots,
};
use crate::SymlinkDirectDependencies;
use std::collections::HashSet;

// The hoisted walker skips workspace links, which still need symlinks in each importer.
pub(super) fn link_hoisted_workspace_dependencies<Reporter: self::Reporter>(
    inputs: &HoistedLinkerInputs<'_>,
    lockfile: &Lockfile,
    skipped: &SkippedSnapshots,
    link_options: &pnpm_cmd_shim::LinkBinsOptions,
) -> Result<HashSet<PathBuf>, HoistedLinkerError> {
    let config = inputs.config;
    let changed_dirs = workspace_linked_bin_dirs(inputs);
    // Workspace `link:` deps still need symlinks under each importer's
    // `node_modules/<alias>` even though the regular deps now live as
    // real directories. The hoisted dep-graph walker skips
    // `workspace:`-prefixed references entirely (they're not in the
    // hoist tree), so without this pass workspace siblings would be
    // missing from each project's `node_modules/`. `link_only: true`
    // filters every other dep out so the call doesn't try to re-create
    // symlinks for packages that the hoisted linker already wrote as
    // real dirs.
    // Importer ids backed by the install's own declared projects —
    // allowed outside the lockfile dir (see the isolated-path use).
    // Ids are lockfile-dir-relative, so derive them against
    // `walker_lockfile_dir`.
    let trusted_importer_ids: std::collections::HashSet<String> = inputs
        .projects
        .manifests
        .iter()
        .map(|(project_dir, _)| {
            pnpm_workspace::importer_id_from_root_dir(
                inputs.projects.walker_lockfile_dir,
                project_dir,
            )
        })
        .collect();
    SymlinkDirectDependencies {
        context: crate::ImporterLinkContext {
            config,
            layout: inputs.graph.layout,
            workspace_root: inputs.projects.symlink_workspace_root,
            link_options,
        },
        graph: crate::ImporterDependencyGraph {
            importers: inputs.projects.importers,
            packages: lockfile.packages.as_ref(),
            skipped,
        },
        policy: crate::DirectLinkPolicy {
            public_hoist_targets: None,
            trusted_importer_ids: Some(&trusted_importer_ids),
            link_only: true,
        },

        dependency_groups: inputs.projects.dependency_groups.iter().copied(),

        // Hoisted-linker path has no public-hoist virtual store to
        // dedupe against; the real-directory tree is the hoist layout.

        // pnpm gates `extraNodePaths` on the isolated linker, so the
        // hoisted linker's shims never carry `NODE_PATH`.

        // `link_only` keeps only `link:` siblings, which have no
        // lockfile row for a prefetched manifest to serve.
        package_manifests: None,
        requires_build_by_snapshot: None,
        scheduled_builds: None,
    }
    .run::<Reporter>()
    .map_err(HoistedLinkerError::SymlinkDirectDependencies)?;
    Ok(changed_dirs)
}

fn workspace_linked_bin_dirs(inputs: &HoistedLinkerInputs<'_>) -> HashSet<PathBuf> {
    inputs.projects.manifests
        .iter()
        .filter_map(|(project_dir, _)| {
            let importer_id = pnpm_workspace::importer_id_from_root_dir(
                inputs.projects.walker_lockfile_dir,
                project_dir,
            );
            let snapshot = inputs.projects.importers.get(&importer_id)?;
            let has_links = snapshot
                .dependencies_by_groups(inputs.projects.dependency_groups.iter().copied())
                .any(|(_, dependency)| {
                    matches!(dependency.version, pnpm_lockfile::ImporterDepVersion::Link(_))
                });
            has_links.then(|| {
                if pnpm_fs::lexical_normalize(project_dir)
                    == pnpm_fs::lexical_normalize(inputs.projects.walker_lockfile_dir)
                {
                    inputs.config.modules_dir.clone()
                } else {
                    project_dir.join(inputs.config.modules_dir_name())
                }
            })
        })
        .collect()
}
