use super::{
    HoistedLinkerError, HoistedLinkerInputs, Lockfile, Path, PathBuf, Reporter, SkippedSnapshots,
};
use crate::SymlinkDirectDependencies;
use std::collections::{HashMap, HashSet};

const ALL_DEPENDENCY_GROUPS: [super::DependencyGroup; 3] =
    [super::DependencyGroup::Prod, super::DependencyGroup::Dev, super::DependencyGroup::Optional];

// The hoisted walker skips workspace links, which still need symlinks in each importer.
pub(super) fn link_hoisted_workspace_dependencies<Reporter: self::Reporter>(
    inputs: &HoistedLinkerInputs<'_>,
    lockfile: &Lockfile,
    skipped: &SkippedSnapshots,
    link_options: &pnpm_cmd_shim::LinkBinsOptions,
    claimed_commands: &mut HashMap<PathBuf, HashSet<String>>,
) -> Result<HashSet<PathBuf>, HoistedLinkerError> {
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
    let trusted = trusted_importer_ids(inputs);
    let workspace_claimed = SymlinkDirectDependencies {
        context: crate::ImporterLinkContext {
            config: inputs.config,
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
            trusted_importer_ids: Some(&trusted),
            link_only: true,
        },
        dependency_groups: ALL_DEPENDENCY_GROUPS.into_iter(),
        // `link_only` keeps only `link:` siblings, which have no
        // lockfile row for a prefetched manifest to serve.
        package_manifests: None,
        package_bins: None,
        requires_build_by_snapshot: None,
        scheduled_builds: None,
    }
    .run_claiming_bins::<Reporter>()
    .map_err(HoistedLinkerError::SymlinkDirectDependencies)?;
    for (modules_dir, commands) in workspace_claimed {
        claimed_commands
            .entry(modules_dir)
            .or_default()
            .extend(commands);
    }
    Ok(changed_dirs)
}

pub(super) fn prune_importer_hoisted_bins(
    inputs: &HoistedLinkerInputs<'_>,
    claimed_commands: &HashMap<PathBuf, HashSet<String>>,
) -> Result<(), HoistedLinkerError> {
    let root_modules_dir = pnpm_fs::lexical_normalize(&inputs.config.modules_dir);
    let mut prunes: HashMap<PathBuf, (PathBuf, HashSet<String>)> = HashMap::new();
    record_hoisted_prune_dir(
        &mut prunes,
        claimed_commands,
        &inputs.config.modules_dir,
        &root_modules_dir,
        std::slice::from_ref(&root_modules_dir),
    );
    for (project_dir, _) in inputs.projects.manifests {
        record_project_hoisted_prune_dir(&mut prunes, inputs, claimed_commands, project_dir);
    }
    for (_, (modules_dir, claimed)) in prunes {
        pnpm_cmd_shim::prune_stale_bins(&modules_dir.join(".bin"), &claimed)
            .map_err(HoistedLinkerError::HoistLinkBins)?;
    }
    Ok(())
}

fn record_project_hoisted_prune_dir(
    prunes: &mut HashMap<PathBuf, (PathBuf, HashSet<String>)>,
    inputs: &HoistedLinkerInputs<'_>,
    claimed_commands: &HashMap<PathBuf, HashSet<String>>,
    project_dir: &Path,
) {
    let modules_dir_name = inputs.config.modules_dir_name();
    let lexical_dir = pnpm_fs::lexical_normalize(&project_dir.join(modules_dir_name));
    if pnpm_fs::lexical_normalize(project_dir)
        == pnpm_fs::lexical_normalize(inputs.projects.walker_lockfile_dir)
    {
        let root_modules_dir = pnpm_fs::lexical_normalize(&inputs.config.modules_dir);
        record_hoisted_prune_dir(
            prunes,
            claimed_commands,
            &inputs.config.modules_dir,
            &root_modules_dir,
            &[root_modules_dir.clone(), lexical_dir],
        );
        return;
    }
    let real_dir = std::fs::canonicalize(project_dir)
        .ok()
        .map(|real_project| pnpm_fs::lexical_normalize(&real_project.join(modules_dir_name)));
    let target_dir = real_dir.as_ref().unwrap_or(&lexical_dir);
    let candidates: Vec<PathBuf> =
        std::iter::once(lexical_dir.clone()).chain(real_dir.clone()).collect();
    record_hoisted_prune_dir(prunes, claimed_commands, target_dir, target_dir, &candidates);
}

fn record_hoisted_prune_dir(
    prunes: &mut HashMap<PathBuf, (PathBuf, HashSet<String>)>,
    claimed_commands: &HashMap<PathBuf, HashSet<String>>,
    physical_hint: &Path,
    modules_dir: &Path,
    candidate_keys: &[PathBuf],
) {
    let key = std::fs::canonicalize(physical_hint)
        .unwrap_or_else(|_| pnpm_fs::lexical_normalize(physical_hint));
    let (_, claimed) = prunes
        .entry(key)
        .or_insert_with(|| (modules_dir.to_path_buf(), HashSet::new()));
    for candidate in candidate_keys {
        if let Some(commands) = claimed_commands.get(candidate) {
            claimed.extend(commands.iter().cloned());
        }
    }
}

/// Importer ids backed by the install's own declared projects, relative to
/// `walker_lockfile_dir`.
fn trusted_importer_ids(inputs: &HoistedLinkerInputs<'_>) -> HashSet<String> {
    inputs.projects.manifests
        .iter()
        .map(|(project_dir, _)| {
            pnpm_workspace::importer_id_from_root_dir(
                inputs.projects.walker_lockfile_dir,
                project_dir,
            )
        })
        .collect()
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
                .dependencies_by_groups(ALL_DEPENDENCY_GROUPS)
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
