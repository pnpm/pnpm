use super::{
    Arc, AtomicU8, AuthHeaders, BTreeMap, Catalogs, Config, DependencyGroup,
    DepsRequiringBuildSink, HashSet, HoistedDependencies, InMemoryPackageMetaCache,
    IncludedDependencies, InstallError, InstallFrozenLockfile, InstallWithFreshLockfile, Lockfile,
    LockfileEntries, LogEvent, LogLevel, MemCache, NodeLinker, PackageManifest, Path, PathBuf,
    PeerIssuesSink, PnpmLog, RebuildOptions, Reporter, ResolutionVerifier, ThrottledClient,
    UpdateSeedPolicy, build_workspace_packages_map, map_fresh_lockfile_error,
    map_frozen_lockfile_error, record_lockfile_verified, run::InstallView, verify_lockfile_eagerly,
};

pub(super) struct MaterializationInputs<'a, 'install> {
    pub(super) install: InstallView<'a>,
    pub(super) effective_node_version: Option<String>,
    pub(super) tarball_mem_cache: Arc<MemCache>,
    pub(super) http_client_arc: Arc<ThrottledClient>,
    pub(super) lockfile: Option<&'a Lockfile>,
    /// An `Arc` handle to the same document as [`Self::lockfile`], when
    /// the lazy loader holds one. Lets the fresh path seed the resolver
    /// without deep-copying a workspace-scale lockfile; `None` falls
    /// back to the copy.
    pub(super) lockfile_shared: Option<Arc<Lockfile>>,
    pub(super) merge_wanted_lockfile: Option<&'a Lockfile>,
    pub(super) take_frozen_path: bool,
    pub(super) lockfile_verification_override:
        Option<super::LockfileVerificationOverride<'install>>,
    pub(super) resolution_verifiers: Vec<Arc<dyn ResolutionVerifier>>,
    pub(super) derived_lockfile_path: Option<PathBuf>,
    pub(super) dependency_groups: Vec<DependencyGroup>,
    pub(super) project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub(super) lockfile_specifier_project_manifests: Option<Vec<(PathBuf, PackageManifest)>>,
    pub(super) workspace_projects: Option<&'a [pnpm_workspace::Project]>,
    pub(super) requested_importer_ids: Option<&'a HashSet<String>>,
    pub(super) real_importer_ids: &'a HashSet<String>,
    pub(super) workspace_root: &'a Path,
    pub(super) included: IncludedDependencies,
    pub(super) rebuild: Option<&'a RebuildOptions>,
    pub(super) current_lockfile: Option<&'a Lockfile>,
    pub(super) supported_architectures:
        Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    /// A host detection spawned right after the wanted lockfile parse —
    /// see [`pnpm_deps_restorer::materialization_plan::HostDetection::spawn`].
    /// Handed to whichever install path runs.
    pub(super) early_host_detection:
        Option<pnpm_deps_restorer::materialization_plan::HostDetection>,
    pub(super) modules_manifest: Option<&'a pnpm_modules_yaml::ModulesLayout>,
    pub(super) prior_hoisted_dependencies: Option<&'a HoistedDependencies>,
    /// Filled by the frozen path's `CreateVirtualStore` after its
    /// warm/cold partition; consumed by the npm verifier's age gate.
    pub(super) planned_canonical_fetches: pnpm_resolving_resolver_base::PlannedCanonicalFetches,
    pub(super) prune_orphans: bool,
    pub(super) logged_methods: &'a AtomicU8,
    pub(super) meta_cache: Arc<InMemoryPackageMetaCache>,
    pub(super) resolve_only: bool,
    pub(super) can_prompt: bool,
    pub(super) update_seed_policy: UpdateSeedPolicy,
    pub(super) preferred_versions_override: Option<pnpm_resolving_resolver_base::PreferredVersions>,
    pub(super) auth_override: Option<Arc<AuthHeaders>>,
    pub(super) resolution_observer: Option<Arc<dyn crate::ResolutionObserver>>,
    pub(super) peer_issues_sink: Option<PeerIssuesSink>,
    pub(super) deps_requiring_build_sink: Option<DepsRequiringBuildSink>,
    pub(super) pnpmfile_hook: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    pub(super) deploy_manifest_hook: bool,
    pub(super) save_lockfile: bool,
    pub(super) manifest_spec_bumps: Option<&'a crate::ManifestSpecBumps>,
    pub(super) catalogs: &'a Catalogs,
    pub(super) prefix: &'a str,
}

/// The store-index writer's wind-down task — see
/// [`MaterializationOutput::store_index_teardown`].
pub(super) type StoreIndexTeardown =
    tokio::task::JoinHandle<Result<(), pnpm_store_dir::StoreIndexError>>;

pub(super) struct MaterializationOutput {
    pub(super) ignored_builds: Vec<String>,
    pub(super) deferred_builds: Vec<String>,
    pub(super) injected_deps: BTreeMap<String, Vec<String>>,
    pub(super) hoisted_dependencies: HoistedDependencies,
    pub(super) hoisted_locations: BTreeMap<String, Vec<String>>,
    pub(super) install_skipped: crate::SkippedSnapshots,
    /// See
    /// [`crate::InstallWithFreshLockfileResult::peer_issue_importer_ids`].
    /// Empty on the frozen path, which resolves nothing and so also
    /// leaves `fresh_lockfile` `None`.
    pub(super) peer_issue_importer_ids: HashSet<String>,
    pub(super) fresh_lockfile: Option<Lockfile>,
    /// The store-index writer task, already winding down (both install
    /// paths dropped every writer handle before returning). The caller
    /// awaits it after the tail writes it can overlap with — the full
    /// rationale lives at the await site in `run.rs`.
    pub(super) store_index_teardown: StoreIndexTeardown,
}

pub(super) async fn materialize<Reporter: self::Reporter + 'static>(
    inputs: MaterializationInputs<'_, '_>,
) -> Result<MaterializationOutput, InstallError> {
    if inputs.take_frozen_path {
        inputs.frozen::<Reporter>().await
    } else {
        inputs.fresh::<Reporter>().await
    }
}

/// What a frozen install materializes: the closure of the requested
/// importers under the isolated linker, and the project manifests the
/// bin links anchor on.
struct FrozenScope<'a> {
    closure: Option<crate::MaterializationClosure>,
    project_manifests: Vec<(PathBuf, &'a PackageManifest)>,
}

impl FrozenScope<'_> {
    fn lockfile<'l>(&'l self, lockfile: &'l Lockfile) -> &'l Lockfile {
        self.closure.as_ref().map_or(lockfile, |closure| &closure.lockfile)
    }
}

impl<'a> MaterializationInputs<'a, '_> {
    fn frozen_scope(&self, lockfile: &Lockfile) -> FrozenScope<'a> {
        let empty_skipped = crate::SkippedSnapshots::new();
        let closure = initial_materialization_ids(
            lockfile,
            self.requested_importer_ids,
            self.install.node_linker,
        )
        .as_ref()
        .map(|importer_ids| {
            crate::materialization_closure(
                lockfile,
                self.workspace_root,
                importer_ids,
                self.included,
                &empty_skipped,
            )
        });
        let project_anchor_ids = frozen_project_anchor_ids(
            self.requested_importer_ids,
            self.real_importer_ids,
            self.install.node_linker,
            closure.as_ref(),
        );
        FrozenScope {
            project_manifests: anchored_project_manifests(
                self.project_manifests,
                self.workspace_root,
                &project_anchor_ids,
            ),
            closure,
        }
    }

    async fn frozen<Reporter: self::Reporter + 'static>(
        self,
    ) -> Result<MaterializationOutput, InstallError> {
        let lockfile = self.lockfile.expect("dispatch verified lockfile is present");
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
        announce_headless_install::<Reporter>(
            lockfile,
            self.rebuild,
            self.install.ignore_manifest_check && !self.install.mutation.is_full_install(),
            self.prefix,
        );
        let scope = self.frozen_scope(lockfile);
        debug_assert!(matches!(scope.lockfile(lockfile).lockfile_version.major, 9 | 12));

        let frozen_verification_override = settle_frozen_verification::<Reporter>(
            self.requested_importer_ids,
            self.lockfile_verification_override,
            lockfile,
            &self.resolution_verifiers,
            self.derived_lockfile_path.as_deref(),
            &self.install.config.cache_dir,
        )
        .await?;
        let frozen_result = InstallFrozenLockfile {
            http_client: self.install.http_client,
            config: self.install.config,
            pnpmfile_hook: self.pnpmfile_hook.as_ref(),
            lockfile: scope.lockfile(lockfile),
            resolution_verifiers: self
                .requested_importer_ids
                .map_or(self.resolution_verifiers.as_slice(), |_| &[][..]),
            lockfile_verification_override: frozen_verification_override,
            lockfile_path: self.derived_lockfile_path.as_deref(),
            current_lockfile: self.current_lockfile,
            current_entries: LockfileEntries::of_previous_install(
                self.current_lockfile,
                self.install.config.force,
            ),
            dependency_groups: &self.dependency_groups,
            project_manifests: &scope.project_manifests,
            package_map_project_manifests: self.project_manifests,
            logged_methods: self.logged_methods,
            workspace_root: self.workspace_root,
            requester: self.prefix,
            supported_architectures: self.supported_architectures,
            skip_runtimes: self.install.skip_runtimes,
            node_version: self.effective_node_version,
            early_host_detection: self.early_host_detection,
            node_linker: self.install.node_linker,
            tarball_mem_cache: Some(&self.tarball_mem_cache),
            seed_skipped: self.modules_manifest.map(|manifest| manifest.skipped.clone()),
            rebuild: self.rebuild,
            prior_hoisted_dependencies: self.prior_hoisted_dependencies,
            prune_orphans: self.prune_orphans,
            planned_canonical_fetches: Some(&self.planned_canonical_fetches),
        }
        .run::<Reporter>()
        .await
        // Surface a verification failure as the same top-level
        // `LockfileVerification` variant the eager paths use, rather
        // than nesting it under `FrozenLockfile` — the concurrent gate
        // is the same gate, just run alongside the fetch.
        .map_err(map_frozen_lockfile_error)?;
        Ok(MaterializationOutput {
            ignored_builds: frozen_result.ignored_builds,
            deferred_builds: frozen_result.deferred_builds,
            injected_deps: frozen_result.injected_deps,
            hoisted_dependencies: frozen_result.hoisted_dependencies,
            hoisted_locations: frozen_result.hoisted_locations,
            install_skipped: frozen_result.skipped,
            peer_issue_importer_ids: HashSet::new(),
            fresh_lockfile: None,
            store_index_teardown: frozen_result.store_index_teardown,
        })
    }

    async fn fresh<Reporter: self::Reporter + 'static>(
        self,
    ) -> Result<MaterializationOutput, InstallError> {
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
            if let Some(lockfile_verification_override) = self.lockfile_verification_override {
                lockfile_verification_override.await.map_err(map_frozen_lockfile_error)?;
                None
            } else {
                self.lockfile.and_then(|loaded_lockfile| {
                    super::LockfileVerificationGate::spawn::<Reporter>(
                        loaded_lockfile,
                        &self.resolution_verifiers,
                        self.derived_lockfile_path.as_deref(),
                        &self.install.config.cache_dir,
                    )
                })
            };
        let fresh_result = InstallWithFreshLockfile {
            tarball_mem_cache: self.tarball_mem_cache,
            resolved_packages: self.install.resolved_packages,
            http_client: self.install.http_client,
            http_client_arc: self.http_client_arc,
            config: self.install.config,
            // Build the per-importer manifest list. The root importer
            // (`"."`) always reuses the in-memory `Install.manifest`
            // — `pacquet add` mutates that value before calling install,
            // so re-reading from disk would walk the pre-add shape and
            // miss the freshly-added dep. Sibling importers come from
            // the `find_workspace_projects` walk, which read them off
            // disk for `workspace_packages` already.
            importer_manifests: importer_manifests_by_id(
                self.project_manifests,
                self.workspace_root,
            ),
            lockfile_specifier_manifests: self.lockfile_specifier_project_manifests.map(
                |manifests| lockfile_specifier_manifests_by_id(manifests, self.workspace_root),
            ),
            dependency_groups: &self.dependency_groups,
            logged_methods: self.logged_methods,
            requester: self.prefix,
            catalogs: self.catalogs.clone(),
            lockfile_dir: self.workspace_root,
            workspace_packages: build_workspace_packages_map(self.workspace_projects),
            update_checksums: self.install.update_checksums,
            meta_cache: self.meta_cache,
            // States 3 and 4 of the dispatch share this branch.
            // State 3 (lockfile present but stale or
            // `preferFrozenLockfile: false`) passes the existing
            // lockfile so the resolver seeds
            // `getPreferredVersionsFromLockfileAndManifests` with
            // already-pinned `(name, version)` pairs — unrelated
            // entries keep their pins on rewrite (the `update: false`
            // mode). State 4 (no lockfile) passes `None`.
            wanted_lockfile: self.lockfile,
            wanted_lockfile_shared: self.lockfile_shared,
            merge_wanted_lockfile: self.merge_wanted_lockfile,
            node_version: self.effective_node_version,
            early_host_detection: self.early_host_detection,
            node_linker: self.install.node_linker,
            supported_architectures: self.supported_architectures,
            lockfile_only: self.resolve_only,
            skip_runtimes: self.install.skip_runtimes,
            dry_run: self.install.dry_run,
            save_lockfile: self.save_lockfile,
            can_prompt: self.can_prompt,
            persist_policy_excludes: self.install.persist_policy_excludes,
            is_full_install: self.install.mutation.is_full_install(),
            update_seed_policy: self.update_seed_policy,
            preferred_versions_override: self.preferred_versions_override,
            auth_override: self.auth_override,
            resolution_observer: self.resolution_observer,
            peer_issues_sink: self.peer_issues_sink,
            deps_requiring_build_sink: self.deps_requiring_build_sink,
            pnpmfile_hook_override: self.pnpmfile_hook,
            deploy_manifest_hook: self.deploy_manifest_hook,
            real_importer_ids: self.requested_importer_ids.map(|_| self.real_importer_ids),
            selected_importer_ids: self.requested_importer_ids,
            current_lockfile: self.current_lockfile,
            prior_hoisted_dependencies: self.prior_hoisted_dependencies,
            prune_orphans: self.prune_orphans,
            manifest_spec_bumps: self.manifest_spec_bumps,
            resolution_verifiers: &self.resolution_verifiers,
            lockfile_verification_gate,
        }
        .run::<Reporter>()
        .await
        .map_err(map_fresh_lockfile_error)?;
        record_fresh_lockfile_verified(
            &fresh_result,
            self.derived_lockfile_path,
            (self.workspace_root, self.install.config),
            &self.resolution_verifiers,
        );
        Ok(MaterializationOutput {
            ignored_builds: fresh_result.ignored_builds,
            deferred_builds: fresh_result.deferred_builds,
            injected_deps: fresh_result.injected_deps,
            hoisted_dependencies: fresh_result.hoisted_dependencies,
            hoisted_locations: fresh_result.hoisted_locations,
            install_skipped: fresh_result.skipped,
            peer_issue_importer_ids: fresh_result.peer_issue_importer_ids,
            fresh_lockfile: fresh_result.wanted_lockfile,
            store_index_teardown: fresh_result.store_index_teardown,
        })
    }
}

/// The project manifests whose importer the frozen install anchors on.
fn anchored_project_manifests<'a>(
    project_manifests: &[(PathBuf, &'a PackageManifest)],
    workspace_root: &Path,
    project_anchor_ids: &HashSet<String>,
) -> Vec<(PathBuf, &'a PackageManifest)> {
    project_manifests
        .iter()
        .filter(|(project_dir, _)| {
            project_anchor_ids
                .contains(&pnpm_workspace::importer_id_from_root_dir(workspace_root, project_dir))
        })
        .cloned()
        .collect()
}

fn importer_manifests_by_id<'a>(
    project_manifests: &[(PathBuf, &'a PackageManifest)],
    workspace_root: &Path,
) -> BTreeMap<String, &'a PackageManifest> {
    project_manifests
        .iter()
        .map(|(project_dir, manifest)| {
            (pnpm_workspace::importer_id_from_root_dir(workspace_root, project_dir), *manifest)
        })
        .collect()
}

fn lockfile_specifier_manifests_by_id(
    project_manifests: Vec<(PathBuf, PackageManifest)>,
    workspace_root: &Path,
) -> BTreeMap<String, PackageManifest> {
    project_manifests
        .into_iter()
        .map(|(project_dir, manifest)| {
            (pnpm_workspace::importer_id_from_root_dir(workspace_root, &project_dir), manifest)
        })
        .collect()
}

// Record under the same path the verification gates key
// their cache on, so the next install's stat shortcut hits.
fn record_fresh_lockfile_verified(
    result: &crate::InstallWithFreshLockfileResult,
    derived_lockfile_path: Option<PathBuf>,
    site: (&Path, &Config),
    resolution_verifiers: &[Arc<dyn ResolutionVerifier>],
) {
    let (workspace_root, config) = site;
    if !result.can_record_lockfile_verification {
        return;
    }
    let Some(lockfile) = result.wanted_lockfile.as_ref() else { return };
    let lockfile_path =
        derived_lockfile_path.unwrap_or_else(|| workspace_root.join(config.wanted_lockfile_name()));
    record_lockfile_verified(
        Some(&config.cache_dir),
        &lockfile_path,
        lockfile,
        resolution_verifiers,
    );
}

/// A selected (`--filter`) frozen install verifies the whole lockfile up
/// front, so nothing is left for the concurrent gate to carry; an unselected
/// one hands its override straight through.
async fn settle_frozen_verification<'install, Reporter: self::Reporter>(
    requested_importer_ids: Option<&HashSet<String>>,
    verification_override: Option<super::LockfileVerificationOverride<'install>>,
    lockfile: &Lockfile,
    resolution_verifiers: &[Arc<dyn ResolutionVerifier>],
    derived_lockfile_path: Option<&Path>,
    cache_dir: &Path,
) -> Result<Option<super::LockfileVerificationOverride<'install>>, InstallError> {
    if requested_importer_ids.is_none() {
        return Ok(verification_override);
    }
    match verification_override {
        Some(verification_override) => {
            verification_override.await.map_err(map_frozen_lockfile_error)?;
        }
        None => {
            verify_lockfile_eagerly::<Reporter>(
                lockfile,
                resolution_verifiers,
                derived_lockfile_path,
                cache_dir,
            )
            .await?;
        }
    }
    Ok(None)
}

/// The importers whose own project manifests the frozen install anchors on.
fn frozen_project_anchor_ids(
    requested_importer_ids: Option<&HashSet<String>>,
    real_importer_ids: &HashSet<String>,
    node_linker: NodeLinker,
    materialization: Option<&crate::MaterializationClosure>,
) -> HashSet<String> {
    match requested_importer_ids {
        Some(selected) if matches!(node_linker, NodeLinker::Hoisted) => selected.clone(),
        Some(_) => materialization
            .expect("selected install has a materialization closure")
            .importer_ids
            .clone(),
        None => real_importer_ids.clone(),
    }
}

/// The importers a frozen install materializes first. A hoisted linker shares
/// one tree, so a selected install still has to materialize every importer.
fn initial_materialization_ids(
    lockfile: &Lockfile,
    requested_importer_ids: Option<&HashSet<String>>,
    node_linker: NodeLinker,
) -> Option<HashSet<String>> {
    let selected = requested_importer_ids?;
    if matches!(node_linker, NodeLinker::Hoisted) {
        return Some(lockfile.importers.keys().cloned().collect());
    }
    Some(selected.clone())
}

/// pnpm's headless installer announces itself whenever it is entered — also
/// on a cold `node_modules` and on subset (`--filter`) installs — not only
/// when nothing needs to be materialized.
///
/// `importing_only` gets upstream's `ignorePackageManifest` wording instead;
/// `pnpm fetch` is the one caller combining `ignore_manifest_check` with a
/// non-full install, and the flag alone can't identify it because `install
/// --ignore-manifest-check` is a user-facing way to skip the frozen freshness
/// gate on a full install. Upstream's headless entry returns before the
/// announcement for an empty lockfile (`isEmptyLockfile`), and an explicit
/// `pnpm rebuild` is not an install, so both stay silent.
fn announce_headless_install<Reporter: self::Reporter>(
    lockfile: &Lockfile,
    rebuild: Option<&RebuildOptions>,
    importing_only: bool,
    prefix: &str,
) {
    if rebuild.is_some() || lockfile.is_empty() {
        return;
    }
    let message = if importing_only {
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
