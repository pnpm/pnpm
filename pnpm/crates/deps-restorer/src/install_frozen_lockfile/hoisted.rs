//! The hoisted node-linker: a flat `node_modules` instead of a virtual store.

use super::{
    AtomicU8, BTreeMap, BTreeSet, Config, DependencyGroup, Diagnostic, Display, Error, HashMap,
    HashSet, HoistedDepGraphError, IncludedDependencies, LinkHoistedModulesError,
    LinkHoistedModulesOpts, Lockfile, LockfileToHoistedDepGraphOptions, NodeLinker, OsStr,
    PackageKey, PackageMetadata, Path, PathBuf, Prefix, ProjectSnapshot, Reporter,
    SkippedSnapshots, SnapshotEntry, SymlinkDirectDependencies, SymlinkDirectDependenciesError,
    SymlinkPackageError, VirtualStoreLayout, build_direct_deps_by_importer, create_matcher,
    get_hoisted_dependencies, link_hoisted_modules, lockfile_to_hoisted_dep_graph,
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
    /// [`crate::BuildModules::pkg_roots_by_key`] for how the list is
    /// consumed.
    pub hoisted_pkg_roots_by_key: Option<HashMap<PackageKey, Vec<std::path::PathBuf>>>,
}

/// Inputs to [`run_hoisted_linker`]. Bundled so the two install
/// paths ([`crate::install_frozen_lockfile::InstallFrozenLockfile`] and `InstallWithFreshLockfile`)
/// can feed the shared hoisted-linker materialization without a
/// long positional argument list. The frozen path passes the
/// loaded `pnpm-lock.yaml`; the fresh path passes the freshly-built
/// lockfile and `current_lockfile: None`.
pub struct HoistedLinkerInputs<'a> {
    pub config: &'static Config,
    /// Lockfile the walker reads `snapshots:` / `packages:` /
    /// `importers:` from. `&built_lockfile` on the fresh path,
    /// the loaded wanted lockfile on the frozen path.
    pub lockfile: &'a Lockfile,
    /// Previous install's `<virtual_store_dir>/lock.yaml`, used by the
    /// walker to diff orphans. `None` on the fresh path (no analogue
    /// yet).
    pub current_lockfile: Option<&'a Lockfile>,
    pub layout: &'a VirtualStoreLayout,
    pub importers: &'a HashMap<String, ProjectSnapshot>,
    pub dependency_groups: &'a [DependencyGroup],
    /// Selected project anchors whose direct dependencies and workspace
    /// links are written by this filtered run.
    pub project_manifests: &'a [(PathBuf, &'a pnpm_package_manifest::PackageManifest)],
    /// Every real importer manifest represented in the full hoisted graph.
    /// The shared package map needs all project names for self-reference
    /// entries even though direct links are limited to selected anchors.
    pub package_map_project_manifests:
        &'a [(PathBuf, &'a pnpm_package_manifest::PackageManifest)],
    /// Lockfile root the walker resolves hoisted directories against.
    pub walker_lockfile_dir: &'a Path,
    /// Anchor for [`crate::SymlinkDirectDependencies`]'s per-importer
    /// `node_modules` lookup. Equals `walker_lockfile_dir` on the
    /// frozen path; the fresh path passes `config.modules_dir.parent()`
    /// so relocated `modules_dir` test configs land symlinks where the
    /// rest of the install writes.
    pub symlink_workspace_root: &'a Path,
    /// `(node_detected, node_version)` from the installability host
    /// probe. `None` when no installability check ran (the fresh
    /// path, and constraint-free frozen lockfiles).
    pub host_node: Option<&'a crate::materialization_plan::HostNode>,
    pub supported_architectures: Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    /// Per-package CAS index produced by [`crate::CreateVirtualStore`]
    /// under `node_linker == Hoisted`. The linker imports files from
    /// these paths into the on-disk hoisted tree.
    pub cas_paths_by_pkg_id: Option<crate::CasPathsByPkgId>,
    pub logged_methods: &'a AtomicU8,
    pub requester: &'a str,
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
    inputs: HoistedLinkerInputs<'_>,
    skipped: &mut SkippedSnapshots,
) -> Result<HoistedLinkerOutput, HoistedLinkerError> {
    let HoistedLinkerInputs {
        config,
        lockfile,
        current_lockfile,
        layout,
        importers,
        dependency_groups,
        project_manifests,
        package_map_project_manifests,
        walker_lockfile_dir,
        symlink_workspace_root,
        host_node,
        supported_architectures,
        cas_paths_by_pkg_id,
        logged_methods,
        requester,
    } = inputs;

    // The hoist tree seeds from every importer dep map, so groups the
    // user excluded (`--prod`, `--dev`, `--no-optional`) must be cleared
    // from the lockfile before the walk — otherwise their whole subgraph
    // materializes as real directories. Mirrors pnpm, which hands its
    // hoisted walker an include-filtered lockfile.
    let included = IncludedDependencies {
        dependencies: dependency_groups.contains(&DependencyGroup::Prod),
        dev_dependencies: dependency_groups.contains(&DependencyGroup::Dev),
        optional_dependencies: dependency_groups.contains(&DependencyGroup::Optional),
    };
    let filtered_lockfile;
    let lockfile =
        if included.dependencies && included.dev_dependencies && included.optional_dependencies {
            lockfile
        } else {
            filtered_lockfile = exclude_importer_groups(lockfile, included);
            &filtered_lockfile
        };

    // Walker installability inputs come straight from the optional
    // `host_node` the caller built for the `compute_skipped_snapshots`
    // pass. When `host_node` is `None` no per-snapshot constraint
    // exists, so the host triple values pass through as defaults the
    // walker won't actually consult.
    let walker_skipped: BTreeSet<String> =
        skipped.iter().map(std::string::ToString::to_string).collect();
    let walker_opts = LockfileToHoistedDepGraphOptions {
        lockfile_dir: walker_lockfile_dir.to_path_buf(),
        auto_install_peers: config.auto_install_peers,
        skipped: walker_skipped.clone(),
        force: config.force,
        // Matches the `engineStrict` policy `compute_skipped_snapshots`
        // used upthread (both read `config.engine_strict`): an engine
        // mismatch on a required package is a hard error under strict,
        // otherwise a skip-optional / warning.
        engine_strict: config.engine_strict,
        current_node_version: host_node.map(|host| host.version.clone()).unwrap_or_default(),
        current_os: pnpm_graph_hasher::host_platform().to_string(),
        current_cpu: pnpm_graph_hasher::host_arch().to_string(),
        current_libc: pnpm_graph_hasher::host_libc().to_string(),
        supported_architectures: supported_architectures.cloned(),
        hoist_workspace_packages: config.hoist_workspace_packages,
        hoisting_limits: crate::get_hoisting_limits(&lockfile.importers, config.hoisting_limits),
        external_dependencies: config.external_dependencies.clone(),
    };
    let walker_result = lockfile_to_hoisted_dep_graph(lockfile, current_lockfile, &walker_opts)
        .map_err(HoistedLinkerError::HoistedDepGraph)?;
    // Augment the live skip set with the walker's *new* skips only —
    // entries already in `walker_skipped` came from the input
    // `SkippedSnapshots`, where each one already lives in its proper
    // subset (installability / fetch-failed / optional-excluded).
    // Re-inserting them as installability would promote transient
    // `fetch_failed` / `optional_excluded` entries into the
    // persisted-on-disk `.modules.yaml.skipped` set, which would
    // survive into the next install — exactly the contract those
    // subsets exist to prevent. Diffing against the input set keeps
    // the persistence boundary intact: only walker-discovered
    // installability skips (optional + unsupported platform) flow
    // into [`SkippedSnapshots::insert_installability`].
    for skipped_dep_path in walker_result.skipped.difference(&walker_skipped) {
        if let Ok(key) = skipped_dep_path.parse::<PackageKey>() {
            skipped.insert_installability(key);
        }
    }
    // Empty CAS index → linker would refuse every non-optional node.
    // Only happens when the install has no snapshots, in which case
    // the linker is a no-op.
    let cas_index = cas_paths_by_pkg_id.expect("hoisted CreateVirtualStore populates cas_paths");
    let link_options = crate::shim_link_options(config, NodeLinker::Hoisted);
    let link_opts = LinkHoistedModulesOpts {
        graph: &walker_result.graph,
        prev_graph: walker_result.prev_graph.as_ref(),
        hierarchy: &walker_result.hierarchy,
        cas_paths_by_pkg_id: &cas_index,
        import_method: config.package_import_method,
        logged_methods,
        requester,
        confine_root: walker_lockfile_dir,
        link_options: &link_options,
    };
    link_hoisted_modules::<Reporter>(&link_opts).map_err(HoistedLinkerError::LinkHoistedModules)?;
    link_selected_hoisted_direct_dependencies(
        config,
        walker_lockfile_dir,
        project_manifests,
        &walker_result.direct_dependencies_by_importer_id,
    )?;
    crate::package_map::write_hoisted_package_map(
        lockfile,
        &walker_result,
        &crate::package_map::HoistedPackageMapOptions {
            lockfile_dir: walker_lockfile_dir,
            modules_dir: &config.modules_dir,
            package_map_type: config.node_package_map_type,
            project_manifests: package_map_project_manifests,
        },
    )
    .map_err(HoistedLinkerError::WritePackageMap)?;
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
    let trusted_importer_ids: std::collections::HashSet<String> = project_manifests
        .iter()
        .map(|(project_dir, _)| {
            pnpm_workspace::importer_id_from_root_dir(walker_lockfile_dir, project_dir)
        })
        .collect();
    SymlinkDirectDependencies {
        config,
        layout,
        importers,
        packages: lockfile.packages.as_ref(),
        dependency_groups: dependency_groups.iter().copied(),
        workspace_root: symlink_workspace_root,
        skipped: &*skipped,
        link_only: true,
        // Hoisted-linker path has no public-hoist virtual store to
        // dedupe against; the real-directory tree is the hoist layout.
        public_hoist_targets: None,
        trusted_importer_ids: Some(&trusted_importer_ids),
        // pnpm gates `extraNodePaths` on the isolated linker, so the
        // hoisted linker's shims never carry `NODE_PATH`.
        link_options: &link_options,
    }
    .run::<Reporter>()
    .map_err(HoistedLinkerError::SymlinkDirectDependencies)?;
    // Map snapshot key → every recorded directory, in walker order. The
    // walker emits multiple [`crate::DependenciesGraphNode`]s with the
    // same `dep_path` when the package nests under a sibling (version
    // conflict). Postinstall scripts and the side-effects-cache key both
    // depend only on the package contents (identical across locations),
    // so `BuildModules` runs those once at the head of the list; patch
    // application and cache-overlay re-imports walk the whole list.
    let mut pkg_roots_by_key: HashMap<PackageKey, Vec<std::path::PathBuf>> = HashMap::new();
    for node in walker_result.graph.values() {
        if let Ok(key) = node.dep_path.as_str().parse::<PackageKey>() {
            pkg_roots_by_key.entry(key).or_default().push(node.dir.clone());
        }
    }
    Ok(HoistedLinkerOutput {
        hoisted_locations: walker_result.hoisted_locations,
        hoisted_pkg_roots_by_key: Some(pkg_roots_by_key),
    })
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
        let importer_id = pnpm_workspace::importer_id_from_root_dir(lockfile_dir, project_dir);
        let Some(direct_dependencies) = direct_dependencies_by_importer_id.get(&importer_id) else {
            continue;
        };
        let modules_dir = project_dir.join(modules_dir_name);
        // The workspace root owns the hoisted slot itself, so its own
        // entries are the real directories rather than links to them.
        let is_workspace_root =
            pnpm_fs::lexical_normalize(project_dir) == pnpm_fs::lexical_normalize(lockfile_dir);
        let mut linked_names = Vec::new();
        for (alias, target) in direct_dependencies {
            let link_path =
                crate::safe_join_modules_dir::safe_join_modules_dir(&modules_dir, alias).map_err(
                    |source| {
                        HoistedLinkerError::SymlinkDirectDependencies(
                            SymlinkDirectDependenciesError::SymlinkPackage {
                                importer_id: importer_id.clone(),
                                name: alias.clone(),
                                source: SymlinkPackageError::InvalidAlias(source),
                            },
                        )
                    },
                )?;
            // A dependency that won the workspace-root slot is reached by
            // walking up from the project, exactly as it is under pnpm.
            // Repeating it inside the project would give a build a second
            // copy to run lifecycle scripts in. Checked after `link_path`
            // so an unusable alias still reports itself.
            if !is_workspace_root
                && pnpm_fs::lexical_normalize(target)
                    == pnpm_fs::lexical_normalize(&root_modules_dir.join(alias))
            {
                // An install that predates this rule, or one where the
                // version had lost the slot, leaves a link here. Left in
                // place it keeps shadowing the root copy, which is the
                // duplicate this skip exists to avoid. A real directory is
                // the pruner's to remove, and only ever belongs to a
                // version that lost the slot.
                // `is_symlink_or_junction`, not `Path::is_symlink`: on
                // Windows `symlink_dir` falls back to a junction when it
                // cannot create a true symlink, and a junction is not a
                // symlink to the stdlib.
                let stale_link = match pnpm_fs::is_symlink_or_junction(&link_path) {
                    Ok(is_link) => is_link,
                    // Nothing to clean up — the common case, and the one
                    // `junction::exists` reports as an error rather than
                    // `Ok(false)`.
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                    Err(error) => {
                        return Err(HoistedLinkerError::SymlinkDirectDependencies(
                            SymlinkDirectDependenciesError::SymlinkPackage {
                                importer_id: importer_id.clone(),
                                name: alias.clone(),
                                source: SymlinkPackageError::SymlinkDir {
                                    symlink_target: target.clone(),
                                    symlink_path: link_path.clone(),
                                    error,
                                },
                            },
                        ));
                    }
                };
                if stale_link {
                    pnpm_fs::remove_symlink_dir(&link_path).map_err(|error| {
                        HoistedLinkerError::SymlinkDirectDependencies(
                            SymlinkDirectDependenciesError::SymlinkPackage {
                                importer_id: importer_id.clone(),
                                name: alias.clone(),
                                source: SymlinkPackageError::SymlinkDir {
                                    symlink_target: target.clone(),
                                    symlink_path: link_path.clone(),
                                    error,
                                },
                            },
                        )
                    })?;
                }
                continue;
            }
            if pnpm_fs::lexical_normalize(&link_path) == pnpm_fs::lexical_normalize(target) {
                linked_names.push(alias.clone());
                continue;
            }
            crate::symlink_package(target, &link_path).map_err(|source| {
                HoistedLinkerError::SymlinkDirectDependencies(
                    SymlinkDirectDependenciesError::SymlinkPackage {
                        importer_id: importer_id.clone(),
                        name: alias.clone(),
                        source,
                    },
                )
            })?;
            linked_names.push(alias.clone());
        }
        crate::link_direct_dep_bins(&modules_dir, &linked_names, &link_options).map_err(
            |source| {
                HoistedLinkerError::SymlinkDirectDependencies(
                    SymlinkDirectDependenciesError::LinkBins(source),
                )
            },
        )?;
    }
    Ok(())
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

/// Pre-computed hoist plan threaded across the install pipeline so
/// the dedupe pass in [`crate::SymlinkDirectDependencies`] (which
/// runs before the on-disk hoist phase in pacquet's ordering) can
/// fold publicly-hoisted aliases into root's target map. The on-disk
/// hoist phase later consumes the same [`crate::HoistResult`] instead of
/// re-running the traversal.
pub struct HoistPlan {
    pub graph: HashMap<PackageKey, crate::HoistGraphNode>,
    pub result: crate::HoistResult,
    pub skipped: HashSet<PackageKey>,
}

/// Compute the in-memory hoist plan. Returns `None` when nothing
/// should be hoisted today (no patterns, no lockfile graph, or the
/// install is going through the hoisted linker). Side-effect-free:
/// the on-disk symlinks happen later in the pipeline. Same input
/// gating as the legacy in-place block in [`crate::install_frozen_lockfile::InstallFrozenLockfile::run`].
/// `hoist-workspace-packages` input: every named non-root project's
/// `name → absolute project dir`, the shape v11 builds from
/// `allProjects` for its `hoistedWorkspacePackages` map. The root
/// project itself is excluded — its dir *is* where the hoisted
/// modules live.
#[must_use]
pub fn workspace_packages_for_hoist(
    workspace_root: &Path,
    project_manifests: &[(PathBuf, &pnpm_package_manifest::PackageManifest)],
) -> indexmap::IndexMap<String, PathBuf> {
    project_manifests
        .iter()
        .filter(|(project_dir, _)| project_dir != workspace_root)
        .filter_map(|(project_dir, manifest)| {
            let name = manifest.value().get("name")?.as_str()?;
            Some((name.to_string(), project_dir.clone()))
        })
        .collect()
}

#[expect(
    clippy::too_many_arguments,
    reason = "bundles every lockfile/config axis one hoist plan needs; both call sites pass the same shapes"
)]
pub fn compute_hoist_plan(
    config: &Config,
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
    importers: &HashMap<String, pnpm_lockfile::ProjectSnapshot>,
    dependency_groups: &[pnpm_package_manifest::DependencyGroup],
    skipped: &SkippedSnapshots,
    is_hoisted: bool,
    hoisted_workspace_packages: Option<&indexmap::IndexMap<String, PathBuf>>,
) -> Option<HoistPlan> {
    if is_hoisted {
        return None;
    }
    // Independent of the empty patterns
    // [`Config::apply_virtual_store_only_derivation`] leaves behind, so a
    // caller that sets the flag without going through `Config::current`
    // still gets no hoisting.
    if config.virtual_store_only {
        return None;
    }
    if config.hoist_pattern.is_none() && config.public_hoist_pattern.is_none() {
        return None;
    }
    let (Some(snaps), Some(pkgs)) = (snapshots, packages) else { return None };
    let private_pattern = create_matcher(config.hoist_pattern.as_deref().unwrap_or(&[]));
    let public_pattern = create_matcher(config.public_hoist_pattern.as_deref().unwrap_or(&[]));
    // Static fast-path: when both compiled matchers come from empty
    // pattern lists (`Some([])`), there's no alias they could match,
    // so the traversal would visit every node only to drop every child.
    // Skip the graph-build + walk entirely.
    if private_pattern.is_empty() && public_pattern.is_empty() {
        return None;
    }
    let graph = crate::build_hoist_graph_with_max_length(
        snaps,
        pkgs,
        config.virtual_store_dir_max_length as usize,
    );
    // Walk every importer's direct deps so transitives unique to a
    // workspace project still get privately hoisted into the shared
    // `<vs>/node_modules` and contribute to `hoistedDependencies`.
    // The `link:` workspace-sibling entries `build_direct_deps_by_importer`
    // sees are skipped via [`pnpm_lockfile::ImporterDepVersion::as_regular`].
    let direct_deps = build_direct_deps_by_importer(importers, dependency_groups.iter().copied());
    // `HoistInputs` takes `&HashSet<PackageKey>`; build it once from
    // the outer `SkippedSnapshots` by cloning the small skip set
    // (typically 0-100 entries). Stored on [`HoistPlan`] so the
    // later on-disk pass can reuse the exact same set the traversal saw.
    let hoist_skipped: HashSet<PackageKey> = skipped.iter().cloned().collect();
    let result = get_hoisted_dependencies(&crate::HoistInputs {
        graph: &graph,
        direct_deps_by_importer: &direct_deps,
        skipped: &hoist_skipped,
        private_pattern,
        public_pattern,
        hoisted_workspace_packages,
    })?;
    Some(HoistPlan { graph, result, skipped: hoist_skipped })
}

/// Build the `<alias → resolved-target-dir>` map for every publicly-
/// hoisted entry that will land in root's `node_modules/`. Pacquet
/// runs the dedupe pass before the on-disk hoist phase, so this map
/// lets the dedupe see the aliases it would otherwise miss — by the
/// time the linker reads `<root>/node_modules/`, the public-hoist
/// symlinks are already there because hoist ran first.
///
/// Skipped snapshots are dropped (their slot dir doesn't exist on
/// disk), missing-in-graph entries are dropped, and only `Public`
/// hoists contribute (private hoists land in the virtual store's
/// own `node_modules`, not root's). The target path uses the same
/// `<slot>/node_modules/<name>` shape that the on-disk hoist symlink
/// will point at, so [`PathBuf`] equality with
/// [`SymlinkDirectDependencies`]'s computed targets is exact.
#[must_use]
pub fn collect_public_hoist_targets(
    result: &crate::HoistResult,
    graph: &HashMap<PackageKey, crate::HoistGraphNode>,
    layout: &crate::VirtualStoreLayout,
    hoist_skipped: &HashSet<PackageKey>,
) -> BTreeMap<String, PathBuf> {
    let mut targets = BTreeMap::new();
    // Publicly-hoisted workspace packages land in root's
    // `node_modules/` too; their dedupe target is the project dir
    // the hoist symlink points at.
    for (alias, kind, project_dir) in &result.hoisted_workspace_aliases {
        if matches!(kind, pnpm_modules_yaml::HoistKind::Public) {
            targets.entry(alias.clone()).or_insert_with(|| project_dir.clone());
        }
    }
    for (node_id, alias_map) in &result.hoisted_dependencies_by_node_id {
        if hoist_skipped.contains(node_id) {
            continue;
        }
        let Some(node) = graph.get(node_id) else { continue };
        let dep_dir = layout.slot_dir(node_id).join("node_modules").join(node.name.to_string());
        for (alias, kind) in alias_map {
            if !matches!(kind, pnpm_modules_yaml::HoistKind::Public) {
                continue;
            }
            // First-wins: the traversal already chose one source per alias
            // via its `hoisted_aliases` claim. Multiple entries with
            // the same alias would be a hoister bug; preserve the
            // first deterministically.
            targets.entry(alias.clone()).or_insert_with(|| dep_dir.clone());
        }
    }
    targets
}

/// Pull the leading major-version digits out of a semver string like
/// `"22.11.0"`. Returns `None` if the leading token isn't parseable
/// as `u32`. Used to derive the engine-name string the
/// side-effects cache lookup expects without re-spawning
/// `node --version`.
#[must_use]
pub fn parse_major_from_version(version: &str) -> Option<u32> {
    let after_v = version.strip_prefix('v').unwrap_or(version);
    after_v.split('.').next()?.parse().ok()
}

/// Pull the `node@runtime:<version>` major out of a lockfile's
/// `snapshots:` map, if the project pinned a runtime Node.
///
/// The runtime resolver writes the pinned Node into the lockfile as a
/// snapshot with key `node@runtime:<version>`. The engine-name string
/// anchors the GVS hash and the side-effects-cache key prefix to that
/// pinned major instead of the host's own `node --version`. Scans the
/// snapshots with "first hit wins" semantics (the resolver rejects
/// workspaces with conflicting pins before they reach the lockfile).
///
/// Returns `None` when no importer pinned a runtime — callers should
/// then fall through to the host probe (`node --version` or the
/// cached `host_node`).
#[must_use]
pub fn find_runtime_node_major(
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
) -> Option<u32> {
    let snapshots = snapshots?;
    for key in snapshots.keys() {
        if key.suffix.prefix() != Prefix::Runtime {
            continue;
        }
        // Only `node@runtime:` feeds the Node-shaped engine string —
        // `bun@runtime:` and `deno@runtime:` exist as separate runtime
        // kinds. Scan for `node@runtime:` exclusively.
        if key.name.scope.is_some() || key.name.bare != "node" {
            continue;
        }
        // `Version::major` is `u64`; the major is small (<=99 in
        // practice), so the cast is lossless. The downstream
        // `engine_name` argument is `u32`.
        let major = key.suffix.version_semver()?.major;
        return Some(major as u32);
    }
    None
}

/// Read one snapshot's own `engines.runtime` Node pin from its
/// `dependencies` map. The resolver desugars `engines.runtime`
/// declared on a dep's manifest into
/// `dependencies.node: 'runtime:<version>'`.
///
/// Returns the bare major when this snapshot pins its own Node, or
/// `None` when it doesn't — callers should then fall back to the
/// install-wide pin / host probe via [`find_runtime_node_major`].
///
/// Per-snapshot resolution matters because the bin linker routes
/// lifecycle-script spawns for a pinning package through that
/// package's own downloaded Node. Anchoring the snapshot's GVS engine
/// hash to an install-wide value would produce the wrong
/// side-effects-cache key for cross-pinning installs.
#[must_use]
pub fn find_own_runtime_node_major(snapshot: &SnapshotEntry) -> Option<u32> {
    let deps = snapshot.dependencies.as_ref()?;
    for (alias, dep_ref) in deps {
        if alias.scope.is_some() || alias.bare != "node" {
            continue;
        }
        // `link:` deps have no version slot and can't carry a
        // `runtime:` pin — skip them.
        let Some(ver_peer) = dep_ref.ver_peer() else {
            continue;
        };
        if ver_peer.prefix() != Prefix::Runtime {
            continue;
        }
        // Same cast as `find_runtime_node_major` above; see the
        // comment there for why `u64 → u32` is lossless in practice.
        return Some(ver_peer.version_semver()?.major as u32);
    }
    None
}
