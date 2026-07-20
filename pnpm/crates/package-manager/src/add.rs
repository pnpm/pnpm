use crate::{
    CatalogDecision, CatalogModeDep, CatalogVersionMismatchError, DIRECT_GROUPS, Install,
    InstallError, ResolvedPackages, UpdateSeedPolicy, WorkspaceInstallSelection,
    catalog_cleanup::{
        WriteWorkspaceCatalogsError, write_workspace_catalogs, write_workspace_catalogs_selected,
    },
    decide_catalog_outcome, emit_initial_package_manifest, package_manifest_prefix,
    resolution_policy::{PickPolicy, pick_package_context},
    resolve_latest::LatestPicker,
    selected_project_indices,
};
use derive_more::{Display, Error};
use futures_util::{StreamExt, stream::FuturesOrdered};
use miette::Diagnostic;
use pacquet_catalogs_config::{
    InvalidCatalogsConfigurationError, get_catalogs_from_workspace_manifest,
};
use pacquet_catalogs_types::Catalogs;
use pacquet_config::Config;
use pacquet_engine_runtime_node_resolver::{NodeResolver, NodeResolverError};
use pacquet_lockfile::{Lockfile, MaybeLazyLockfile};
use pacquet_lockfile_preferred_versions::get_preferred_versions_from_lockfile_and_manifests;
use pacquet_network::ThrottledClient;
use pacquet_package_manifest::{DependencyGroup, PackageManifest, PackageManifestError};
use pacquet_registry::PinnedVersion;
use pacquet_reporter::{LogEvent, LogLevel, PackageManifestLog, PackageManifestMessage, Reporter};
use pacquet_resolving_git_resolver::{
    GitFetchContext, GitResolver, HostedGit, HostedOpts, RealGitProbe, RealGitRunner,
};
use pacquet_resolving_npm_resolver::{
    InMemoryPackageMetaCache, PackumentFetchLocker, PickPackageError, PickPackageOptions,
    parse_bare_specifier, pick_package, pick_registry_for_package, shared_packument_fetch_locker,
    which_version_is_pinned,
};
use pacquet_tarball::MemCache;
use std::{collections::HashSet, path::PathBuf, sync::Arc};

#[must_use]
pub struct Add<'a, DependencyGroupList>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    pub tarball_mem_cache: std::sync::Arc<MemCache>,
    pub resolved_packages: &'a ResolvedPackages,
    pub http_client: &'a ThrottledClient,
    pub http_client_arc: std::sync::Arc<ThrottledClient>,
    pub config: &'static Config,
    pub manifest: &'a mut PackageManifest,
    pub lockfile: Option<&'a Lockfile>,
    pub lockfile_path: Option<&'a std::path::Path>,
    /// The manifest group(s) the added packages are saved into. `None`
    /// means pnpm's default: an already-declared package is updated in
    /// the group it occupies (`guessDependencyType` — checked in
    /// `optionalDependencies`, `dependencies`, `devDependencies`,
    /// `peerDependencies` order, with a peer-only entry left untouched),
    /// and a new package lands in `dependencies`.
    pub dependency_groups: Option<DependencyGroupList>,
    /// Package selectors, each of which may carry an `@<version>` suffix.
    pub package_names: &'a [String],
    /// How the freshly-resolved version is pinned into the manifest range,
    /// derived from `--save-exact` / `--save-prefix`. See
    /// [`PinnedVersion::from_save_options`].
    // TODO: read `save-exact` / `save-prefix` from `.npmrc`, merge configs, and derive this there.
    pub pinned_version: PinnedVersion,
    /// `--save-catalog-name=<name>` (with `--save-catalog` a shorthand for
    /// `default`), or the `saveCatalogName` config default. When `Some`,
    /// the added dependency is written as `catalog:` / `catalog:<name>`
    /// and recorded in `pnpm-workspace.yaml` even under
    /// [`pacquet_config::CatalogMode::Manual`].
    pub save_catalog_name: Option<String>,
    /// CLI-merged `supportedArchitectures` forwarded to the
    /// `Install` run that follows the manifest mutation. See
    /// [`Install::supported_architectures`].
    pub supported_architectures: Option<pacquet_package_is_installable::SupportedArchitectures>,
    /// `--lockfile-only`: add the dependency to the manifest and write
    /// `pnpm-lock.yaml`, but skip materializing `node_modules`. Forwarded
    /// to the follow-up `Install` run. See [`Install::lockfile_only`].
    pub lockfile_only: bool,
}

/// Error type of [`Add`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum AddError {
    #[display("Failed to add package to manifest: {_0}")]
    AddDependencyToManifest(#[error(source)] PackageManifestError),
    #[display("Failed to save the manifest file: {_0}")]
    SaveManifest(#[error(source)] PackageManifestError),

    /// Locating the workspace root (to read `pnpm-workspace.yaml`'s
    /// catalogs) failed while applying `catalogMode`.
    #[diagnostic(transparent)]
    FindWorkspaceDir(#[error(source)] pacquet_workspace::FindWorkspaceDirError),

    /// Reading `pnpm-workspace.yaml` failed while applying `catalogMode`.
    #[diagnostic(transparent)]
    ReadWorkspaceManifest(#[error(source)] pacquet_workspace::ReadWorkspaceManifestError),

    /// `pnpm-workspace.yaml`'s catalog sections are misconfigured.
    #[diagnostic(transparent)]
    InvalidCatalogsConfiguration(#[error(source)] InvalidCatalogsConfigurationError),

    /// `catalogMode: strict` and the added version disagreed with the
    /// catalog entry for that package.
    #[diagnostic(transparent)]
    CatalogVersionMismatch(#[error(source)] CatalogVersionMismatchError),

    /// Writing the auto-cataloged entry back to `pnpm-workspace.yaml`
    /// (or the `cleanupUnusedCatalogs` pass it runs) failed.
    #[diagnostic(transparent)]
    WriteWorkspaceManifest(#[error(source)] WriteWorkspaceCatalogsError),

    #[diagnostic(transparent)]
    Install(#[error(source)] InstallError),

    /// Resolving a brand-new dependency's `latest` tag against the registry
    /// failed while computing the version to add.
    #[display("Failed to resolve the latest version of {name}: {error}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_ADD_RESOLVE_LATEST))]
    ResolveLatest {
        name: String,
        #[error(source)]
        error: crate::resolve_latest::ResolveLatestError,
    },

    /// Resolving an explicit `add <name>@<spec>` specifier against the
    /// registry (to pin the manifest range to a concrete version) failed.
    #[diagnostic(transparent)]
    ResolveSpec(#[error(source)] Box<PickPackageError>),

    #[display("Failed to resolve git dependency {specifier:?}: {source}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_ADD_RESOLVE_GIT))]
    ResolveGit {
        specifier: String,
        #[error(source)]
        source: pacquet_resolving_resolver_base::ResolveError,
    },

    #[display("Could not determine the package name of git dependency {specifier:?}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_ADD_GIT_PACKAGE_NAME))]
    GitPackageName { specifier: String },

    /// Resolving a `node@runtime:<spec>` selector against the Node.js
    /// release index (to pin the manifest to the picked version) failed.
    #[diagnostic(transparent)]
    ResolveRuntimeSpec(#[error(source)] NodeResolverError),

    /// `minimumReleaseAgeExclude` contained an invalid rule.
    #[display("Invalid value in minimumReleaseAgeExclude: {_0}")]
    #[diagnostic(code(ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE_EXCLUDE))]
    MinimumReleaseAgeExclude(#[error(source)] pacquet_config::version_policy::VersionPolicyError),
}

impl<DependencyGroupList> Add<'_, DependencyGroupList>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    pub async fn run<Reporter: self::Reporter + 'static>(self) -> Result<(), AddError> {
        let Add {
            tarball_mem_cache,
            http_client,
            http_client_arc,
            config,
            manifest,
            lockfile,
            lockfile_path,
            dependency_groups,
            package_names,
            pinned_version,
            save_catalog_name,
            resolved_packages,
            supported_architectures,
            lockfile_only,
        } = self;
        let dependency_groups: Option<Vec<DependencyGroup>> =
            dependency_groups.map(|groups| groups.into_iter().collect());

        let latest_picker = tokio::sync::OnceCell::new();
        let meta_cache = std::sync::Arc::new(InMemoryPackageMetaCache::default());
        let fetch_locker = shared_packument_fetch_locker();
        let catalog_ctx = read_catalog_ctx(manifest, config)?;
        let updated_catalogs = prepare_manifest::<Reporter>(
            manifest,
            http_client,
            &http_client_arc,
            config,
            lockfile,
            dependency_groups.as_deref(),
            package_names,
            &latest_picker,
            pinned_version,
            save_catalog_name.as_deref(),
            &catalog_ctx.catalogs,
            &catalog_ctx.prefix,
            &meta_cache,
            &fetch_locker,
        )
        .await?;

        // Write the new catalog entry to `pnpm-workspace.yaml` before the
        // install so the resolver reads it back and the lockfile's
        // `catalogs:` snapshot records the resolved version. The same
        // write runs the `cleanupUnusedCatalogs` pass when configured.
        write_workspace_catalogs(
            config,
            Some(&catalog_ctx.workspace_dir),
            &updated_catalogs,
            manifest,
        )
        .map_err(AddError::WriteWorkspaceManifest)?;
        let catalogs_override = (!updated_catalogs.is_empty()).then(|| {
            let mut catalogs = catalog_ctx.catalogs;
            merge_catalogs(&mut catalogs, &updated_catalogs);
            catalogs
        });

        Install {
            tarball_mem_cache,
            http_client,
            http_client_arc,
            config,
            manifest,
            emit_initial_manifest: false,
            lockfile: MaybeLazyLockfile::Loaded(lockfile),
            lockfile_path,
            // `dependency_groups` names the manifest group the new
            // package is saved into (`prepare_manifest` above), not an
            // include filter: like `remove`, the re-resolve walks every
            // dependency group so the other groups' entries stay in the
            // lockfile, the virtual store, and `node_modules`.
            dependency_groups: DIRECT_GROUPS,
            frozen_lockfile: false,
            // `pacquet add` mutates the manifest, so the lockfile is
            // necessarily stale by the time the install dispatch
            // runs — short-circuit the prefer-frozen fast path so we
            // always re-resolve. `None` would fall back to
            // `config.prefer_frozen_lockfile`, which is `true` by
            // default and the dispatch would discover the staleness
            // anyway; explicit `Some(false)` keeps `pacquet add`
            // behaviour self-evident at the call site.
            prefer_frozen_lockfile: Some(false),
            ignore_manifest_check: false,
            skip_runtimes: config.skip_runtimes,
            trust_lockfile: config.trust_lockfile,
            update_checksums: false,
            // `pacquet add` is a partial install, so the root project's
            // own lifecycle scripts must not run — they fire only for a
            // full install.
            is_full_install: false,
            installs_only: false,
            resolved_packages,
            supported_architectures,
            node_linker: config.node_linker,
            lockfile_only,
            dry_run: false,
            // `add` keeps every lockfile pin; the freshly-added range
            // is the only thing that re-resolves. `update`'s bump is a
            // separate operation.
            update_seed_policy: UpdateSeedPolicy::KeepAll,
            auth_override: None,
            resolution_observer: None,
            peer_issues_sink: None,
            catalogs_override,
            disable_optimistic_repeat_install: false,
            pnpmfile_hook_override: None,
            workspace_projects_override: None,
        }
        .run::<Reporter>()
        .await
        .map_err(AddError::Install)?;

        persist_manifest::<Reporter>(manifest)?;

        Ok(())
    }

    pub async fn run_selected<Reporter: self::Reporter + 'static>(
        self,
        projects: &mut [pacquet_workspace::Project],
        ordered_dirs: &[PathBuf],
        selected_dirs: &HashSet<PathBuf>,
        active_manifest_is_standin: bool,
    ) -> Result<(), AddError> {
        let Add {
            tarball_mem_cache,
            http_client,
            http_client_arc,
            config,
            manifest,
            lockfile,
            lockfile_path,
            dependency_groups,
            package_names,
            pinned_version,
            save_catalog_name,
            resolved_packages,
            supported_architectures,
            lockfile_only,
        } = self;
        let dependency_groups: Option<Vec<DependencyGroup>> =
            dependency_groups.map(|groups| groups.into_iter().collect());
        let selected_indices = selected_project_indices(projects, ordered_dirs, selected_dirs);
        if selected_indices.is_empty() {
            return Ok(());
        }
        let prepared = prepare_selected_manifests::<Reporter>(
            projects,
            &selected_indices,
            http_client,
            &http_client_arc,
            config,
            lockfile,
            dependency_groups.as_deref(),
            package_names,
            pinned_version,
            save_catalog_name.as_deref(),
        )
        .await?;
        write_workspace_catalogs_selected(
            config,
            &prepared.workspace_dir,
            &prepared.updated_catalogs,
            projects,
        )
        .map_err(AddError::WriteWorkspaceManifest)?;

        Box::pin(
            Install {
                tarball_mem_cache,
                http_client,
                http_client_arc,
                config,
                manifest,
                emit_initial_manifest: false,
                lockfile: MaybeLazyLockfile::Loaded(lockfile),
                lockfile_path,
                // See the `dependency_groups` comment in [`Self::run`]:
                // the save target must not narrow the install's include
                // set.
                dependency_groups: DIRECT_GROUPS,
                frozen_lockfile: false,
                prefer_frozen_lockfile: Some(false),
                ignore_manifest_check: false,
                skip_runtimes: config.skip_runtimes,
                trust_lockfile: config.trust_lockfile,
                update_checksums: false,
                is_full_install: false,
                installs_only: false,
                resolved_packages,
                supported_architectures,
                node_linker: config.node_linker,
                lockfile_only,
                dry_run: false,
                update_seed_policy: UpdateSeedPolicy::KeepAll,
                auth_override: None,
                resolution_observer: None,
                peer_issues_sink: None,
                catalogs_override: prepared.catalogs_override,
                disable_optimistic_repeat_install: false,
                pnpmfile_hook_override: None,
                workspace_projects_override: None,
            }
            .run_selected::<Reporter>(WorkspaceInstallSelection {
                all_projects: projects,
                ordered_dirs,
                selected_dirs,
                active_manifest_is_standin,
            }),
        )
        .await
        .map_err(AddError::Install)?;

        persist_selected_manifests::<Reporter>(projects, &selected_indices)?;
        Ok(())
    }
}

struct AddCatalogCtx {
    catalogs: Catalogs,
    workspace_dir: PathBuf,
    prefix: String,
}

struct SelectedAddPreparation {
    updated_catalogs: Catalogs,
    catalogs_override: Option<Catalogs>,
    workspace_dir: PathBuf,
}

#[expect(
    clippy::too_many_arguments,
    reason = "selected add preparation reuses the command's resolution inputs"
)]
async fn prepare_selected_manifests<Reporter: self::Reporter>(
    projects: &mut [pacquet_workspace::Project],
    selected_indices: &[usize],
    http_client: &ThrottledClient,
    http_client_arc: &std::sync::Arc<ThrottledClient>,
    config: &'static Config,
    lockfile: Option<&Lockfile>,
    dependency_groups: Option<&[DependencyGroup]>,
    package_names: &[String],
    pinned_version: PinnedVersion,
    save_catalog_name: Option<&str>,
) -> Result<SelectedAddPreparation, AddError> {
    let first_index = *selected_indices.first().expect("selected add requires a project");
    let catalog_ctx = read_catalog_ctx(&projects[first_index].manifest, config)?;
    let mut catalogs = catalog_ctx.catalogs;
    let mut updated_catalogs = Catalogs::new();
    // One picker, packument cache, and fetch locker across every selected
    // project: the picker is created on first use (a selection that resolves
    // no `latest` tag never builds one), and the shared caches keep the same
    // package from being fetched once per project.
    let latest_picker = tokio::sync::OnceCell::new();
    let meta_cache = std::sync::Arc::new(InMemoryPackageMetaCache::default());
    let fetch_locker = shared_packument_fetch_locker();

    for &index in selected_indices {
        let updates = prepare_manifest::<Reporter>(
            &mut projects[index].manifest,
            http_client,
            http_client_arc,
            config,
            lockfile,
            dependency_groups,
            package_names,
            &latest_picker,
            pinned_version,
            save_catalog_name,
            &catalogs,
            &catalog_ctx.prefix,
            &meta_cache,
            &fetch_locker,
        )
        .await?;
        merge_catalogs(&mut catalogs, &updates);
        merge_catalogs(&mut updated_catalogs, &updates);
    }

    let catalogs_override = (!updated_catalogs.is_empty()).then_some(catalogs);
    Ok(SelectedAddPreparation {
        updated_catalogs,
        catalogs_override,
        workspace_dir: catalog_ctx.workspace_dir,
    })
}

/// Resolve every selector against `catalogs` concurrently, then apply them
/// to `manifest`.
///
/// Every selector is resolved before the manifest is touched so the
/// `initial` manifest event still reports the pre-add shape exactly once,
/// however many selectors a single `add` carries. `FuturesOrdered` overlaps
/// the registry requests while keeping the buffered catalog warnings and the
/// applied dependencies in selector order. The `latest_picker`,
/// `meta_cache`, and `fetch_locker` are threaded in so one selected-add pass
/// shares packument state across every project it touches.
/// The manifest group `name` already occupies, in pnpm's
/// `guessDependencyType` scan order.
fn guess_dependency_group(manifest: &PackageManifest, name: &str) -> Option<DependencyGroup> {
    [DependencyGroup::Optional, DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Peer]
        .into_iter()
        .find(|&group| manifest.dependencies([group]).any(|(dep, _)| dep == name))
}

#[expect(
    clippy::too_many_arguments,
    reason = "manifest preparation consumes the add command's resolution inputs"
)]
async fn prepare_manifest<'a, Reporter: self::Reporter>(
    manifest: &mut PackageManifest,
    http_client: &'a ThrottledClient,
    http_client_arc: &std::sync::Arc<ThrottledClient>,
    config: &'static Config,
    lockfile: Option<&Lockfile>,
    dependency_groups: Option<&[DependencyGroup]>,
    package_names: &[String],
    latest_picker: &tokio::sync::OnceCell<LatestPicker<'a>>,
    pinned_version: PinnedVersion,
    save_catalog_name: Option<&str>,
    catalogs: &Catalogs,
    prefix: &str,
    meta_cache: &std::sync::Arc<InMemoryPackageMetaCache>,
    fetch_locker: &PackumentFetchLocker,
) -> Result<Catalogs, AddError> {
    let resolved_dependencies = {
        let mut resolution_futures = FuturesOrdered::new();
        for package_selector in package_names {
            resolution_futures.push_back(resolve_added_dependency(
                package_selector,
                config,
                manifest,
                lockfile,
                http_client,
                http_client_arc,
                latest_picker,
                pinned_version,
                save_catalog_name,
                catalogs,
                prefix,
                meta_cache,
                fetch_locker,
            ));
        }
        let mut dependencies = Vec::with_capacity(package_names.len());
        while let Some(result) = resolution_futures.next().await {
            let dependency = result?;
            if let Some(warning) = &dependency.warning {
                Reporter::emit(warning);
            }
            dependencies.push(dependency);
        }
        dependencies
    };

    emit_initial_package_manifest::<Reporter>(manifest);

    for dependency in &resolved_dependencies {
        let inferred;
        let groups: &[DependencyGroup] = match dependency_groups {
            Some(groups) => groups,
            // pnpm's `guessDependencyType`: keep an already-declared
            // package in its group; a peer-only entry stays untouched
            // (the install still resolves it); a new package lands in
            // `dependencies`.
            None => match guess_dependency_group(manifest, &dependency.package_name) {
                Some(DependencyGroup::Peer) => &[],
                Some(group) => {
                    inferred = [group];
                    &inferred
                }
                None => &[DependencyGroup::Prod],
            },
        };
        for &dependency_group in groups {
            manifest
                .add_dependency(
                    &dependency.package_name,
                    &dependency.manifest_specifier,
                    dependency_group,
                )
                .map_err(AddError::AddDependencyToManifest)?;
        }
    }

    let mut updated_catalogs = Catalogs::new();
    for dependency in resolved_dependencies {
        merge_catalogs(&mut updated_catalogs, &dependency.updated_catalogs);
    }
    Ok(updated_catalogs)
}

fn read_catalog_ctx(
    manifest: &PackageManifest,
    config: &Config,
) -> Result<AddCatalogCtx, AddError> {
    let manifest_dir =
        manifest.path().parent().expect("manifest path always has a parent dir").to_path_buf();
    let workspace_dir_opt =
        pacquet_workspace::find_workspace_dir(&manifest_dir).map_err(AddError::FindWorkspaceDir)?;
    let catalogs = if let Some(catalogs) = config.catalogs.clone() {
        catalogs
    } else {
        let workspace_manifest = match workspace_dir_opt.as_deref() {
            Some(dir) => pacquet_workspace::read_workspace_manifest(dir)
                .map_err(AddError::ReadWorkspaceManifest)?,
            None => None,
        };
        get_catalogs_from_workspace_manifest(workspace_manifest.as_ref())
            .map_err(AddError::InvalidCatalogsConfiguration)?
    };
    let workspace_dir = workspace_dir_opt.unwrap_or(manifest_dir);
    let prefix = workspace_dir.to_string_lossy().into_owned();
    Ok(AddCatalogCtx { catalogs, workspace_dir, prefix })
}

fn merge_catalogs(target: &mut Catalogs, updates: &Catalogs) {
    for (catalog_name, entries) in updates {
        let catalog = target.entry(catalog_name.clone()).or_default();
        for (dependency, specifier) in entries {
            catalog.insert(dependency.clone(), specifier.clone());
        }
    }
}

fn persist_selected_manifests<Reporter: self::Reporter>(
    projects: &mut [pacquet_workspace::Project],
    selected_indices: &[usize],
) -> Result<(), AddError> {
    for &index in selected_indices {
        persist_manifest::<Reporter>(&mut projects[index].manifest)?;
    }
    Ok(())
}

fn persist_manifest<Reporter: self::Reporter>(
    manifest: &mut PackageManifest,
) -> Result<(), AddError> {
    let updated = manifest.save_and_get_written_value().map_err(AddError::SaveManifest)?;
    let prefix = package_manifest_prefix(manifest);
    Reporter::emit(&LogEvent::PackageManifest(PackageManifestLog {
        level: LogLevel::Debug,
        message: PackageManifestMessage::Updated { prefix, updated },
    }));
    Ok(())
}

struct ResolvedAddedDependency {
    package_name: String,
    manifest_specifier: String,
    updated_catalogs: Catalogs,
    warning: Option<LogEvent>,
}

#[expect(
    clippy::too_many_arguments,
    reason = "resolving an add selector requires the shared resolution inputs"
)]
async fn resolve_added_dependency<'a>(
    package_selector: &str,
    config: &'static Config,
    manifest: &PackageManifest,
    lockfile: Option<&Lockfile>,
    http_client: &'a ThrottledClient,
    http_client_arc: &std::sync::Arc<ThrottledClient>,
    latest_picker: &tokio::sync::OnceCell<LatestPicker<'a>>,
    pinned_version: PinnedVersion,
    save_catalog_name: Option<&str>,
    catalogs: &Catalogs,
    prefix: &str,
    meta_cache: &std::sync::Arc<InMemoryPackageMetaCache>,
    fetch_locker: &PackumentFetchLocker,
) -> Result<ResolvedAddedDependency, AddError> {
    let parsed =
        pacquet_resolving_parse_wanted_dependency::parse_wanted_dependency(package_selector);
    let aliasless_git = match (parsed.alias.as_deref(), parsed.bare_specifier.as_deref()) {
        (None, Some(specifier))
            if pacquet_resolving_git_resolver::parse_bare_specifier(specifier).is_some() =>
        {
            Some(resolve_aliasless_git(specifier, config, http_client_arc).await?)
        }
        _ => None,
    };
    let (package_name, explicit_spec) = match aliasless_git.as_ref() {
        Some(git) => (git.package_name.as_str(), Some(git.manifest_specifier.as_str())),
        None => split_name_spec(package_selector),
    };

    // The dependency's current specifier, so a re-add keeps the
    // existing range / `catalog:` reference rather than re-pinning to
    // `^<latest>`. The scan order matches pnpm's `findSpec` /
    // `guessDependencyType` (`DEPENDENCIES_OR_PEER_FIELDS`):
    // `optionalDependencies`, `dependencies`, `devDependencies`,
    // `peerDependencies` — the first-found specifier wins even when the
    // add targets a different group ([`PackageManifest::add_dependency`]
    // then removes the entry from its old group).
    let prev_specifier = manifest
        .dependencies([
            DependencyGroup::Optional,
            DependencyGroup::Prod,
            DependencyGroup::Dev,
            DependencyGroup::Peer,
        ])
        .find(|(name, _)| *name == package_name)
        .map(|(_, spec)| spec.to_string());

    // The bare specifier to reconcile against the catalogs:
    // - an explicit `@<version>` is resolved to a concrete version and
    //   recorded with the range operator it (or the existing entry)
    //   pins — `pnpm add foo@^7` records `^7.8.4`, not
    //   `^7`. Specifiers that aren't a plain registry range/tag/version
    //   for this package (protocols, `npm:` aliases) stay verbatim;
    // - an explicit `node@runtime:<spec>` is likewise pinned to the
    //   picked Node.js version, so the `devEngines.runtime` entry the
    //   saved dependency folds into records e.g. `26.5.0`, not the
    //   requested `26`;
    // - a re-add with no version keeps the dependency's current
    //   specifier verbatim (a `catalog:` reference, a range, or an
    //   exact pin) — `pnpm add <existing>` without a
    //   version leaves the declared range untouched;
    // - a brand-new dependency fetches and pins the `latest` range.
    let bare_specifier =
        if let Some(version_spec) = node_runtime_version_spec(package_name, explicit_spec) {
            let mut node_resolver = NodeResolver::new(std::sync::Arc::clone(http_client_arc));
            node_resolver.node_download_mirrors.clone_from(&config.node_download_mirrors);
            node_resolver.offline = config.offline;
            node_resolver
                .resolve_save_specifier(version_spec, prev_specifier.as_deref())
                .await
                .map_err(AddError::ResolveRuntimeSpec)?
        } else {
            match (explicit_spec, prev_specifier.as_deref()) {
                (Some(spec), prev) => resolve_explicit_registry_spec(
                    package_name,
                    spec,
                    prev,
                    config,
                    http_client,
                    pinned_version,
                    lockfile,
                    manifest,
                    meta_cache,
                    fetch_locker,
                )
                .await?
                .unwrap_or_else(|| normalized_save_specifier(spec)),
                (None, Some(prev)) => prev.to_string(),
                (None, None) => {
                    let latest = latest_picker
                        .get_or_try_init(|| {
                            std::future::ready(
                                PickPolicy::from_config(config)
                                    .map(|policy| {
                                        LatestPicker::new(
                                            config,
                                            http_client,
                                            policy,
                                            std::sync::Arc::clone(meta_cache),
                                            std::sync::Arc::clone(fetch_locker),
                                        )
                                    })
                                    .map_err(AddError::MinimumReleaseAgeExclude),
                            )
                        })
                        .await?
                        .resolve(package_name, false)
                        .await
                        .map_err(|error| AddError::ResolveLatest {
                            name: package_name.to_string(),
                            error,
                        })?;
                    latest.serialize(pinned_version)
                }
            }
        };

    let mut updated_catalogs = Catalogs::new();
    let dep = CatalogModeDep {
        alias: package_name,
        bare_specifier: &bare_specifier,
        prev_specifier: prev_specifier.as_deref(),
    };
    let outcome =
        decide_catalog_outcome(config.catalog_mode, save_catalog_name, catalogs, &dep, prefix)
            .map_err(AddError::CatalogVersionMismatch)?;
    let manifest_specifier = match outcome.decision {
        CatalogDecision::KeepDirect => bare_specifier,
        CatalogDecision::Catalog { manifest_specifier, updated_entry } => {
            if let Some(entry) = updated_entry {
                updated_catalogs
                    .entry(entry.catalog_name)
                    .or_default()
                    .insert(package_name.to_string(), entry.specifier);
            }
            manifest_specifier
        }
    };

    Ok(ResolvedAddedDependency {
        package_name: package_name.to_string(),
        manifest_specifier,
        updated_catalogs,
        warning: outcome.warning,
    })
}

struct AliaslessGitDependency {
    package_name: String,
    manifest_specifier: String,
}

async fn resolve_aliasless_git(
    specifier: &str,
    config: &'static Config,
    http_client: &Arc<ThrottledClient>,
) -> Result<AliaslessGitDependency, AddError> {
    let resolver = GitResolver::new(
        Arc::new(RealGitProbe::new(Arc::clone(http_client))),
        Arc::new(RealGitRunner::new()),
    )
    .with_fetch_context(GitFetchContext {
        http_client: Arc::clone(http_client),
        store_dir: &config.store_dir,
        store_index_writer: None,
        auth_headers: Arc::clone(&config.auth_headers),
        retry_opts: crate::retry_config::retry_opts_from_config(config),
        git_shallow_hosts: config.git_shallow_hosts.clone(),
    });
    let wanted = pacquet_resolving_resolver_base::WantedDependency {
        bare_specifier: Some(specifier.to_string()),
        ..pacquet_resolving_resolver_base::WantedDependency::default()
    };
    let result = pacquet_resolving_resolver_base::Resolver::resolve(
        &resolver,
        &wanted,
        &pacquet_resolving_resolver_base::ResolveOptions::default(),
    )
    .await
    .map_err(|source| AddError::ResolveGit { specifier: specifier.to_string(), source })?
    .expect("a specifier accepted by the git parser is resolved by GitResolver");
    let package_name = result
        .manifest
        .as_ref()
        .and_then(|manifest| manifest.get("name"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .or_else(|| HostedGit::from_url(specifier).map(|hosted| hosted.project))
        .ok_or_else(|| AddError::GitPackageName { specifier: specifier.to_string() })?;
    let manifest_specifier =
        result.normalized_bare_specifier.unwrap_or_else(|| normalized_save_specifier(specifier));
    Ok(AliaslessGitDependency { package_name, manifest_specifier })
}

/// Resolve an explicit `add <name>@<spec>` registry specifier to the
/// manifest range pnpm would record: the spec resolved to a concrete
/// version (through the *same* resolver path the follow-up install uses, so
/// the pinned version equals the version the install locks — `resolutionMode`
/// and `minimumReleaseAge` included), carrying the operator the existing
/// entry pins, then the spec's, then the configured default. So `pnpm add
/// foo@^7` records `^7.8.4`, not `^7`.
///
/// Returns `Ok(None)` — write the specifier verbatim — for anything that is
/// not a plain registry range/tag/version for `package_name` itself:
/// non-registry protocols (`git:`/`file:`/`workspace:`/URLs, which
/// [`parse_bare_specifier`] rejects), `npm:` aliases (resolving them risks
/// dropping the aliased target), and specifiers that resolve to no version.
#[expect(
    clippy::too_many_arguments,
    reason = "a resolve helper threading the install's resolution inputs"
)]
async fn resolve_explicit_registry_spec(
    package_name: &str,
    spec: &str,
    prev_specifier: Option<&str>,
    config: &Config,
    http_client: &ThrottledClient,
    pinned_version: PinnedVersion,
    lockfile: Option<&Lockfile>,
    manifest: &PackageManifest,
    meta_cache: &InMemoryPackageMetaCache,
    fetch_locker: &PackumentFetchLocker,
) -> Result<Option<String>, AddError> {
    if spec.starts_with("npm:") {
        return Ok(None);
    }
    let registries: std::collections::HashMap<String, String> =
        config.resolved_registries().into_iter().collect();
    let registry = pick_registry_for_package(&registries, package_name, None);
    let Some(spec_parsed) = parse_bare_specifier(spec, Some(package_name), "latest", &registry)
    else {
        return Ok(None);
    };
    // A registry-host tarball URL parses as a registry `Version` spec but
    // must stay verbatim — resolving it would rewrite an explicit URL
    // dependency into a semver range. The npm resolver marks such parses
    // with `normalized_bare_specifier`.
    if spec_parsed.normalized_bare_specifier.is_some() {
        return Ok(None);
    }
    if spec_parsed.name != package_name {
        return Ok(None);
    }

    let policy = PickPolicy::from_config(config).map_err(AddError::MinimumReleaseAgeExclude)?;
    // Bias the pick toward versions already present in the workspace, so a
    // dedup pick matches what the install locks (e.g. a sibling already on
    // `1.2.0` keeps `pnpm add foo@^1` on `1.2.0`). Seeded from the wanted
    // lockfile + this manifest; sibling manifests aren't reachable here, so
    // an unlocked sibling declaration may still differ — never an
    // inconsistency, since the install resolves the rewritten range.
    let preferred_versions = get_preferred_versions_from_lockfile_and_manifests(
        lockfile.and_then(|lockfile| lockfile.snapshots.as_ref()),
        &[manifest],
    );
    let ctx = pick_package_context(http_client, config, &policy, meta_cache, fetch_locker);
    let opts = PickPackageOptions {
        registry: &registry,
        preferred_version_selectors: preferred_versions.get(package_name),
        published_by: policy.published_by,
        published_by_exclude: policy.published_by_exclude.as_ref(),
        pick_lowest_version: policy.pick_lowest_direct,
        // `false`: the explicit spec is authoritative. The highest version
        // satisfying the spec is already the `latest`-tag version whenever
        // `latest` satisfies it; forcing the `latest` tag in would wrongly
        // bump a narrower spec (`~7.0.0`, `7.0.0`) past its own bound.
        include_latest_tag: false,
        dry_run: false,
        optional: false,
        update_checksums: false,
        blocked_versions: None,
    };

    let pick = pick_package(&ctx, &spec_parsed, &opts)
        .await
        .map_err(|error| AddError::ResolveSpec(Box::new(error)))?;
    let Some(picked) = pick.picked_package else {
        return Ok(None);
    };

    // Specifier-operator precedence: the existing entry's operator wins
    // over the spec's, which wins over the configured default. Only a
    // registry-style previous specifier carries a meaningful operator —
    // `which_version_is_pinned` scans for a version anywhere in the spec, so a
    // path/URL prev (e.g. `file:../deps/2.0.0.tgz`) would otherwise be misread
    // as a pin. Gate it on `parse_bare_specifier` accepting a non-URL spec.
    let prev_pin = prev_specifier
        .filter(|prev| is_registry_style_specifier(prev, package_name, &registry))
        .and_then(which_version_is_pinned);
    let pin = prev_pin.or_else(|| which_version_is_pinned(spec)).unwrap_or(pinned_version);
    Ok(Some(picked.serialize(pin)))
}

/// Whether `specifier` is a plain registry range/tag/version for
/// `package_name` (not a non-registry protocol, path, or tarball URL), and
/// so carries a meaningful range operator.
fn is_registry_style_specifier(specifier: &str, package_name: &str, registry: &str) -> bool {
    parse_bare_specifier(specifier, Some(package_name), "latest", registry)
        .is_some_and(|parsed| parsed.normalized_bare_specifier.is_none())
}

/// Split a `pacquet add` argument into its package name and optional
/// `@<version>` part. The version separator is the first `@` at or after
/// index 1, so a leading scope `@` (`@scope/pkg`) is never mistaken for a
/// version.
fn split_name_spec(input: &str) -> (&str, Option<&str>) {
    match input.get(1..).and_then(|rest| rest.find('@')).map(|offset| offset + 1) {
        Some(idx) => (&input[..idx], Some(&input[idx + 1..])),
        None => (input, None),
    }
}

/// The specifier `pacquet add <name>@<spec>` saves when `<spec>` isn't a plain
/// registry range. A hosted-git request — a bare `owner/repo#committish`
/// shorthand or a GitHub / GitLab / Bitbucket URL — is rewritten to its
/// `github:` / `gitlab:` / `bitbucket:` shortcut form. Everything else
/// (`file:`, `link:`, `workspace:`, `npm:` aliases, tarball URLs) is
/// kept verbatim.
///
/// An auth-bearing HTTPS URL (`git+https://<token>@github.com/...`) is also
/// kept verbatim: the shortcut form cannot carry userinfo, so shortcutting
/// would silently drop the credentials the follow-up install needs to reach a
/// private repo. This mirrors the git resolver, which keeps such URLs in a
/// `git+https` form rather than shortcutting them
/// (see `parse_bare_specifier`'s `hosted.auth.is_some()` branch).
fn normalized_save_specifier(spec: &str) -> String {
    match HostedGit::from_url(spec) {
        Some(hosted) if hosted.auth.is_none() => hosted.shortcut(HostedOpts::default()),
        _ => spec.to_string(),
    }
}

/// The `<spec>` half of an explicit `node@runtime:<spec>` request, when that
/// is what's being added. Only the node resolver pins the saved specifier to
/// the picked version; deno and bun normalize to the requested spec, so they
/// stay on the verbatim save path.
fn node_runtime_version_spec<'a>(
    package_name: &str,
    explicit_spec: Option<&'a str>,
) -> Option<&'a str> {
    if package_name != "node" {
        return None;
    }
    explicit_spec?.strip_prefix("runtime:")
}

#[cfg(test)]
mod tests;
