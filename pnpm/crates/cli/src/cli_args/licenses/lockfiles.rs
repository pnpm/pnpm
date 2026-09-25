use super::{
    BelongsTo, Include, PackageDirs, collect_dependencies, compare_package_names, compare_versions,
};
use crate::cli_args::recursive::{
    AutoExcludeRoot, discover_workspace_projects, select_recursive_projects,
};
use miette::IntoDiagnostic;
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, PackageKey};
use pnpm_package_is_installable::InstallabilityOptions;
use pnpm_package_manager::{
    AllowBuildPolicy, validate_virtual_store_slot_containment, virtual_store_layout_for_lockfile,
};
use pnpm_package_manifest::{
    node_version_from_engines_runtime, safe_read_project_manifest_from_dir,
};
use pnpm_workspace::importer_id_from_root_dir;
use pnpm_workspace_projects_graph::BaseProject;
use std::{
    cmp::Ordering,
    collections::HashMap,
    path::{Path, PathBuf},
};

/// The packages a report lists, each paired with the index of the
/// lockfile whose install holds its files.
pub(super) struct LicensedPackages {
    pub package_dirs: Vec<PackageDirs>,
    pub dependencies: Vec<(usize, LicensedDependency)>,
}

impl LicensedPackages {
    /// Walk the lockfiles of the listed projects. `None` when none of
    /// them has a lockfile.
    pub(super) fn collect(
        config: &Config,
        dir: &Path,
        recursive: bool,
        include: Include,
    ) -> miette::Result<Option<Self>> {
        let walk = LockfileWalk {
            config,
            dir,
            include,
            installability: InstallabilityOptions {
                supported_architectures: config.supported_architectures.as_ref(),
                current_os: pnpm_detect_libc::host_platform(),
                current_cpu: pnpm_detect_libc::host_arch(),
                current_libc: pnpm_graph_hasher::host_libc(),
                ..Default::default()
            },
        };
        let mut packages = Self { package_dirs: Vec::new(), dependencies: Vec::new() };
        for licensed in licensed_lockfiles(config, dir, recursive)? {
            let Some((package_dirs, dependencies)) = walk.packages(&licensed)? else {
                continue;
            };
            let lockfile_index = packages.package_dirs.len();
            packages.package_dirs.push(package_dirs);
            packages.dependencies.extend(
                dependencies
                    .into_iter()
                    .map(|dependency| (lockfile_index, dependency)),
            );
        }
        if packages.package_dirs.is_empty() {
            return Ok(None);
        }
        // Dedicated lockfiles each contribute an ordered run; the report
        // needs one order across all of them.
        if packages.package_dirs.len() > 1 {
            packages.dependencies.sort_by(|(_, left), (_, right)| {
                compare_licensed_dependencies(left, right)
            });
        }
        Ok(Some(packages))
    }
}

/// What every lockfile of one report is walked with.
struct LockfileWalk<'a> {
    config: &'a Config,
    dir: &'a Path,
    include: Include,
    installability: InstallabilityOptions<'a>,
}

impl LockfileWalk<'_> {
    /// Where the packages of one lockfile are installed and the packages
    /// its listed importers depend on. `None` when the lockfile is missing.
    fn packages(
        &self,
        licensed: &LicensedLockfile,
    ) -> miette::Result<Option<(PackageDirs, Vec<LicensedDependency>)>> {
        let Some(lockfile) =
            Lockfile::load_wanted_from_dir(&licensed.lockfile_dir).into_diagnostic()?
        else {
            return Ok(None);
        };
        let (layout_config, manifest_dir) = match &licensed.project_config {
            Some(project_config) => (project_config, licensed.lockfile_dir.as_path()),
            None => (self.config, self.dir),
        };
        let layout =
            lockfile_layout(layout_config, manifest_dir, &licensed.lockfile_dir, &lockfile)?;
        let package_dirs = PackageDirs::new(layout_config, &licensed.lockfile_dir, layout)?;
        let belongs_to = collect_dependencies(
            &lockfile,
            &licensed.importer_ids,
            self.include,
            &self.installability,
            self.config.peer_edge_options(),
        );
        Ok(Some((package_dirs, sorted_licensed_dependencies(&lockfile, belongs_to))))
    }
}

/// A lockfile to read and the importers of it whose dependencies are
/// listed.
struct LicensedLockfile {
    lockfile_dir: PathBuf,
    /// `config` re-anchored on the project that owns a dedicated lockfile
    /// (`sharedWorkspaceLockfile: false`), whose virtual store sits in that
    /// project rather than in the workspace root.
    project_config: Option<Config>,
    importer_ids: Vec<String>,
}

/// The lockfiles recording the listed importers — the `--filter`
/// selection under `--recursive`, the project in `dir` otherwise. A
/// workspace with dedicated lockfiles has one per project.
fn licensed_lockfiles(
    config: &Config,
    dir: &Path,
    recursive: bool,
) -> miette::Result<Vec<LicensedLockfile>> {
    let projects = listed_projects(config, dir, recursive)?;
    if config.shares_one_lockfile() || config.workspace_dir.is_none() {
        let lockfile_dir = config.lockfile_dir_for(dir).to_path_buf();
        let importer_ids = projects
            .iter()
            .map(|(project_dir, _)| importer_id_from_root_dir(&lockfile_dir, project_dir))
            .collect();
        return Ok(vec![LicensedLockfile { lockfile_dir, project_config: None, importer_ids }]);
    }
    Ok(projects
        .into_iter()
        .map(|(project_dir, name)| {
            let mut project_config = config.clone();
            project_config.anchor_dedicated_project(&project_dir, name.as_deref());
            LicensedLockfile {
                importer_ids: vec![importer_id_from_root_dir(&project_dir, &project_dir)],
                lockfile_dir: project_dir,
                project_config: Some(project_config),
            }
        })
        .collect())
}

/// The directory and manifest name of each project whose dependencies
/// are listed.
fn listed_projects(
    config: &Config,
    dir: &Path,
    recursive: bool,
) -> miette::Result<Vec<(PathBuf, Option<String>)>> {
    if !recursive {
        let name = safe_read_project_manifest_from_dir(dir)
            .into_diagnostic()?
            .and_then(|manifest| {
                manifest
                    .get("name")?
                    .as_str()
                    .map(ToString::to_string)
            });
        return Ok(vec![(dir.to_path_buf(), name)]);
    }
    let workspace_root = config.workspace_dir.as_deref().unwrap_or(dir);
    let (projects, _) = discover_workspace_projects(workspace_root, config)?;
    let selection = select_recursive_projects(&projects, config, dir, AutoExcludeRoot::Disabled)?;
    Ok(selection.selected
        .iter()
        .map(|(project_dir, project)| {
            (project_dir.clone(), project.package.manifest_name().map(ToString::to_string))
        })
        .collect())
}

/// A listed package: its lockfile key, the dependency group it belongs
/// to, its name and its manifest version.
pub(super) type LicensedDependency = (PackageKey, BelongsTo, String, String);

/// The listed packages with their manifest versions, in the order the
/// report renders them.
fn sorted_licensed_dependencies(
    lockfile: &Lockfile,
    belongs_to: HashMap<PackageKey, BelongsTo>,
) -> Vec<LicensedDependency> {
    let pkgs = lockfile.packages.as_ref();
    let mut dependencies = belongs_to
        .into_iter()
        .map(|(key, kind)| {
            let name = key.name.to_string();
            let version = pkgs
                .and_then(|packages| packages.get(&key.without_peer()))
                .and_then(|meta| meta.version.clone())
                .unwrap_or_else(|| key.suffix.version().to_string());
            (key, kind, name, version)
        })
        .collect::<Vec<_>>();
    dependencies.sort_by(compare_licensed_dependencies);
    dependencies
}

fn compare_licensed_dependencies(
    left: &LicensedDependency,
    right: &LicensedDependency,
) -> Ordering {
    compare_package_names(&left.2, &right.2)
        .then_with(|| compare_versions(&left.3, &right.3))
        .then_with(|| {
            left.0
                .to_string()
                .cmp(&right.0.to_string())
        })
        .then_with(|| left.1.cmp(&right.1))
}

/// Where each locked package's files live, checked to sit inside the
/// virtual store.
pub(super) fn lockfile_layout(
    config: &Config,
    dir: &Path,
    lockfile_dir: &Path,
    lockfile: &Lockfile,
) -> miette::Result<pnpm_deps_restorer::VirtualStoreLayout> {
    let allow_build_policy = AllowBuildPolicy::from_config(config).into_diagnostic()?;
    let project_manifest = safe_read_project_manifest_from_dir(dir).into_diagnostic()?;
    let manifest_node_version =
        project_manifest.as_ref().and_then(node_version_from_engines_runtime);
    let effective_node_version =
        config.node_version.as_deref().or(manifest_node_version.as_deref());
    let layout = virtual_store_layout_for_lockfile(
        config,
        effective_node_version,
        lockfile.snapshots.as_ref(),
        lockfile.packages.as_ref(),
        Some(&allow_build_policy),
        Some(lockfile_dir),
    );
    validate_virtual_store_slot_containment(lockfile.snapshots.as_ref(), &layout)
        .into_diagnostic()?;
    Ok(layout)
}
