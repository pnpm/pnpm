//! The hoisted node-linker: a flat `node_modules` instead of a virtual store.

pub use hoist_plan::{
    HoistPlan, collect_public_hoist_targets, compute_hoist_plan, find_own_runtime_node_major,
    find_runtime_node_major, parse_major_from_version, workspace_packages_for_hoist,
};

mod hoist_plan;

use super::{
    AtomicU8, BTreeMap, BTreeSet, Config, DependencyGroup, Diagnostic, Display, Error, HashMap,
    HoistedDepGraphError, IncludedDependencies, LinkHoistedModulesError, LinkHoistedModulesOpts,
    Lockfile, LockfileToHoistedDepGraphOptions, NodeLinker, OsStr, PackageKey, Path, PathBuf,
    Reporter, SkippedSnapshots, SymlinkDirectDependencies, SymlinkDirectDependenciesError,
    SymlinkPackageError, link_hoisted_modules, lockfile_to_hoisted_dep_graph,
};

/// Internal handoff between the hoisted-linker walker/linker pass
/// and the downstream `BuildModules` + `.modules.yaml` writes. Bundled
/// as a struct so the hoisted branch in [`crate::install_frozen_lockfile::InstallFrozenLockfile::run`]
/// can return both fields in one binding without tripping
/// `clippy::type_complexity`. Always [`Default`]-empty for the
/// isolated linker.
#[derive(Debug, Default)]
pub struct HoistedLinkerOutput {
    /// `LockfileToDepGraphResult::hoisted_locations` from the slice
    /// 4 walker. Persisted into `.modules.yaml.hoisted_locations`
    /// when non-empty.
    pub hoisted_locations: BTreeMap<String, Vec<String>>,
    /// Per-snapshot `pkgRoot` override for the build phase — snapshot
    /// key → every directory the hoisted graph placed it in, in walker
    /// order. `None` for the isolated linker (the layout-based lookup in
    /// `BuildModules` is used instead). See
    /// [`crate::BuildLayout::pkg_roots_by_key`] for how the list is
    /// consumed.
    pub hoisted_pkg_roots_by_key: Option<HashMap<PackageKey, Vec<std::path::PathBuf>>>,
    /// The snapshots the build phase should consider: those the linker
    /// imported this install, plus every present one when
    /// [`crate::PriorHoistedState::build_present_packages`] is set. Replaces
    /// `CreateVirtualStore`'s materialized list for the hoisted linker,
    /// whose snapshots all survive its skip filter.
    pub hoisted_build_snapshots: Option<Vec<PackageKey>>,
}

/// Inputs to [`run_hoisted_linker`]. Bundled so the two install
/// paths ([`crate::install_frozen_lockfile::InstallFrozenLockfile`] and `InstallWithFreshLockfile`)
/// can feed the shared hoisted-linker materialization without a
/// long positional argument list. The frozen path passes the
/// loaded `pnpm-lock.yaml`; the fresh path passes the freshly-built
/// lockfile and `current_lockfile: None`.
pub struct HoistedLinkerInputs<'a> {
    pub graph: crate::HoistedLinkGraph<'a>,
    pub prior: crate::PriorHoistedState<'a>,
    pub projects: crate::HoistedProjects<'a>,
    pub config: &'static Config,
    /// `(node_detected, node_version)` from the installability host
    /// probe. `None` when no installability check ran (the fresh
    /// path, and constraint-free frozen lockfiles).
    pub host_node: Option<&'a crate::materialization_plan::HostNode>,
    pub supported_architectures: Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    pub materialization: HoistedMaterialization<'a>,
}

/// What the linker materializes a package directory with: the
/// install-scoped import state every import reports through, and the
/// reuse inputs that let a directory be cloned from its canonical slot
/// instead of imported file by file.
pub struct HoistedMaterialization<'a> {
    pub logged_methods: &'a AtomicU8,
    pub requester: &'a str,
    /// Prefetched build flags gate reuse of canonical package directories.
    pub requires_build_by_snapshot: Option<&'a crate::RequiresBuildBySnapshot>,
    pub dir_clone_cache: Option<&'a crate::DirCloneCache<'a>>,
}

/// Error type of [`run_hoisted_linker`]. Each install path maps these
/// back onto its own error enum's matching variant so the user-facing
/// error code is identical regardless of which path drove the hoist.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum HoistedLinkerError {
    #[diagnostic(transparent)]
    HoistedDepGraph(#[error(source)] HoistedDepGraphError),
    #[diagnostic(transparent)]
    LinkHoistedModules(#[error(source)] LinkHoistedModulesError),
    #[diagnostic(transparent)]
    SymlinkDirectDependencies(#[error(source)] SymlinkDirectDependenciesError),
    #[display("failed to write package map: {_0}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_WRITE_PACKAGE_MAP))]
    WritePackageMap(#[error(source)] crate::WritePackageMapError),
}

/// Materialize the `nodeLinker: hoisted` on-disk tree from a lockfile.
///
/// Runs the [`crate::lockfile_to_hoisted_dep_graph`] walker over the
/// lockfile's snapshots, materializes the resulting graph with
/// [`crate::link_hoisted_modules()`] (real directories under each
/// importer's tree, fed from `cas_paths_by_pkg_id`), then layers
/// [`crate::SymlinkDirectDependencies`] with `link_only: true` to wire
/// `workspace:` / `link:` deps the hoist walker skips. Folds the
/// walker's newly-discovered installability skips into `skipped`.
///
/// Shared by both install paths so the hoisted layout, skip-set
/// accounting, and `pkg_roots_by_key` derivation stay identical.
pub fn run_hoisted_linker<Reporter: self::Reporter>(
    inputs: &HoistedLinkerInputs<'_>,
    skipped: &mut SkippedSnapshots,
) -> Result<HoistedLinkerOutput, HoistedLinkerError> {
    let lockfile = included_lockfile(inputs);
    let build_present = inputs.prior.build_present_packages;
    let unbuilt = inputs.prior.unbuilt_builds;
    let walked = walk_hoisted_graph(inputs, &lockfile, skipped)?;
    link_hoisted::<Reporter>(inputs, &lockfile, &walked, skipped)?;
    // A present package leaves the build set unless everything is being
    // rebuilt or the previous install left it unbuilt (ignored or
    // pending): that one is judged by the build policy again, as it
    // would be on an install that imported it.
    let pkg_roots = pkg_roots_by_key(
        walked.graph
            .values()
            .filter(|node| build_present || !node.present || recorded_unbuilt(unbuilt, node)),
    );
    // Several nodes can share one snapshot (a package nested under more
    // than one consumer); the roots map has already collapsed them, so
    // the build set is its keys. Sorted because a `HashMap` hands them
    // over in no particular order and `pendingBuilds` is written from
    // this list.
    let mut build_snapshots: Vec<PackageKey> = pkg_roots.keys().cloned().collect();
    build_snapshots.sort_by_cached_key(ToString::to_string);
    Ok(HoistedLinkerOutput {
        hoisted_pkg_roots_by_key: Some(pkg_roots),
        hoisted_build_snapshots: Some(build_snapshots),
        hoisted_locations: walked.hoisted_locations,
    })
}

/// The hoist tree seeds from every importer dep map, so groups the user
/// excluded (`--prod`, `--dev`, `--no-optional`) must be cleared from
/// the lockfile before the walk — otherwise their whole subgraph
/// materializes as real directories. Mirrors pnpm, which hands its
/// hoisted walker an include-filtered lockfile.
fn included_lockfile<'l>(inputs: &HoistedLinkerInputs<'l>) -> std::borrow::Cow<'l, Lockfile> {
    let included = IncludedDependencies {
        dependencies: inputs.projects.dependency_groups.contains(&DependencyGroup::Prod),
        dev_dependencies: inputs.projects.dependency_groups.contains(&DependencyGroup::Dev),
        optional_dependencies: inputs.projects.dependency_groups.contains(
            &DependencyGroup::Optional,
        ),
    };
    if included.dependencies && included.dev_dependencies && included.optional_dependencies {
        std::borrow::Cow::Borrowed(inputs.graph.lockfile)
    } else {
        std::borrow::Cow::Owned(exclude_importer_groups(inputs.graph.lockfile, included))
    }
}

/// Walker installability inputs come straight from the optional
/// `host_node` the caller built for the `compute_skipped_snapshots`
/// pass. When `host_node` is `None` no per-snapshot constraint exists,
/// so the host triple values pass through as defaults the walker won't
/// actually consult.
///
/// Augments the live skip set with the walker's *new* skips only —
/// entries already in the input `SkippedSnapshots` each live in their
/// proper subset (installability / fetch-failed / optional-excluded).
/// Re-inserting them as installability would promote transient
/// `fetch_failed` / `optional_excluded` entries into the
/// persisted-on-disk `.modules.yaml.skipped` set, which would survive
/// into the next install — exactly the contract those subsets exist to
/// prevent. Diffing against the input set keeps the persistence
/// boundary intact: only walker-discovered installability skips
/// (optional + unsupported platform) flow into
/// [`SkippedSnapshots::insert_installability`].
fn walk_hoisted_graph(
    inputs: &HoistedLinkerInputs<'_>,
    lockfile: &Lockfile,
    skipped: &mut SkippedSnapshots,
) -> Result<crate::hoisted_dep_graph::LockfileToDepGraphResult, HoistedLinkerError> {
    let config = inputs.config;
    let walker_skipped: BTreeSet<String> = skipped
        .iter()
        .map(std::string::ToString::to_string)
        .collect();
    let walker_opts = LockfileToHoistedDepGraphOptions {
        installability: crate::HoistedInstallability {
            engine_strict: config.engine_strict,
            current_node_version: inputs.host_node
                .map(|host| host.version.clone())
                .unwrap_or_default(),
            current_os: pnpm_graph_hasher::host_platform().to_string(),
            current_cpu: pnpm_graph_hasher::host_arch().to_string(),
            current_libc: pnpm_graph_hasher::host_libc().to_string(),
            supported_architectures: inputs.supported_architectures.cloned(),
        },
        placement: crate::HoistedPlacementOptions {
            auto_install_peers: config.auto_install_peers,
            hoist_workspace_packages: config.hoist_workspace_packages,
            hoisting_limits: crate::get_hoisting_limits(
                &lockfile.importers,
                config.hoisting_limits,
            ),
            external_dependencies: config.external_dependencies.clone(),
        },
        lockfile_dir: inputs.projects.walker_lockfile_dir.to_path_buf(),

        skipped: walker_skipped.clone(),
        force: config.force,
        // Matches the `engineStrict` policy `compute_skipped_snapshots`
        // used upthread (both read `config.engine_strict`): an engine
        // mismatch on a required package is a hard error under strict,
        // otherwise a skip-optional / warning.
        current_hoisted_locations: inputs.prior.current_hoisted_locations,
    };
    let walked =
        lockfile_to_hoisted_dep_graph(lockfile, inputs.prior.current_lockfile, &walker_opts)
            .map_err(HoistedLinkerError::HoistedDepGraph)?;
    for skipped_dep_path in walked.skipped.difference(&walker_skipped) {
        if let Ok(key) = skipped_dep_path.parse::<PackageKey>() {
            skipped.insert_installability(key);
        }
    }
    Ok(walked)
}

fn link_hoisted<Reporter: self::Reporter>(
    inputs: &HoistedLinkerInputs<'_>,
    lockfile: &Lockfile,
    walked: &crate::hoisted_dep_graph::LockfileToDepGraphResult,
    skipped: &SkippedSnapshots,
) -> Result<(), HoistedLinkerError> {
    let config = inputs.config;
    // Empty CAS index → linker would refuse every non-optional node.
    // Only happens when the install has no snapshots, in which case
    // the linker is a no-op.
    let cas_index = inputs.graph.cas_paths_by_pkg_id
        .as_ref()
        .expect("hoisted CreateVirtualStore populates cas_paths");
    let link_options = crate::shim_link_options(config, NodeLinker::Hoisted);
    let dir_clone_cache = crate::link_hoisted_modules::HoistedDirCloneCache::new(
        inputs.materialization.dir_clone_cache,
        lockfile.packages.as_ref(),
        inputs.prior.current_lockfile.and_then(|lockfile| lockfile.packages.as_ref()),
        inputs.materialization.requires_build_by_snapshot,
        config.force,
    );
    link_hoisted_modules::<Reporter>(&LinkHoistedModulesOpts {
        import: crate::PackageImportOptions {
            method: config.package_import_method,
            patterns: &config.package_import_patterns,
            logged_methods: inputs.materialization.logged_methods,
            requester: inputs.materialization.requester,
        },
        graph: &walked.graph,
        prev_graph: walked.prev_graph.as_ref(),
        hierarchy: &walked.hierarchy,
        cas_paths_by_pkg_id: cas_index,

        confine_root: inputs.projects.walker_lockfile_dir,
        link_options: &link_options,
        dir_clone_cache: dir_clone_cache.as_ref(),
    })
    .map_err(HoistedLinkerError::LinkHoistedModules)?;
    link_selected_hoisted_direct_dependencies(
        config,
        inputs.projects.walker_lockfile_dir,
        inputs.projects.manifests,
        &walked.direct_dependencies_by_importer_id,
    )?;
    update_hoisted_package_map(inputs, lockfile, walked)?;
    link_hoisted_workspace_dependencies::<Reporter>(inputs, lockfile, skipped, &link_options)?;
    // The pass above links `link:` siblings only, so it reports only
    // those. The rest are real directories this linker wrote.
    crate::report_direct_dependency_changes::report_direct_dependency_changes::<Reporter>(
        inputs, lockfile, skipped,
    );
    Ok(())
}

fn update_hoisted_package_map(
    inputs: &HoistedLinkerInputs<'_>,
    lockfile: &Lockfile,
    walked: &crate::hoisted_dep_graph::LockfileToDepGraphResult,
) -> Result<(), HoistedLinkerError> {
    let config = inputs.config;
    if crate::should_write_hoisted_package_map(config) {
        crate::package_map::write_hoisted_package_map(
            lockfile,
            walked,
            &crate::package_map::HoistedPackageMapOptions {
                lockfile_dir: inputs.projects.walker_lockfile_dir,
                modules_dir: &config.modules_dir,
                package_map_type: config.node_package_map_type,
                project_manifests: inputs.projects.package_map_manifests,
            },
        )
        .map_err(HoistedLinkerError::WritePackageMap)?;
    } else {
        crate::package_map::remove_package_map(&config.modules_dir);
    }
    Ok(())
}

// The hoisted walker skips workspace links, which still need symlinks in each importer.
fn link_hoisted_workspace_dependencies<Reporter: self::Reporter>(
    inputs: &HoistedLinkerInputs<'_>,
    lockfile: &Lockfile,
    skipped: &SkippedSnapshots,
    link_options: &pnpm_cmd_shim::LinkBinsOptions,
) -> Result<(), HoistedLinkerError> {
    let config = inputs.config;
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
    }
    .run::<Reporter>()
    .map_err(HoistedLinkerError::SymlinkDirectDependencies)
}

/// Whether the previous install recorded `node` as not built.
/// `ignoredBuilds` and `pendingBuilds` hold `name@version` keys; the dep
/// path is checked too for the entries written with a peer suffix.
fn recorded_unbuilt(unbuilt: &crate::UnbuiltBuilds, node: &crate::DependenciesGraphNode) -> bool {
    !unbuilt.is_empty()
        && (unbuilt.contains(&format!("{}@{}", node.package.name, node.package.version))
            || unbuilt.contains(node.package.dep_path.as_str()))
}

/// Map snapshot key → every recorded directory, in walker order. The
/// walker emits multiple [`crate::DependenciesGraphNode`]s with the
/// same `dep_path` when the package nests under a sibling (version
/// conflict). Postinstall scripts and the side-effects-cache key both
/// depend only on the package contents (identical across locations),
/// so `BuildModules` runs those once at the head of the list; patch
/// application and cache-overlay re-imports walk the whole list.
fn pkg_roots_by_key<'n>(
    nodes: impl Iterator<Item = &'n crate::DependenciesGraphNode>,
) -> HashMap<PackageKey, Vec<std::path::PathBuf>> {
    let mut roots: HashMap<PackageKey, Vec<std::path::PathBuf>> = HashMap::new();
    for node in nodes {
        if let Ok(key) = node.package.dep_path.as_str().parse::<PackageKey>() {
            roots
                .entry(key)
                .or_default()
                .push(node.dir.clone());
        }
    }
    roots
}

pub(crate) fn link_selected_hoisted_direct_dependencies(
    config: &Config,
    lockfile_dir: &Path,
    project_manifests: &[(PathBuf, &pnpm_package_manifest::PackageManifest)],
    direct_dependencies_by_importer_id: &crate::DirectDependenciesByImporterId,
) -> Result<(), HoistedLinkerError> {
    let modules_dir_name =
        config.modules_dir.file_name().unwrap_or_else(|| OsStr::new("node_modules"));
    let root_modules_dir = pnpm_fs::lexical_normalize(&lockfile_dir.join(modules_dir_name));
    let link_options = crate::shim_link_options(config, NodeLinker::Hoisted);
    for (project_dir, _) in project_manifests {
        let scope = HoistedLinkScope {
            importer_id: pnpm_workspace::importer_id_from_root_dir(lockfile_dir, project_dir),
            root_modules_dir: &root_modules_dir,
            modules_dir: project_dir.join(modules_dir_name),
            // The workspace root owns the hoisted slot itself, so its
            // own entries are the real directories rather than links to
            // them.
            is_workspace_root: pnpm_fs::lexical_normalize(project_dir)
                == pnpm_fs::lexical_normalize(lockfile_dir),
        };
        scope.link_direct_dependencies(direct_dependencies_by_importer_id, &link_options)?;
    }
    Ok(())
}

/// One importer's share of the hoisted direct-dependency linking.
struct HoistedLinkScope<'a> {
    importer_id: String,
    /// The workspace root's `node_modules`, where the hoister put the
    /// dependency copy every project reaches by walking up.
    root_modules_dir: &'a Path,
    modules_dir: PathBuf,
    is_workspace_root: bool,
}

impl HoistedLinkScope<'_> {
    fn link_direct_dependencies(
        &self,
        direct_dependencies_by_importer_id: &crate::DirectDependenciesByImporterId,
        link_options: &pnpm_cmd_shim::LinkBinsOptions,
    ) -> Result<(), HoistedLinkerError> {
        let Some(direct_dependencies) = direct_dependencies_by_importer_id.get(&self.importer_id)
        else {
            return Ok(());
        };
        let mut linked_names = Vec::new();
        for (alias, target) in direct_dependencies {
            if self.link_one(alias, target)? {
                linked_names.push(alias.clone());
            }
        }
        crate::link_direct_dep_bins(&self.modules_dir, &linked_names, link_options)
            .map_err(|source| {
                HoistedLinkerError::SymlinkDirectDependencies(
                    SymlinkDirectDependenciesError::LinkBins(source),
                )
            })
    }

    /// `Ok(true)` when the alias now resolves inside the project's own
    /// `node_modules`, so its bins are this importer's to link.
    fn link_one(&self, alias: &str, target: &Path) -> Result<bool, HoistedLinkerError> {
        let link_path =
            crate::safe_join_modules_dir::safe_join_modules_dir(&self.modules_dir, alias)
                .map_err(|source| {
                    self.symlink_failure(alias, SymlinkPackageError::InvalidAlias(source))
                })?;
        // A dependency that won the workspace-root slot is reached by
        // walking up from the project, exactly as it is under pnpm.
        // Repeating it inside the project would give a build a second
        // copy to run lifecycle scripts in. Checked after `link_path` so
        // an unusable alias still reports itself.
        if !self.is_workspace_root
            && pnpm_fs::lexical_normalize(target)
                == pnpm_fs::lexical_normalize(&self.root_modules_dir.join(alias))
        {
            self.remove_root_shadow(alias, target, &link_path)?;
            return Ok(false);
        }
        if pnpm_fs::lexical_normalize(&link_path) == pnpm_fs::lexical_normalize(target) {
            return Ok(true);
        }
        crate::symlink_package(target, &link_path)
            .map_err(|source| self.symlink_failure(alias, source))?;
        Ok(true)
    }

    /// An install that predates the walk-up rule, or one where the
    /// version had lost the root slot, leaves a link shadowing the root
    /// copy — the duplicate the rule exists to avoid. A real directory
    /// is the pruner's to remove, and only ever belongs to a version
    /// that lost the slot.
    fn remove_root_shadow(
        &self,
        alias: &str,
        target: &Path,
        link_path: &Path,
    ) -> Result<(), HoistedLinkerError> {
        // `is_symlink_or_junction`, not `Path::is_symlink`: on Windows
        // `symlink_dir` falls back to a junction when it cannot create a
        // true symlink, and a junction is not a symlink to the stdlib.
        let stale_link = match pnpm_fs::is_symlink_or_junction(link_path) {
            Ok(is_link) => is_link,
            // Nothing to clean up — the common case, and the one
            // `junction::exists` reports as an error rather than
            // `Ok(false)`.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(self.symlink_dir_failure(alias, target, link_path, error)),
        };
        if !stale_link {
            return Ok(());
        }
        pnpm_fs::remove_symlink_dir(link_path)
            .map_err(|error| self.symlink_dir_failure(alias, target, link_path, error))
    }

    fn symlink_failure(&self, alias: &str, source: SymlinkPackageError) -> HoistedLinkerError {
        HoistedLinkerError::SymlinkDirectDependencies(
            SymlinkDirectDependenciesError::SymlinkPackage {
                importer_id: self.importer_id.clone(),
                name: alias.to_owned(),
                source,
            },
        )
    }

    fn symlink_dir_failure(
        &self,
        alias: &str,
        target: &Path,
        link_path: &Path,
        error: std::io::Error,
    ) -> HoistedLinkerError {
        self.symlink_failure(
            alias,
            SymlinkPackageError::SymlinkDir {
                symlink_target: target.to_path_buf(),
                symlink_path: link_path.to_path_buf(),
                error,
            },
        )
    }
}

/// Clone the lockfile with every importer's excluded dep groups
/// cleared, so seeds for the hoist tree come only from the included
/// groups. Snapshots that thereby become unreachable are simply never
/// visited by the hoister, so the snapshot/package maps stay as-is.
pub(crate) fn exclude_importer_groups(
    lockfile: &Lockfile,
    included: IncludedDependencies,
) -> Lockfile {
    let mut filtered = lockfile.clone();
    for importer in filtered.importers.values_mut() {
        if !included.dependencies {
            importer.dependencies = None;
        }
        if !included.dev_dependencies {
            importer.dev_dependencies = None;
        }
        if !included.optional_dependencies {
            importer.optional_dependencies = None;
        }
    }
    filtered
}
