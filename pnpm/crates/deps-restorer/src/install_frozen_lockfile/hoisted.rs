//! The hoisted node-linker: a flat `node_modules` instead of a virtual store.

pub(crate) use direct_links::link_selected_hoisted_direct_dependencies;
pub use hoist_plan::{
    HoistPlan, HoistedWorkspacePackages, collect_public_hoist_targets, compute_hoist_plan,
    find_own_runtime_node_major, find_runtime_node_key, find_runtime_node_major,
    parse_major_from_version, workspace_packages_for_hoist,
};

mod direct_links;
mod hoist_plan;

use super::{
    AtomicU8, BTreeMap, BTreeSet, Config, DependencyGroup, Diagnostic, Display, Error, HashMap,
    HoistedDepGraphError, IncludedDependencies, LinkHoistedModulesError, LinkHoistedModulesOpts,
    Lockfile, LockfileToHoistedDepGraphOptions, NodeLinker, PackageKey, Path, PathBuf, Reporter,
    SkippedSnapshots, SymlinkDirectDependencies, SymlinkDirectDependenciesError,
    SymlinkPackageError, create_matcher, link_hoisted_modules, lockfile_to_hoisted_dep_graph,
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
    /// See [`crate::link_hoisted_modules()`]'s return value.
    pub held_back_bins_dirs: Vec<crate::HeldBackBinsDir>,
    /// The workspace projects `hoist-workspace-packages` linked into the
    /// root `node_modules`, for `.modules.yaml.hoistedDependencies`.
    pub hoisted_dependencies: crate::HoistedDependencies,
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
    #[diagnostic(transparent)]
    PruneWorkspaceHoists(#[error(source)] crate::PruneDirectDepsError),
    #[diagnostic(transparent)]
    HoistSymlink(#[error(source)] SymlinkPackageError),
    #[diagnostic(transparent)]
    HoistLinkBins(#[error(source)] pnpm_cmd_shim::LinkBinsError),
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
    unlink_prior_workspace_hoists(inputs)?;
    let (held_back_bins_dirs, hoisted_dependencies) =
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
        held_back_bins_dirs,
        hoisted_dependencies,
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
    let walker_skipped: BTreeSet<String> = skipped
        .iter()
        .map(std::string::ToString::to_string)
        .collect();
    let walker_opts = hoisted_walker_options(inputs, lockfile, walker_skipped.clone());
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

fn hoisted_walker_options<'a>(
    inputs: &HoistedLinkerInputs<'a>,
    lockfile: &Lockfile,
    walker_skipped: BTreeSet<String>,
) -> LockfileToHoistedDepGraphOptions<'a> {
    let config = inputs.config;
    LockfileToHoistedDepGraphOptions {
        installability: crate::HoistedInstallability {
            engine_strict: config.effective_engine_strict(),
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
        root_modules_dir: config.modules_dir.clone(),

        skipped: walker_skipped,
        force: config.force,
        include_incompatible_packages: config.installs_incompatible_packages(),
        current_hoisted_locations: inputs.prior.current_hoisted_locations,
    }
}

fn link_hoisted<Reporter: self::Reporter>(
    inputs: &HoistedLinkerInputs<'_>,
    lockfile: &Lockfile,
    walked: &crate::hoisted_dep_graph::LockfileToDepGraphResult,
    skipped: &SkippedSnapshots,
) -> Result<(Vec<crate::HeldBackBinsDir>, crate::HoistedDependencies), HoistedLinkerError> {
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
    let held_back_bins_dirs = link_hoisted_modules::<Reporter>(&LinkHoistedModulesOpts {
        import: crate::PackageImportOptions {
            method: config.package_import_method,
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
    let hoisted_dependencies = link_hoisted_workspace_packages(inputs, walked, &link_options)?;
    link_hoisted_workspace_dependencies::<Reporter>(inputs, lockfile, skipped, &link_options)?;
    // The pass above links `link:` siblings only, so it reports only
    // those. The rest are real directories this linker wrote.
    crate::report_direct_dependency_changes::report_direct_dependency_changes::<Reporter>(
        inputs, lockfile, skipped,
    );
    Ok((held_back_bins_dirs, hoisted_dependencies))
}

/// Runs before the linker writes the tree, so no package is imported at
/// a path that is still a project's link.
fn unlink_prior_workspace_hoists(
    inputs: &HoistedLinkerInputs<'_>,
) -> Result<(), HoistedLinkerError> {
    let modules_dir = &inputs.config.modules_dir;
    crate::prune_stale_modules::prune_workspace_hoists(
        &crate::prune_stale_modules::WorkspaceHoistDirs {
            workspace_root: inputs.projects.walker_lockfile_dir,
            private: modules_dir,
            public: modules_dir,
        },
        inputs.prior.current_lockfile.unwrap_or(inputs.graph.lockfile),
        inputs.prior.hoisted_dependencies,
        None,
    )
    .map_err(HoistedLinkerError::PruneWorkspaceHoists)
}

/// `hoist-workspace-packages` under this linker: every package lives in
/// the root `node_modules`, so a project either hoist pattern selects is
/// linked there.
fn link_hoisted_workspace_packages(
    inputs: &HoistedLinkerInputs<'_>,
    walked: &crate::hoisted_dep_graph::LockfileToDepGraphResult,
    link_options: &pnpm_cmd_shim::LinkBinsOptions,
) -> Result<crate::HoistedDependencies, HoistedLinkerError> {
    let config = inputs.config;
    if !config.hoist_workspace_packages {
        return Ok(crate::HoistedDependencies::new());
    }
    let workspace_packages = workspace_packages_for_hoist(
        inputs.projects.walker_lockfile_dir,
        inputs.projects.manifests,
    );
    let root_links = root_dependency_aliases(inputs);
    let hoists = crate::hoist_workspace_packages_to_root(
        &workspace_packages,
        walked.direct_dependencies_by_importer_id
            .get(".")
            .into_iter()
            .flat_map(|root_deps| root_deps.keys().map(String::as_str))
            .chain(root_links.iter().map(String::as_str)),
        &create_matcher(
            config.hoist_pattern
                .as_deref()
                .unwrap_or(&[]),
        ),
        &create_matcher(
            config.public_hoist_pattern
                .as_deref()
                .unwrap_or(&[]),
        ),
    );
    link_workspace_hoists(inputs, hoists.aliases, link_options)?;
    Ok(hoists.hoisted_dependencies)
}

/// The aliases of the root project's dependencies. Its `link:`
/// dependencies are not in the root hierarchy and take their names after
/// the workspace pass, so they must be reserved before it.
fn root_dependency_aliases(inputs: &HoistedLinkerInputs<'_>) -> Vec<String> {
    inputs.projects.importers
        .get(Lockfile::ROOT_IMPORTER_KEY)
        .into_iter()
        .flat_map(|root| {
            root.dependencies_by_groups(inputs.projects.dependency_groups.iter().copied())
        })
        .map(|(alias, _)| alias.to_string())
        .collect()
}

/// Symlink each placed project into the root `node_modules` and shim the
/// bins no hoisted package already provides into the root `.bin`.
fn link_workspace_hoists(
    inputs: &HoistedLinkerInputs<'_>,
    aliases: Vec<(String, pnpm_modules_yaml::HoistKind, PathBuf)>,
    link_options: &pnpm_cmd_shim::LinkBinsOptions,
) -> Result<(), HoistedLinkerError> {
    let modules_dir = &inputs.config.modules_dir;
    crate::symlink_hoisted_dependencies(
        &HashMap::new(),
        &aliases,
        &HashMap::new(),
        inputs.graph.layout,
        modules_dir,
        modules_dir,
        &std::collections::HashSet::new(),
    )
    .map_err(HoistedLinkerError::HoistSymlink)?;
    let project_dirs: Vec<PathBuf> = aliases
        .into_iter()
        .map(|(_, _, project_dir)| project_dir)
        .collect();
    crate::link_new_bins_from_locations(modules_dir, &project_dirs, link_options)
        .map_err(HoistedLinkerError::HoistLinkBins)
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
        scheduled_builds: None,
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
