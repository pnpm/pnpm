use super::{
    Arc, AtomicU8, AuthHeaders, BTreeMap, Catalogs, Config, DependencyGroup,
    DepsRequiringBuildSink, HashSet, HoistedDependencies, InMemoryPackageMetaCache,
    IncludedDependencies, InstallError, InstallFrozenLockfile, InstallWithFreshLockfile, Lockfile,
    LogEvent, LogLevel, MemCache, NodeLinker, PackageManifest, Path, PathBuf, PeerIssuesSink,
    PnpmLog, ProjectMutation, RebuildOptions, Reporter, ResolutionVerifier, ResolvedPackages,
    ThrottledClient, UpdateSeedPolicy, build_workspace_packages_map, map_fresh_lockfile_error,
    map_frozen_lockfile_error, node_version_from_engines_runtime, record_lockfile_verified,
    verify_lockfile_eagerly,
};

pub(super) struct MaterializationInputs<'a, 'install> {
    pub(super) tarball_mem_cache: Arc<MemCache>,
    pub(super) resolved_packages: &'a ResolvedPackages,
    pub(super) http_client: &'a ThrottledClient,
    pub(super) http_client_arc: Arc<ThrottledClient>,
    pub(super) config: &'static Config,
    pub(super) manifest: &'a PackageManifest,
    pub(super) lockfile: Option<&'a Lockfile>,
    pub(super) take_frozen_path: bool,
    pub(super) lockfile_verification_override:
        Option<super::LockfileVerificationOverride<'install>>,
    pub(super) resolution_verifiers: Vec<Arc<dyn ResolutionVerifier>>,
    pub(super) derived_lockfile_path: Option<PathBuf>,
    pub(super) dependency_groups: Vec<DependencyGroup>,
    pub(super) project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub(super) workspace_projects: Option<&'a [pnpm_workspace::Project]>,
    pub(super) requested_importer_ids: Option<&'a HashSet<String>>,
    pub(super) real_importer_ids: &'a HashSet<String>,
    pub(super) workspace_root: &'a Path,
    pub(super) included: IncludedDependencies,
    pub(super) node_linker: NodeLinker,
    pub(super) rebuild: Option<&'a RebuildOptions>,
    pub(super) ignore_manifest_check: bool,
    pub(super) mutation: ProjectMutation,
    pub(super) current_lockfile: Option<&'a Lockfile>,
    pub(super) supported_architectures:
        Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    pub(super) skip_runtimes: bool,
    pub(super) modules_manifest: Option<&'a pnpm_modules_yaml::ModulesLayout>,
    pub(super) prior_hoisted_dependencies: Option<&'a HoistedDependencies>,
    /// Filled by the frozen path's `CreateVirtualStore` after its
    /// warm/cold partition; consumed by the npm verifier's age gate.
    pub(super) planned_canonical_fetches: pnpm_resolving_resolver_base::PlannedCanonicalFetches,
    pub(super) prune_orphans: bool,
    pub(super) logged_methods: &'a AtomicU8,
    pub(super) update_checksums: bool,
    pub(super) meta_cache: Arc<InMemoryPackageMetaCache>,
    pub(super) resolve_only: bool,
    pub(super) dry_run: bool,
    pub(super) can_prompt: bool,
    pub(super) persist_policy_excludes: bool,
    pub(super) update_seed_policy: UpdateSeedPolicy,
    pub(super) preferred_versions_override: Option<pnpm_resolving_resolver_base::PreferredVersions>,
    pub(super) auth_override: Option<Arc<AuthHeaders>>,
    pub(super) resolution_observer: Option<Arc<dyn crate::ResolutionObserver>>,
    pub(super) peer_issues_sink: Option<PeerIssuesSink>,
    pub(super) deps_requiring_build_sink: Option<DepsRequiringBuildSink>,
    pub(super) pnpmfile_hook: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    pub(super) save_lockfile: bool,
    pub(super) manifest_spec_bumps: Option<&'a crate::ManifestSpecBumps>,
    pub(super) catalogs: &'a Catalogs,
    pub(super) prefix: &'a str,
}

pub(super) struct MaterializationOutput {
    pub(super) ignored_builds: Vec<String>,
    pub(super) deferred_builds: Vec<String>,
    pub(super) injected_deps: BTreeMap<String, Vec<String>>,
    pub(super) hoisted_dependencies: HoistedDependencies,
    pub(super) hoisted_locations: BTreeMap<String, Vec<String>>,
    pub(super) install_skipped: crate::SkippedSnapshots,
    pub(super) fresh_lockfile: Option<Lockfile>,
}

pub(super) async fn materialize<Reporter: self::Reporter + 'static>(
    inputs: MaterializationInputs<'_, '_>,
) -> Result<MaterializationOutput, InstallError> {
    let MaterializationInputs {
        tarball_mem_cache,
        resolved_packages,
        http_client,
        http_client_arc,
        config,
        manifest,
        lockfile,
        take_frozen_path,
        lockfile_verification_override,
        resolution_verifiers,
        derived_lockfile_path,
        dependency_groups,
        project_manifests,
        workspace_projects,
        requested_importer_ids,
        real_importer_ids,
        workspace_root,
        included,
        node_linker,
        rebuild,
        ignore_manifest_check,
        mutation,
        current_lockfile,
        supported_architectures,
        skip_runtimes,
        modules_manifest,
        prior_hoisted_dependencies,
        planned_canonical_fetches,
        prune_orphans,
        logged_methods,
        update_checksums,
        meta_cache,
        resolve_only,
        dry_run,
        can_prompt,
        persist_policy_excludes,
        update_seed_policy,
        preferred_versions_override,
        auth_override,
        resolution_observer,
        peer_issues_sink,
        deps_requiring_build_sink,
        pnpmfile_hook,
        save_lockfile,
        manifest_spec_bumps,
        catalogs,
        prefix,
    } = inputs;
    let ignored_builds: Vec<String>;
    let deferred_builds: Vec<String>;
    let injected_deps: BTreeMap<String, Vec<String>>;
    let effective_node_version =
        config.node_version.clone().or_else(|| node_version_from_engines_runtime(manifest.value()));
    let (hoisted_dependencies, hoisted_locations, install_skipped, fresh_lockfile): (
        HoistedDependencies,
        BTreeMap<String, Vec<String>>,
        crate::SkippedSnapshots,
        Option<Lockfile>,
    ) = if take_frozen_path {
        let lockfile = lockfile.expect("dispatch verified lockfile is present");
        // pnpm's headless installer announces itself whenever it is
        // entered — also on a cold `node_modules` and on subset
        // (`--filter`) installs — not only when nothing needs to be
        // materialized. `pnpm fetch` gets upstream's
        // ignorePackageManifest wording instead; it is the one
        // caller combining `ignore_manifest_check` with a non-full
        // install, and the flag alone can't identify it because
        // `install --ignore-manifest-check` is a user-facing way to
        // skip the frozen freshness gate on a full install.
        // Upstream's headless entry returns before the announcement
        // for an empty lockfile (`isEmptyLockfile`), and an explicit
        // `pnpm rebuild` is not an install, so both stay silent.
        if rebuild.is_none() && !lockfile.is_empty() {
            let message = if ignore_manifest_check && !mutation.is_full_install() {
                "Importing packages to virtual store"
            } else {
                "Lockfile is up to date, resolution step is skipped"
            };
            Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Info,
                message: message.to_string(),
                prefix: prefix.to_string(),
            }));
        }
        let initial_materialization_ids = requested_importer_ids.map(|selected| {
            if matches!(node_linker, NodeLinker::Hoisted) {
                lockfile.importers.keys().cloned().collect()
            } else {
                selected.clone()
            }
        });
        let empty_skipped = crate::SkippedSnapshots::new();
        let materialization = initial_materialization_ids.as_ref().map(|importer_ids| {
            crate::materialization_closure(
                lockfile,
                workspace_root,
                importer_ids,
                included,
                &empty_skipped,
            )
        });
        let materialization_lockfile =
            materialization.as_ref().map_or(lockfile, |closure| &closure.lockfile);
        let project_anchor_ids = match requested_importer_ids {
            Some(selected) if matches!(node_linker, NodeLinker::Hoisted) => selected.clone(),
            Some(_) => materialization
                .as_ref()
                .expect("selected install has a materialization closure")
                .importer_ids
                .clone(),
            None => real_importer_ids.clone(),
        };
        let frozen_project_manifests = project_manifests
            .iter()
            .filter(|(project_dir, _)| {
                let importer_id =
                    pnpm_workspace::importer_id_from_root_dir(workspace_root, project_dir);
                project_anchor_ids.contains(&importer_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        let Lockfile { lockfile_version, importers, packages, snapshots, .. } =
            materialization_lockfile;
        let lockfile_major = lockfile_version.major;
        let supported_lockfile_major = matches!(lockfile_major, 9 | 12);
        debug_assert!(supported_lockfile_major);

        let mut frozen_verification_override = lockfile_verification_override;
        if requested_importer_ids.is_some() {
            if let Some(verification_override) = frozen_verification_override.take() {
                verification_override.await.map_err(map_frozen_lockfile_error)?;
            } else {
                verify_lockfile_eagerly::<Reporter>(
                    lockfile,
                    &resolution_verifiers,
                    derived_lockfile_path.as_deref(),
                    &config.cache_dir,
                )
                .await?;
            }
        }
        let frozen_resolution_verifiers = if requested_importer_ids.is_some() {
            &[][..]
        } else {
            resolution_verifiers.as_slice()
        };

        let frozen_result = InstallFrozenLockfile {
            http_client,
            config,
            importers,
            packages: packages.as_ref(),
            snapshots: snapshots.as_ref(),
            lockfile: materialization_lockfile,
            resolution_verifiers: frozen_resolution_verifiers,
            lockfile_verification_override: frozen_verification_override,
            lockfile_path: derived_lockfile_path.as_deref(),
            current_lockfile,
            // `--force` relinks every package, so the per-snapshot
            // "unchanged since the previous install" skip must not
            // see the current lockfile — pnpm's
            // `lockfileToDepGraph(..., opts.force ? null :
            // currentLockfile)`. `current_lockfile` itself stays:
            // pnpm's prune runs on the real current lockfile even
            // under force.
            current_snapshots: (!config.force)
                .then_some(current_lockfile)
                .flatten()
                .and_then(|lockfile| lockfile.snapshots.as_ref()),
            current_packages: (!config.force)
                .then_some(current_lockfile)
                .flatten()
                .and_then(|lockfile| lockfile.packages.as_ref()),
            dependency_groups,
            project_manifests: &frozen_project_manifests,
            package_map_project_manifests: project_manifests,
            logged_methods,
            workspace_root,
            requester: prefix,
            supported_architectures,
            skip_runtimes,
            node_version: effective_node_version.clone(),
            node_linker,
            tarball_mem_cache: Some(&tarball_mem_cache),
            seed_skipped: modules_manifest.map(|manifest| manifest.skipped.clone()),
            rebuild,
            prior_hoisted_dependencies,
            prune_orphans,
            planned_canonical_fetches: Some(&planned_canonical_fetches),
        }
        .run::<Reporter>()
        .await
        // Surface a verification failure as the same top-level
        // `LockfileVerification` variant the eager paths use, rather
        // than nesting it under `FrozenLockfile` — the concurrent gate
        // is the same gate, just run alongside the fetch.
        .map_err(map_frozen_lockfile_error)?;

        ignored_builds = frozen_result.ignored_builds;
        deferred_builds = frozen_result.deferred_builds;
        injected_deps = frozen_result.injected_deps;
        (
            frozen_result.hoisted_dependencies,
            frozen_result.hoisted_locations,
            frozen_result.skipped,
            None,
        )
    } else {
        // Re-verify the existing lockfile alongside the fresh resolve,
        // matching the pre-resolution gate: a committed lockfile that
        // bypassed the policy locally is caught even though the resolver
        // re-resolves from it. The fan-out's registry round trips overlap
        // the resolve and the materialization; the verdict still gates
        // bin linking, dependency builds, and the lockfile save inside
        // [`InstallWithFreshLockfile`]. No-op when there's no lockfile
        // (state 4) or verification is disabled. The pnpr override stays
        // a blocking gate — it is a single round trip with nothing
        // substantial to overlap.
        let lockfile_verification_gate =
            if let Some(lockfile_verification_override) = lockfile_verification_override {
                lockfile_verification_override.await.map_err(map_frozen_lockfile_error)?;
                None
            } else {
                lockfile.and_then(|loaded_lockfile| {
                    super::LockfileVerificationGate::spawn::<Reporter>(
                        loaded_lockfile,
                        &resolution_verifiers,
                        derived_lockfile_path.as_deref(),
                        &config.cache_dir,
                    )
                })
            };

        let workspace_packages = build_workspace_packages_map(workspace_projects);
        // Build the per-importer manifest list. The root importer
        // (`"."`) always reuses the in-memory `Install.manifest`
        // — `pacquet add` mutates that value before calling install,
        // so re-reading from disk would walk the pre-add shape and
        // miss the freshly-added dep. Sibling importers come from
        // the `find_workspace_projects` walk, which read them off
        // disk for `workspace_packages` already.
        let importer_manifests: BTreeMap<String, &PackageManifest> = project_manifests
            .iter()
            .map(|(project_dir, manifest)| {
                (pnpm_workspace::importer_id_from_root_dir(workspace_root, project_dir), *manifest)
            })
            .collect();
        let fresh_result = InstallWithFreshLockfile {
            tarball_mem_cache,
            resolved_packages,
            http_client,
            http_client_arc: Arc::clone(&http_client_arc),
            config,
            importer_manifests,
            dependency_groups,
            logged_methods,
            requester: prefix,
            catalogs: catalogs.clone(),
            lockfile_dir: workspace_root,
            workspace_packages,
            update_checksums,
            meta_cache: Arc::clone(&meta_cache),
            // States 3 and 4 of the dispatch share this branch.
            // State 3 (lockfile present but stale or
            // `preferFrozenLockfile: false`) passes the existing
            // lockfile so the resolver seeds
            // `getPreferredVersionsFromLockfileAndManifests` with
            // already-pinned `(name, version)` pairs — unrelated
            // entries keep their pins on rewrite (the `update: false`
            // mode). State 4 (no lockfile) passes `None`.
            wanted_lockfile: lockfile,
            node_version: effective_node_version,
            node_linker,
            supported_architectures,
            lockfile_only: resolve_only,
            skip_runtimes,
            dry_run,
            save_lockfile,
            can_prompt,
            persist_policy_excludes,
            is_full_install: mutation.is_full_install(),
            update_seed_policy,
            preferred_versions_override,
            auth_override,
            resolution_observer,
            peer_issues_sink: peer_issues_sink.clone(),
            deps_requiring_build_sink: deps_requiring_build_sink.as_ref().map(Arc::clone),
            pnpmfile_hook_override: pnpmfile_hook,
            real_importer_ids: requested_importer_ids.map(|_| real_importer_ids),
            selected_importer_ids: requested_importer_ids,
            current_lockfile,
            prior_hoisted_dependencies,
            prune_orphans,
            manifest_spec_bumps,
            lockfile_verification_gate,
        }
        .run::<Reporter>()
        .await
        .map_err(map_fresh_lockfile_error)?;

        if fresh_result.can_record_lockfile_verification
            && let Some(lockfile) = fresh_result.wanted_lockfile.as_ref()
        {
            // Record under the same path the verification gates key
            // their cache on, so the next install's stat shortcut hits.
            let lockfile_path = derived_lockfile_path
                .clone()
                .unwrap_or_else(|| workspace_root.join(Lockfile::FILE_NAME));
            record_lockfile_verified(
                Some(&config.cache_dir),
                &lockfile_path,
                lockfile,
                &resolution_verifiers,
            );
        }

        ignored_builds = fresh_result.ignored_builds;
        deferred_builds = fresh_result.deferred_builds;
        injected_deps = fresh_result.injected_deps;
        (
            fresh_result.hoisted_dependencies,
            fresh_result.hoisted_locations,
            fresh_result.skipped,
            fresh_result.wanted_lockfile,
        )
    };

    Ok(MaterializationOutput {
        ignored_builds,
        deferred_builds,
        injected_deps,
        hoisted_dependencies,
        hoisted_locations,
        install_skipped,
        fresh_lockfile,
    })
}
