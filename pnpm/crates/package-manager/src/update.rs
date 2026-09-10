use crate::{
    CatalogDecision, CatalogModeDep, CatalogVersionMismatchError, DIRECT_GROUPS,
    ImporterUpdateSeedPolicy, Install, InstallError, ProjectMutation, ResolvedPackages,
    UpdateSeedPolicy, WorkspaceInstallSelection,
    catalog_cleanup::{
        WriteWorkspaceCatalogsError, post_install_prune, write_workspace_catalogs,
        write_workspace_catalogs_selected,
    },
    decide_catalog, defer_ignored_builds, emit_initial_package_manifest, included_direct_groups,
    manifest_spec_bumps::{ManifestSpecBumps, split_npm_alias},
    package_manifest_prefix,
    resolution_policy::{PickPolicy, create_configured_npm_resolver},
    selected_project_indices,
};
use chrono::{DateTime, Utc};
use derive_more::{Display, Error};
use miette::Diagnostic;
use node_semver::Version;
use pipe_trait::Pipe;
use pnpm_catalogs_config::{
    InvalidCatalogsConfigurationError, get_catalogs_from_workspace_manifest,
};
use pnpm_catalogs_protocol_parser::parse_catalog_protocol;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::{
    CatalogMode, Config, SaveWorkspaceProtocol, matcher::create_matcher,
    version_policy::PackageVersionPolicy,
};
use pnpm_engine_pm_yarn_resolver::YarnResolver;
use pnpm_engine_runtime_bun_resolver::BunResolver;
use pnpm_engine_runtime_deno_resolver::DenoResolver;
use pnpm_engine_runtime_node_resolver::NodeResolver;
use pnpm_lockfile::{Lockfile, MaybeLazyLockfile};
use pnpm_lockfile_preferred_versions::get_version_selector_type;
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::{DependencyGroup, PackageManifest, PackageManifestError};
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::{
    LogEvent, LogLevel, PackageManifestLog, PackageManifestMessage, PnpmLog, Reporter,
};
use pnpm_resolving_default_resolver::DefaultResolver;
use pnpm_resolving_deps_resolver::{UpdateDepth, UpdateTargets, VersionLine, real_package_name_of};
use pnpm_resolving_npm_resolver::{
    DeclaredSpecifiers, calc_specifier_for_workspace_dep, calc_version_range,
    infer_range_spec_style,
};
use pnpm_resolving_resolver_base::{
    PreferredVersions, ResolveOptions, Resolver, UpdateBehavior, VersionSelectorType,
    WantedDependency, WorkspacePackages, WorkspacePackagesByVersion,
};
use pnpm_tarball::MemCache;
use pnpm_workspace_range_resolver::resolve_workspace_range;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// Everything `pacquet update` (alias `up` / `upgrade`) does.
///
/// Runs on pacquet's always-fresh-resolve install path. Its behavior has
/// two halves:
///
/// * **Compatible bump** (no `--latest`): the matched names have their
///   lockfile pins withheld from the preferred-versions seed
///   ([`UpdateSeedPolicy`]) so the resolver re-picks the highest version
///   satisfying the manifest range, and each matched *direct* dependency's
///   declared range is moved onto the version the install settled on
///   ([`crate::ManifestSpecBumps`]).
/// * **`--latest`**: each matched *direct* dependency's `latest` tag is
///   fetched and written into `package.json` before resolving, since the
///   tag reaches past the declared range. The follow-up install then
///   resolves the new range.
/// * **`--workspace`** ([`Update::workspace_packages`]): each matched
///   direct dependency that a workspace project publishes is re-pointed
///   at the local copy through the `workspace:` protocol, with
///   `saveWorkspaceProtocol` deciding whether the linked version is
///   written out or only its range operator.
///
/// A compatible bump and `--latest` write the same way: the operator the
/// dependency already pinned wins over the configured default, a dist-tag or
/// a non-registry protocol is left alone, and a `catalog:` reference moves
/// the catalog entry rather than the manifest entry.
///
/// Selector handling:
/// bare-name selectors (`foo`, `@scope/bar-*`) with `depth > 0` and no
/// `--latest` match every package of that name **at any depth** (the
/// match is applied against the lockfile's package names); selectors
/// carrying a version (`foo@2`) or any selector under `--latest` match
/// only direct dependencies, and the version (or fetched latest) is
/// written into the manifest before resolving.
#[must_use]
pub struct Update<'a> {
    pub tarball_mem_cache: Arc<MemCache>,
    pub resolved_packages: &'a ResolvedPackages,
    pub http_client: &'a ThrottledClient,
    pub http_client_arc: Arc<ThrottledClient>,
    pub config: &'static Config,
    pub manifest: &'a mut PackageManifest,
    pub lockfile: Option<&'a Lockfile>,
    pub lockfile_path: Option<&'a std::path::Path>,
    /// Package selectors from the CLI (`foo`, `@scope/bar-*`, `foo@2`).
    /// Empty means "update every direct dependency in the included
    /// groups", matching `pnpm update` with no arguments.
    pub packages: &'a [String],
    /// `--latest` / `-L`: ignore the manifest range and bump matched
    /// direct dependencies to their `latest` dist-tag, rewriting
    /// `package.json`.
    pub latest: bool,
    /// `--patches`: refresh registry revisions while retaining every locked
    /// package version and leaving manifest specifiers unchanged.
    pub patches: bool,
    /// `--save-exact` / `-E`: write the resolved version without a range
    /// operator when rewriting the manifest under `--latest`. Only applies
    /// to dependencies whose current specifier has no recoverable pin; an
    /// existing `^`/`~`/exact range is preserved over this default.
    pub save_exact: bool,
    /// `--save` (default) / `--no-save`. When `false`, `package.json` on
    /// disk is left untouched, so its specifiers stay authoritative:
    /// `pnpm-lock.yaml` still updates, but only within the ranges the
    /// manifest keeps, since the importer entry has to keep satisfying the
    /// specifier it records. A requested version those ranges exclude is
    /// skipped, and `--latest` degrades to a compatible bump.
    pub save: bool,
    /// Dependency groups the update considers when choosing which direct
    /// dependencies to match, derived from
    /// `--prod` / `--dev` / `--no-optional`. Note: the *materialized*
    /// dependency set is always all three groups (the `node_modules`
    /// layout is unchanged); this only narrows the update scope.
    pub include_direct: Vec<DependencyGroup>,
    /// `--depth`: how deep into the dependency graph the update reaches.
    /// A node below the ceiling keeps its locked resolution even when its
    /// name is a target, so `0` updates direct dependencies only.
    /// `usize::MAX` stands in for the `Infinity` default.
    pub depth: usize,
    /// `--workspace`: what the workspace projects publish, as built by
    /// [`crate::build_workspace_packages_map`]. `Some` turns the update
    /// into a workspace-link update — the matched direct dependencies
    /// are re-pointed at the workspace copies through the `workspace:`
    /// protocol instead of the registry. `None` is a plain update.
    pub workspace_packages: Option<&'a WorkspacePackages>,
    /// CLI-merged `supportedArchitectures`, forwarded to the install.
    pub supported_architectures: Option<pnpm_package_is_installable::SupportedArchitectures>,
    /// `--lockfile-only`: re-resolve and rewrite `pnpm-lock.yaml` without
    /// materializing `node_modules`. Forwarded to the install.
    pub lockfile_only: bool,
    /// Sink notified for each resolved tarball package, and the source of
    /// the optional resolver-time [`PackageVersionGuard`]. `None` for a
    /// plain `pacquet update`; `pacquet audit --fix update` installs one
    /// whose guard rejects vulnerable versions so the resolver falls back
    /// to a safe one.
    ///
    /// [`PackageVersionGuard`]: pnpm_resolving_resolver_base::PackageVersionGuard
    pub resolution_observer: Option<Arc<dyn crate::ResolutionObserver>>,
}

/// Error type of [`Update`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum UpdateError {
    /// A path named by the `pnpmfile` setting is not on disk. pnpm reports the
    /// same code and message from `requireHooks`.
    #[diagnostic(code(ERR_PNPM_PNPMFILE_NOT_FOUND))]
    MissingPnpmfile(#[error(not(source))] pnpm_hooks::finder::MissingPnpmfileError),
    /// `--latest` was combined with a versioned selector (`foo@2`).
    #[display("Specs are not allowed to be used with --latest ({_0})")]
    #[diagnostic(code(ERR_PNPM_LATEST_WITH_SPEC))]
    LatestWithSpec(#[error(not(source))] String),

    /// Package selectors were given with `--depth 0` but none matched a
    /// direct dependency.
    #[display("None of the specified packages were found in the dependencies.")]
    #[diagnostic(code(ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES))]
    NoPackageInDependencies,

    /// A versioned selector named a package no selected project declares
    /// directly, so there is nowhere to record the requested version.
    #[display("{message}")]
    #[diagnostic(code(ERR_PNPM_UPDATE_VERSION_ON_INDIRECT_DEP), help("{hint}"))]
    UpdateVersionOnIndirectDep {
        #[error(not(source))]
        message: String,
        hint: String,
    },

    /// A `--workspace` selector named a dependency that no workspace
    /// project publishes.
    #[display(r#""{_0}" not found in the workspace"#)]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_PACKAGE_NOT_FOUND))]
    WorkspacePackageNotFound(#[error(not(source))] String),

    /// A resolver failed while computing the specifier `--latest` should
    /// write for a direct dependency.
    #[display("Failed to resolve the latest version of {name}: {error}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_UPDATE_RESOLVE_LATEST))]
    ResolveLatest {
        name: String,
        #[error(source)]
        error: pnpm_resolving_resolver_base::ResolveError,
    },

    /// A resolver failed while resolving the dist tag an explicit
    /// `<name>@<tag>` update selector named.
    #[display("Failed to resolve {name}@{tag}: {error}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_UPDATE_RESOLVE_TAG))]
    ResolveTag {
        name: String,
        tag: String,
        #[error(source)]
        error: pnpm_resolving_resolver_base::ResolveError,
    },

    /// A `named-registries` alias is misconfigured.
    #[diagnostic(transparent)]
    InvalidNamedRegistry(#[error(source)] pnpm_resolving_npm_resolver::MergeNamedRegistriesError),

    /// `minimumReleaseAgeExclude` contained an invalid rule.
    #[display("Invalid value in minimumReleaseAgeExclude: {_0}")]
    #[diagnostic(code(ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE_EXCLUDE))]
    MinimumReleaseAgeExclude(#[error(source)] pnpm_config::version_policy::VersionPolicyError),

    /// Locating the workspace root (to read `pnpm-workspace.yaml`'s
    /// catalogs) failed while applying `catalogMode`.
    #[diagnostic(transparent)]
    FindWorkspaceDir(#[error(source)] pnpm_workspace::FindWorkspaceDirError),

    /// Reading `pnpm-workspace.yaml` failed while applying `catalogMode`.
    #[diagnostic(transparent)]
    ReadWorkspaceManifest(#[error(source)] pnpm_workspace::ReadWorkspaceManifestError),

    /// `pnpm-workspace.yaml`'s catalog sections are misconfigured.
    #[diagnostic(transparent)]
    InvalidCatalogsConfiguration(#[error(source)] InvalidCatalogsConfigurationError),

    /// `catalogMode: strict` and an updated version disagreed with the
    /// catalog entry for that package.
    #[diagnostic(transparent)]
    CatalogVersionMismatch(#[error(source)] CatalogVersionMismatchError),

    /// Writing the auto-cataloged entries back to `pnpm-workspace.yaml`
    /// failed.
    #[diagnostic(transparent)]
    WriteWorkspaceManifest(#[error(source)] WriteWorkspaceCatalogsError),

    #[display("Failed to update the manifest: {_0}")]
    UpdateManifest(#[error(source)] PackageManifestError),

    #[display("Failed to save the manifest file: {_0}")]
    SaveManifest(#[error(source)] PackageManifestError),

    #[diagnostic(transparent)]
    Install(#[error(source)] InstallError),

    #[diagnostic(transparent)]
    MinimumReleaseAge(#[error(source)] crate::minimum_release_age::MinimumReleaseAgeError),
}

/// A CLI selector split into its name pattern and optional version part.
struct ParsedSelector {
    pattern: String,
    version: Option<String>,
}

fn parse_update_param(input: &str) -> ParsedSelector {
    let search_start = if input.starts_with('!') { 2 } else { 1 };
    let at_index = input
        .get(search_start..)
        .and_then(|rest| rest.find('@'))
        .map(|offset| offset + search_start);
    match at_index {
        Some(idx) => ParsedSelector {
            pattern: input[..idx].to_string(),
            version: Some(input[idx + 1..].to_string()),
        },
        None => ParsedSelector { pattern: input.to_string(), version: None },
    }
}

/// pnpm's mutation for an update: a full install of the projects it was
/// pointed at when the user named nothing, and `installSome` once the
/// update targets specific dependencies — either by selector or through
/// `--latest`, which expands to every direct dependency's spec.
///
/// `--workspace` does not enter into it: pnpm picks the mutation from the
/// selectors the user passed, so a selector-less workspace-link update
/// stays a full install.
fn update_mutation(packages: &[String], latest: bool) -> ProjectMutation {
    if packages.is_empty() && !latest {
        ProjectMutation::InstallSelected
    } else {
        ProjectMutation::InstallSome
    }
}

impl<'a> Update<'a> {
    /// Separate what every step reads from what one of them consumes, and
    /// the manifest the update rewrites.
    fn split(self) -> (UpdateView<'a>, UpdateOwned, &'a mut PackageManifest) {
        (
            UpdateView {
                resolved_packages: self.resolved_packages,
                http_client: self.http_client,
                config: self.config,
                lockfile: self.lockfile,
                lockfile_path: self.lockfile_path,
                packages: self.packages,
                latest: self.latest,
                patches: self.patches,
                save_exact: self.save_exact,
                save: self.save,
                depth: self.depth,
                workspace_packages: self.workspace_packages,
                lockfile_only: self.lockfile_only,
            },
            UpdateOwned {
                tarball_mem_cache: self.tarball_mem_cache,
                http_client_arc: self.http_client_arc,
                include_direct: self.include_direct,
                supported_architectures: self.supported_architectures,
                resolution_observer: self.resolution_observer,
            },
            self.manifest,
        )
    }

    pub async fn run<Reporter: self::Reporter + 'static>(self) -> Result<(), UpdateError> {
        let (update, owned, manifest) = self.split();
        begin::<Reporter>(update, &owned)?;
        let site = UpdateSite::find::<Reporter>(update, manifest)?;
        let unsaved = site.hook_update_manifest(update, manifest).await?;
        if !update.latest && update.depth > 0 {
            reject_versions_of_indirect_update_specs::<Reporter>(
                &parse_selectors(update.packages),
                &[manifest],
                &owned.include_direct,
                &package_manifest_prefix(manifest),
            )?;
        }
        let mut latest_chain = None;
        let Some(prepared) =
            prepare_manifest::<Reporter>(manifest, update, &owned, None, &mut latest_chain).await?
        else {
            return nothing_to_update(update.depth, update.packages, update.latest);
        };
        if update.save {
            write_workspace_catalogs(
                update.config,
                prepared.workspace_dir_for_catalogs.as_deref(),
                &prepared.updated_catalogs,
                manifest,
            )
            .map_err(UpdateError::WriteWorkspaceManifest)?;
        }
        let importer_id =
            pnpm_workspace::importer_id_from_root_dir(&site.workspace_root, manifest_dir(manifest));
        let bumps = (!prepared.bump_targets.is_empty()).then(|| ManifestSpecBumps {
            targets: BTreeMap::from([(importer_id.clone(), prepared.bump_targets)]),
            range_spec_style: RangeSpecStyle::from_save_options(update.save_exact, None),
            applied: Mutex::default(),
        });
        let ignored_builds = run_update_install::<Reporter, _>(
            update_install(
                update,
                owned,
                manifest,
                UpdateSeed {
                    policy: if update.patches {
                        UpdateSeedPolicy::RefreshRevisions
                    } else {
                        prepared.seed_policy
                    },
                    preferred_versions_override: prepared.preferred_versions_override,
                    catalogs_override: prepared.catalogs_override,
                },
                site.read_package_hook.as_ref(),
            ),
            unsaved,
            bumps.as_ref(),
        )
        .await?;

        let applied = bumps.map(|bumps| bumps.applied.into_inner().expect("never poisoned"));
        settle_update_manifest::<Reporter>(
            manifest,
            update.config,
            SettleUpdate {
                save: update.save,
                should_persist_manifest: prepared.persist_manifest,
                importer_id: &importer_id,
                applied: applied.as_ref(),
                workspace_dir_for_catalogs: prepared.workspace_dir_for_catalogs.as_deref(),
            },
        )?;

        if let Some(ignored_builds) = ignored_builds {
            return Err(UpdateError::Install(ignored_builds));
        }
        Ok(())
    }

    pub async fn run_selected<Reporter: self::Reporter + 'static>(
        self,
        selected: SelectedProjects<'_>,
    ) -> Result<(), UpdateError> {
        let (update, owned, manifest) = self.split();
        begin::<Reporter>(update, &owned)?;
        let selected_indices = selected_project_indices(
            selected.projects,
            selected.ordered_dirs,
            selected.selected_dirs,
        );
        if selected_indices.is_empty() {
            return Ok(());
        }
        let site = UpdateSite::find::<Reporter>(update, manifest)?;
        let unsaved = site
            .hook_selected_manifests(update, selected.projects, manifest, &selected_indices)
            .await?;
        let mut prepared = prepare_selected_manifests::<Reporter>(
            selected.projects,
            &selected_indices,
            &site.workspace_root,
            update,
            &owned,
        )
        .await?;
        if !prepared.any_work {
            return Ok(());
        }
        if update.save {
            write_workspace_catalogs_selected(
                update.config,
                site.catalogs_dir(prepared.workspace_dir_for_catalogs.as_deref()),
                &prepared.updated_catalogs,
                selected.projects,
            )
            .map_err(UpdateError::WriteWorkspaceManifest)?;
        }

        let bumps = (!prepared.bump_targets.is_empty()).then(|| ManifestSpecBumps {
            targets: std::mem::take(&mut prepared.bump_targets),
            range_spec_style: RangeSpecStyle::from_save_options(update.save_exact, None),
            applied: Mutex::default(),
        });
        let ignored_builds = run_selected_update_install::<Reporter, _>(
            update_install(
                update,
                owned,
                manifest,
                UpdateSeed {
                    policy: selected_seed_policy(
                        update.patches,
                        std::mem::take(&mut prepared.seed_policies),
                        update.depth,
                    ),
                    preferred_versions_override: std::mem::take(
                        &mut prepared.preferred_versions_override,
                    ),
                    catalogs_override: prepared.catalogs_override.take(),
                },
                site.read_package_hook.as_ref(),
            ),
            selected.selection(),
            unsaved,
            bumps.as_ref(),
        )
        .await?;

        settle_selected_update::<Reporter>(
            update,
            &site,
            selected.projects,
            manifest,
            prepared,
            bumps,
        )?;
        if let Some(ignored_builds) = ignored_builds {
            return Err(UpdateError::Install(ignored_builds));
        }
        Ok(())
    }
}

/// The update's borrowed and `Copy` inputs, as one value every step reads.
#[derive(Clone, Copy)]
struct UpdateView<'a> {
    resolved_packages: &'a ResolvedPackages,
    http_client: &'a ThrottledClient,
    config: &'static Config,
    lockfile: Option<&'a Lockfile>,
    lockfile_path: Option<&'a Path>,
    packages: &'a [String],
    latest: bool,
    patches: bool,
    save_exact: bool,
    save: bool,
    depth: usize,
    workspace_packages: Option<&'a WorkspacePackages>,
    lockfile_only: bool,
}

/// The update's owned inputs, consumed by the install it runs.
struct UpdateOwned {
    tarball_mem_cache: Arc<MemCache>,
    http_client_arc: Arc<ThrottledClient>,
    include_direct: Vec<DependencyGroup>,
    supported_architectures: Option<pnpm_package_is_installable::SupportedArchitectures>,
    resolution_observer: Option<Arc<dyn crate::ResolutionObserver>>,
}

/// The projects a recursive update runs over, and the selection that
/// narrows the install to them.
pub struct SelectedProjects<'s> {
    pub projects: &'s mut [pnpm_workspace::Project],
    pub project_dependencies: &'s indexmap::IndexMap<PathBuf, Vec<PathBuf>>,
    pub ordered_dirs: &'s [PathBuf],
    pub selected_dirs: &'s HashSet<PathBuf>,
    pub install_dirs: &'s HashSet<PathBuf>,
    pub active_manifest_is_standin: bool,
}

impl SelectedProjects<'_> {
    pub(crate) fn selection(&self) -> WorkspaceInstallSelection<'_> {
        WorkspaceInstallSelection {
            all_projects: self.projects,
            project_dependencies: self.project_dependencies,
            ordered_dirs: self.ordered_dirs,
            selected_dirs: self.selected_dirs,
            install_dirs: self.install_dirs,
            active_manifest_is_standin: self.active_manifest_is_standin,
            workspace_cycles: crate::PrecomputedWorkspaceCycles::Unknown,
        }
    }
}

/// Route the clients' warnings through the reporter and refuse a save the
/// strict minimum release age forbids, before anything is read.
fn begin<Reporter: self::Reporter>(
    update: UpdateView<'_>,
    owned: &UpdateOwned,
) -> Result<(), UpdateError> {
    update.http_client.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);
    owned.http_client_arc.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);
    crate::minimum_release_age::ensure_strict_minimum_release_age_can_save(
        update.config,
        update.save,
    )
    .map_err(UpdateError::MinimumReleaseAge)
}

fn manifest_dir(manifest: &PackageManifest) -> &Path {
    manifest.path().parent().expect("manifest path always has a parent dir")
}

fn parse_selectors(packages: &[String]) -> Vec<ParsedSelector> {
    packages.iter().map(|input| parse_update_param(input)).collect()
}

/// Where the update runs: the lockfile root, and the pnpmfile's
/// read-package hook when `--no-save` must show the resolver the manifests
/// as the hook rewrites them.
struct UpdateSite {
    workspace_root: PathBuf,
    read_package_hook: Option<ReadPackageHook>,
}

impl UpdateSite {
    fn find<Reporter: self::Reporter>(
        update: UpdateView<'_>,
        manifest: &PackageManifest,
    ) -> Result<Self, UpdateError> {
        let workspace_root =
            crate::install::lockfile_root_dir(update.config, manifest_dir(manifest))
                .map_err(UpdateError::FindWorkspaceDir)?;
        let read_package_hook = (!update.save && !update.config.ignore_pnpmfile)
            .then(|| update_read_package_hook::<Reporter>(&workspace_root, update.config))
            .transpose()?
            .flatten();
        Ok(Self { workspace_root, read_package_hook })
    }

    /// Where the workspace manifest's catalogs are written: the directory the
    /// preparation found them in, else the lockfile root.
    fn catalogs_dir<'d>(&'d self, prepared: Option<&'d Path>) -> &'d Path {
        prepared.unwrap_or(&self.workspace_root)
    }

    async fn hook_update_manifest(
        &self,
        update: UpdateView<'_>,
        manifest: &mut PackageManifest,
    ) -> Result<UnsavedManifests, UpdateError> {
        let mut hooked_paths = HashSet::new();
        if let Some((hook, log)) = self.read_package_hook.as_ref() {
            apply_read_package_hook_to_update_manifest(manifest, hook, log).await?;
            hooked_paths.insert(manifest.path().to_path_buf());
        }
        Ok(UnsavedManifests {
            hooked_paths,
            lockfile_specifiers: (!update.save)
                .then(|| vec![(manifest_dir(manifest).to_path_buf(), manifest.clone())]),
        })
    }

    async fn hook_selected_manifests(
        &self,
        update: UpdateView<'_>,
        projects: &mut [pnpm_workspace::Project],
        manifest: &mut PackageManifest,
        selected_indices: &[usize],
    ) -> Result<UnsavedManifests, UpdateError> {
        let mut hooked_paths = HashSet::new();
        if let Some((hook, log)) = self.read_package_hook.as_ref() {
            hook_selected_manifests(projects, manifest, hook, log, &mut hooked_paths).await?;
        }
        Ok(UnsavedManifests {
            hooked_paths,
            lockfile_specifiers: (!update.save).then(|| {
                selected_indices
                    .iter()
                    .map(|&index| {
                        (projects[index].root_dir.clone(), projects[index].manifest.clone())
                    })
                    .collect()
            }),
        })
    }
}

/// What `--no-save` hands the install: the manifest paths the read-package
/// hook already rewrote, and the on-disk manifests the lockfile's importer
/// specifiers come from.
struct UnsavedManifests {
    hooked_paths: HashSet<PathBuf>,
    lockfile_specifiers: Option<Vec<(PathBuf, PackageManifest)>>,
}

/// What the resolve seeds from: the pins it keeps or drops, the versions it
/// prefers, and the catalogs as the update rewrote them.
struct UpdateSeed {
    policy: UpdateSeedPolicy,
    preferred_versions_override: PreferredVersions,
    catalogs_override: Option<Catalogs>,
}

/// `include` is always all-true for updates: the materialized
/// `node_modules` layout must not change just because the
/// update scope was narrowed.
/// `update` always re-resolves against the registry, so the
/// auto-frozen / repeat-install fast paths must not fire.
fn update_install<'i>(
    update: UpdateView<'i>,
    owned: UpdateOwned,
    manifest: &'i PackageManifest,
    seed: UpdateSeed,
    read_package_hook: Option<&ReadPackageHook>,
) -> Install<'i, impl Iterator<Item = DependencyGroup>> {
    Install {
        tarball_mem_cache: owned.tarball_mem_cache,
        http_client: update.http_client,
        http_client_arc: owned.http_client_arc,
        config: update.config,
        manifest,
        emit_initial_manifest: false,
        lockfile: MaybeLazyLockfile::Loaded(update.lockfile),
        lockfile_path: update.lockfile_path,
        dependency_groups: included_direct_groups(update.config.optional),
        frozen_lockfile: false,
        prefer_frozen_lockfile: Some(false),
        ignore_manifest_check: false,
        skip_runtimes: update.config.skip_runtimes,
        trust_lockfile: update.config.trust_lockfile,
        update_checksums: update.patches,
        mutation: update_mutation(update.packages, update.latest),
        installs_only: true,
        resolved_packages: update.resolved_packages,
        supported_architectures: owned.supported_architectures,
        node_linker: update.config.node_linker,
        lockfile_only: update.lockfile_only,
        dry_run: false,
        persist_policy_excludes: update.save,
        update_seed_policy: seed.policy,
        preferred_versions_override: Some(seed.preferred_versions_override),
        auth_override: None,
        resolution_observer: owned.resolution_observer,
        peer_issues_sink: None,
        deps_requiring_build_sink: None,
        catalogs_override: seed.catalogs_override,
        disable_optimistic_repeat_install: false,
        pnpmfile_hook_override: read_package_hook.map(|(hook, _)| Arc::clone(hook)),
        workspace_projects_override: None,
    }
}

/// Write back the manifests the update rewrote and the catalogs the install
/// settled on, then prune the workspace manifest.
fn settle_selected_update<Reporter: self::Reporter>(
    update: UpdateView<'_>,
    site: &UpdateSite,
    projects: &mut [pnpm_workspace::Project],
    manifest: &PackageManifest,
    prepared: SelectedUpdatePreparation,
    bumps: Option<ManifestSpecBumps>,
) -> Result<(), UpdateError> {
    let applied = bumps.map(|bumps| bumps.applied.into_inner().expect("never poisoned"));
    let persist_indices = bumped_persist_indices::<Reporter>(
        projects,
        &site.workspace_root,
        applied.as_ref(),
        prepared.persist_indices,
    );
    persist_selected_manifests::<Reporter>(projects, &persist_indices)?;
    let workspace_dir = site.catalogs_dir(prepared.workspace_dir_for_catalogs.as_deref());
    if update.save
        && let Some(applied) = applied.as_ref().filter(|applied| !applied.catalogs.is_empty())
    {
        write_workspace_catalogs_selected(
            update.config,
            workspace_dir,
            &applied.catalogs,
            projects,
        )
        .map_err(UpdateError::WriteWorkspaceManifest)?;
    }
    if update.save {
        post_install_prune(update.config, Some(workspace_dir), manifest)
            .map_err(UpdateError::WriteWorkspaceManifest)?;
    }
    Ok(())
}

/// A selector that matched nothing at depth 0 is an error; anything else
/// leaves the command a no-op.
fn nothing_to_update(depth: usize, packages: &[String], latest: bool) -> Result<(), UpdateError> {
    if depth == 0 && !packages.is_empty() && !latest {
        return Err(UpdateError::NoPackageInDependencies);
    }
    Ok(())
}

async fn run_update_install<Reporter, DependencyGroupList>(
    install: Install<'_, DependencyGroupList>,
    unsaved: UnsavedManifests,
    bumps: Option<&ManifestSpecBumps>,
) -> Result<Option<crate::InstallError>, UpdateError>
where
    Reporter: self::Reporter + 'static,
    DependencyGroupList: IntoIterator<Item = DependencyGroup> + Send,
{
    match unsaved.lockfile_specifiers {
        Some(manifests) => {
            install
                .run_with_lockfile_specifier_project_manifests::<Reporter>(
                    manifests,
                    unsaved.hooked_paths,
                )
                .await
        }
        None => match bumps {
            Some(bumps) => install.run_with_manifest_spec_bumps::<Reporter>(bumps).await,
            None => install.run::<Reporter>().await,
        },
    }
    .pipe(defer_ignored_builds)
    .map_err(UpdateError::Install)
}

/// What deciding the post-install manifest writes depends on.
#[derive(Clone, Copy)]
struct SettleUpdate<'a> {
    save: bool,
    should_persist_manifest: bool,
    importer_id: &'a str,
    applied: Option<&'a crate::AppliedSpecBumps>,
    workspace_dir_for_catalogs: Option<&'a Path>,
}

/// Write back what the install settled on: the bumped manifest ranges, the
/// catalogs the bumps moved, and the workspace-manifest prune.
fn settle_update_manifest<Reporter: self::Reporter>(
    manifest: &mut PackageManifest,
    config: &Config,
    settle: SettleUpdate<'_>,
) -> Result<(), UpdateError> {
    let bumped_manifest = settle
        .applied
        .and_then(|applied| applied.manifests.get(settle.importer_id))
        .is_some_and(|bumped| {
            apply_bumped_manifest_specs::<Reporter>(
                manifest,
                bumped,
                !settle.should_persist_manifest,
            )
        });
    if settle.should_persist_manifest || bumped_manifest {
        persist_manifest::<Reporter>(manifest)?;
    }
    if settle.save
        && let Some(applied) = settle.applied.filter(|applied| !applied.catalogs.is_empty())
    {
        write_workspace_catalogs(
            config,
            settle.workspace_dir_for_catalogs,
            &applied.catalogs,
            manifest,
        )
        .map_err(UpdateError::WriteWorkspaceManifest)?;
    }
    if settle.save {
        post_install_prune(config, settle.workspace_dir_for_catalogs, manifest)
            .map_err(UpdateError::WriteWorkspaceManifest)?;
    }
    Ok(())
}

/// Run the pnpmfile's `readPackage` hook over every selected project's
/// manifest, and over the active one, each at most once.
async fn hook_selected_manifests(
    projects: &mut [pnpm_workspace::Project],
    manifest: &mut PackageManifest,
    hook: &Arc<dyn pnpm_hooks::PnpmfileHooks>,
    log: &pnpm_hooks::LogFn,
    hooked_paths: &mut HashSet<PathBuf>,
) -> Result<(), UpdateError> {
    for project in projects.iter_mut() {
        if hooked_paths.insert(project.manifest.path().to_path_buf()) {
            apply_read_package_hook_to_update_manifest(&mut project.manifest, hook, log).await?;
        }
    }
    if hooked_paths.insert(manifest.path().to_path_buf()) {
        apply_read_package_hook_to_update_manifest(manifest, hook, log).await?;
    }
    Ok(())
}

fn selected_seed_policy(
    patches: bool,
    policies: BTreeMap<String, ImporterUpdateSeedPolicy>,
    depth: usize,
) -> UpdateSeedPolicy {
    if patches {
        return UpdateSeedPolicy::RefreshRevisions;
    }
    UpdateSeedPolicy::ByImporter { policies, max_depth: UpdateDepth::new(depth) }
}

async fn run_selected_update_install<Reporter, DependencyGroupList>(
    install: Install<'_, DependencyGroupList>,
    selection: WorkspaceInstallSelection<'_>,
    unsaved: UnsavedManifests,
    bumps: Option<&ManifestSpecBumps>,
) -> Result<Option<crate::InstallError>, UpdateError>
where
    Reporter: self::Reporter + 'static,
    DependencyGroupList: IntoIterator<Item = DependencyGroup> + Send,
{
    match unsaved.lockfile_specifiers {
        Some(manifests) => {
            install
                .run_selected_with_lockfile_specifier_project_manifests::<Reporter>(
                    selection,
                    manifests,
                    unsaved.hooked_paths,
                )
                .await
        }
        None => match bumps {
            Some(bumps) => {
                install.run_selected_with_manifest_spec_bumps::<Reporter>(selection, bumps).await
            }
            None => install.run_selected::<Reporter>(selection).await,
        },
    }
    .pipe(defer_ignored_builds)
    .map_err(UpdateError::Install)
}

/// Write the install's bumped ranges into every project that got one, and
/// report which projects now need persisting.
fn bumped_persist_indices<Reporter: self::Reporter>(
    projects: &mut [pnpm_workspace::Project],
    workspace_root: &Path,
    applied: Option<&crate::AppliedSpecBumps>,
    mut persist_indices: Vec<usize>,
) -> Vec<usize> {
    let Some(applied) = applied else { return persist_indices };
    for (index, project) in projects.iter_mut().enumerate() {
        let importer_id =
            pnpm_workspace::importer_id_from_root_dir(workspace_root, &project.root_dir);
        let Some(bumped) = applied.manifests.get(&importer_id) else { continue };
        let already_persisting = persist_indices.contains(&index);
        if apply_bumped_manifest_specs::<Reporter>(
            &mut project.manifest,
            bumped,
            !already_persisting,
        ) && !already_persisting
        {
            persist_indices.push(index);
        }
    }
    persist_indices
}

struct UpdatePreparation {
    seed_policy: UpdateSeedPolicy,
    preferred_versions_override: PreferredVersions,
    persist_manifest: bool,
    /// Direct dependencies whose declared range the install may move onto
    /// the version it resolves, each mapped to the group and specifier the
    /// manifest declares for it. See [`crate::ManifestSpecBumps`].
    bump_targets: HashMap<String, (DependencyGroup, String)>,
    updated_catalogs: Catalogs,
    catalogs_override: Option<Catalogs>,
    workspace_dir_for_catalogs: Option<PathBuf>,
}

#[derive(Default)]
struct SelectedUpdatePreparation {
    seed_policies: BTreeMap<String, ImporterUpdateSeedPolicy>,
    preferred_versions_override: PreferredVersions,
    persist_indices: Vec<usize>,
    /// [`UpdatePreparation::bump_targets`] per importer id.
    bump_targets: BTreeMap<String, HashMap<String, (DependencyGroup, String)>>,
    updated_catalogs: Catalogs,
    catalogs_override: Option<Catalogs>,
    workspace_dir_for_catalogs: Option<PathBuf>,
    any_work: bool,
}

impl SelectedUpdatePreparation {
    /// Fold one project's preparation in, under the importer id it was
    /// prepared for.
    fn merge(&mut self, index: usize, importer_id: String, prepared: UpdatePreparation) {
        self.any_work = true;
        for (name, selectors) in prepared.preferred_versions_override {
            self.preferred_versions_override.entry(name).or_default().extend(selectors);
        }
        if !prepared.bump_targets.is_empty() {
            self.bump_targets.insert(importer_id.clone(), prepared.bump_targets);
        }
        if let Some(policy) = importer_seed_policy(prepared.seed_policy) {
            self.seed_policies.insert(importer_id, policy);
        }
        if prepared.persist_manifest {
            self.persist_indices.push(index);
        }
        merge_catalogs(&mut self.updated_catalogs, &prepared.updated_catalogs);
        if let Some(complete_catalogs) = prepared.catalogs_override {
            self.catalogs_override = Some(complete_catalogs);
        }
        if self.workspace_dir_for_catalogs.is_none() {
            self.workspace_dir_for_catalogs = prepared.workspace_dir_for_catalogs;
        }
    }
}

/// A loaded `readPackage` hook paired with the log sink its `context.log`
/// calls are forwarded to.
type ReadPackageHook = (Arc<dyn pnpm_hooks::PnpmfileHooks>, pnpm_hooks::LogFn);

fn update_read_package_hook<Reporter: self::Reporter>(
    workspace_root: &Path,
    config: &Config,
) -> Result<Option<ReadPackageHook>, UpdateError> {
    let Some(hook) =
        pnpm_hooks::finder::load_pnpmfiles(workspace_root, crate::pnpmfile_selection(config))
            .map_err(UpdateError::MissingPnpmfile)?
    else {
        return Ok(None);
    };
    let log = hook.source_path().map_or_else(
        || Arc::new(|_| {}) as pnpm_hooks::LogFn,
        |from| {
            crate::install_with_fresh_lockfile::hook_log_fn::<Reporter>(
                workspace_root,
                from,
                "readPackage",
            )
        },
    );
    Ok(Some((hook, log)))
}

async fn apply_read_package_hook_to_update_manifest(
    manifest: &mut PackageManifest,
    hook: &Arc<dyn pnpm_hooks::PnpmfileHooks>,
    log: &pnpm_hooks::LogFn,
) -> Result<(), UpdateError> {
    let ctx = pnpm_hooks::HookContext { log: Arc::clone(log), dir: None };
    let value = hook
        .read_package(manifest.value().clone(), ctx)
        .await
        .map_err(InstallError::ReadPackageHook)
        .map_err(UpdateError::Install)?;
    *manifest.value_mut() = (*value).clone();
    Ok(())
}

async fn prepare_manifest<Reporter: self::Reporter>(
    manifest: &mut PackageManifest,
    update: UpdateView<'_>,
    owned: &UpdateOwned,
    catalogs_seed: Option<&Catalogs>,
    latest_chain: &mut Option<LatestResolverChain>,
) -> Result<Option<UpdatePreparation>, UpdateError> {
    let Some(decision) =
        decide_update::<Reporter>(manifest, update, owned, catalogs_seed, latest_chain).await?
    else {
        return Ok(None);
    };
    apply_update_decision::<Reporter>(manifest, update, decision).map(Some)
}

/// What the seed-policy decision settled: the plan, the policy, the direct
/// dependencies as the manifest declared them, and the catalogs consulted.
struct UpdateDecision {
    plan: UpdatePlan,
    seed_policy: UpdateSeedPolicy,
    direct: Vec<(String, DependencyGroup, String)>,
    catalog_ctx: Option<CatalogCtx>,
}

async fn decide_update<Reporter: self::Reporter>(
    manifest: &PackageManifest,
    update: UpdateView<'_>,
    owned: &UpdateOwned,
    catalogs_seed: Option<&Catalogs>,
    latest_chain: &mut Option<LatestResolverChain>,
) -> Result<Option<UpdateDecision>, UpdateError> {
    let selectors = parse_selectors(update.packages);
    if update.latest {
        reject_versioned_latest_selectors(update.packages, &selectors)?;
    }
    // Snapshot direct dependencies before mutation so matching and rewrites
    // both see the original manifest shape.
    let direct = declared_direct(manifest, &owned.include_direct);
    // Catalogs stay lazy unless an earlier selected project already produced
    // the complete in-memory catalog set for this batch.
    let mut catalog_ctx = catalogs_seed
        .map(|catalogs| read_catalog_ctx_with_catalogs(manifest, catalogs.clone()))
        .transpose()?;
    let scope = UpdateScope {
        selectors: &selectors,
        direct: &direct,
        lockfile: update.lockfile,
        config: update.config,
        latest: update.latest,
        save: update.save,
        depth: update.depth,
        max_depth: UpdateDepth::new(update.depth),
        // `pacquet update` has no `--save-prefix` flag yet, so `save_exact`
        // selects between an exact pin and the default caret range.
        range_spec_style: RangeSpecStyle::from_save_options(update.save_exact, None),
        updates_all_groups: updates_all_groups(&owned.include_direct),
        // Bare-name selectors with depth update matching names at any depth.
        use_name_matcher: !selectors.is_empty()
            && selectors.iter().all(|selector| selector.version.is_none())
            && update.depth > 0
            && !update.latest,
    };
    let mut plan = UpdatePlan::default();
    let Some(seed_policy) = select_seed_policy::<Reporter>(
        &scope,
        &mut plan,
        &LatestRewriteCtx {
            manifest,
            config: update.config,
            http_client_arc: &owned.http_client_arc,
            resolution_observer: owned.resolution_observer.as_ref(),
            range_spec_style: scope.range_spec_style,
            lockfile_only: update.lockfile_only,
        },
        latest_chain,
        &mut catalog_ctx,
        (update.workspace_packages, workspace_targets(update, &selectors, &direct)?),
    )
    .await?
    else {
        return Ok(None);
    };
    Ok(Some(UpdateDecision { plan, seed_policy, direct, catalog_ctx }))
}

/// The direct dependencies of the groups the update covers, as
/// `(name, group, specifier)`.
fn declared_direct(
    manifest: &PackageManifest,
    include_direct: &[DependencyGroup],
) -> Vec<(String, DependencyGroup, String)> {
    include_direct
        .iter()
        .flat_map(|&group| {
            manifest
                .dependencies([group])
                .map(move |(name, spec)| (name.to_string(), group, spec.to_string()))
        })
        .collect()
}

fn updates_all_groups(include_direct: &[DependencyGroup]) -> bool {
    DIRECT_GROUPS.iter().all(|group| include_direct.contains(group))
}

/// `--workspace` with nothing to link falls through to the ordinary
/// branches below: a selector that matched no direct dependency still
/// updates that name deeper in the graph, and an empty selector list
/// still updates every direct dependency.
fn workspace_targets(
    update: UpdateView<'_>,
    selectors: &[ParsedSelector],
    direct: &[(String, DependencyGroup, String)],
) -> Result<Vec<WorkspaceLinkTarget>, UpdateError> {
    Ok(update
        .workspace_packages
        .map(|packages| workspace_link_targets(selectors, direct, packages, update.config))
        .transpose()?
        .unwrap_or_default())
}

fn apply_update_decision<Reporter: self::Reporter>(
    manifest: &mut PackageManifest,
    update: UpdateView<'_>,
    decision: UpdateDecision,
) -> Result<UpdatePreparation, UpdateError> {
    let UpdateDecision { mut plan, seed_policy, direct, mut catalog_ctx } = decision;
    // Reconcile only manifest rewrites. Existing `catalog:` references retain
    // their group, and non-manual catalog modes may promote direct versions.
    let mut updated_catalogs = Catalogs::new();
    let workspace_dir_for_catalogs = reconcile_catalog_rewrites::<Reporter>(
        manifest,
        update.config,
        update.latest,
        &direct,
        &mut plan.rewrites,
        &mut catalog_ctx,
        &mut updated_catalogs,
    )?;
    // `--no-save` still mutates the in-memory manifest used for resolution,
    // while leaving package.json and reporter manifest events untouched.
    let persist_manifest = update.save && !plan.rewrites.is_empty();
    if persist_manifest {
        emit_initial_package_manifest::<Reporter>(manifest);
    }
    apply_rewrites(manifest, &plan.rewrites)?;
    Ok(UpdatePreparation {
        seed_policy,
        preferred_versions_override: plan.preferred_versions_override,
        persist_manifest,
        bump_targets: plan.bump_targets,
        catalogs_override: merged_catalogs_override(catalog_ctx.as_ref(), &updated_catalogs),
        updated_catalogs,
        workspace_dir_for_catalogs,
    })
}

fn apply_rewrites(
    manifest: &mut PackageManifest,
    rewrites: &[(String, DependencyGroup, String)],
) -> Result<(), UpdateError> {
    for (name, group, specifier) in rewrites {
        manifest.add_dependency(name, specifier, *group).map_err(UpdateError::UpdateManifest)?;
    }
    Ok(())
}

/// The install must resolve against the complete catalog set even when
/// `--no-save` deliberately skips the workspace-manifest write.
fn merged_catalogs_override(
    catalog_ctx: Option<&CatalogCtx>,
    updated_catalogs: &Catalogs,
) -> Option<Catalogs> {
    (!updated_catalogs.is_empty()).then(|| {
        let mut merged = catalog_ctx.map(|ctx| ctx.catalogs.clone()).unwrap_or_default();
        merge_catalogs(&mut merged, updated_catalogs);
        merged
    })
}

/// `--latest` forbids versioned selectors.
fn reject_versioned_latest_selectors(
    packages: &[String],
    selectors: &[ParsedSelector],
) -> Result<(), UpdateError> {
    let with_spec = packages
        .iter()
        .zip(selectors)
        .filter(|(_, selector)| selector.version.is_some())
        .map(|(raw, _)| raw.as_str())
        .collect::<Vec<_>>();
    if with_spec.is_empty() {
        return Ok(());
    }
    Err(UpdateError::LatestWithSpec(with_spec.join(", ")))
}

/// What every branch of the seed-policy decision reads.
struct UpdateScope<'a> {
    selectors: &'a [ParsedSelector],
    /// The direct dependencies as the manifest declared them before the
    /// update rewrote anything: `(name, group, specifier)`.
    direct: &'a [(String, DependencyGroup, String)],
    lockfile: Option<&'a Lockfile>,
    config: &'a Config,
    latest: bool,
    save: bool,
    depth: usize,
    max_depth: UpdateDepth,
    range_spec_style: RangeSpecStyle,
    updates_all_groups: bool,
    use_name_matcher: bool,
}

/// What the branches accumulate on the way to a seed policy.
#[derive(Default)]
struct UpdatePlan {
    /// Names whose lockfile pins the resolve must not reuse.
    drop_targets: UpdateTargets,
    /// Manifest declarations to rewrite: `(name, group, specifier)`.
    rewrites: Vec<(String, DependencyGroup, String)>,
    /// A compatible bump cannot name its version before the resolve, so the
    /// matched names are collected here and the install reports back what it
    /// settled on.
    bump_targets: HashMap<String, (DependencyGroup, String)>,
    preferred_versions_override: PreferredVersions,
}

impl UpdatePlan {
    fn drop_only(&mut self, max_depth: UpdateDepth) -> UpdateSeedPolicy {
        UpdateSeedPolicy::DropOnly { targets: std::mem::take(&mut self.drop_targets), max_depth }
    }
}

/// The seed policy this update runs under, or `None` when nothing it names is
/// updatable and the command is a no-op.
async fn select_seed_policy<Reporter: self::Reporter>(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
    rewrite_ctx: &LatestRewriteCtx<'_, '_>,
    latest_chain: &mut Option<LatestResolverChain>,
    catalog_ctx: &mut Option<CatalogCtx>,
    workspace: (Option<&WorkspacePackages>, Vec<WorkspaceLinkTarget>),
) -> Result<Option<UpdateSeedPolicy>, UpdateError> {
    let (workspace_packages, workspace_targets) = workspace;
    if let Some(workspace_packages) = workspace_packages.filter(|_| !workspace_targets.is_empty()) {
        return Ok(Some(workspace_seed_policy(scope, plan, workspace_targets, workspace_packages)));
    }
    if scope.selectors.is_empty() {
        return all_direct_seed_policy::<Reporter>(
            scope,
            plan,
            rewrite_ctx,
            latest_chain,
            catalog_ctx,
        )
        .await
        .map(Some);
    }
    if scope.use_name_matcher {
        return Ok(Some(name_matched_seed_policy(scope, plan)));
    }
    selector_seed_policy::<Reporter>(scope, plan, rewrite_ctx, latest_chain, catalog_ctx).await
}

/// `--workspace`: every matched dependency is relinked to the workspace
/// project that provides it.
fn workspace_seed_policy(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
    workspace_targets: Vec<WorkspaceLinkTarget>,
    workspace_packages: &WorkspacePackages,
) -> UpdateSeedPolicy {
    for target in workspace_targets {
        let specifier = workspace_specifier(
            &target,
            &workspace_packages[&target.name],
            scope.config.save_workspace_protocol,
            scope.range_spec_style,
        );
        plan.drop_targets.insert(target.name.clone(), None);
        plan.rewrites.push((target.name, target.group, specifier));
    }
    plan.drop_only(scope.max_depth)
}

/// No selector: every included direct dependency updates.
async fn all_direct_seed_policy<Reporter: self::Reporter>(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
    rewrite_ctx: &LatestRewriteCtx<'_, '_>,
    latest_chain: &mut Option<LatestResolverChain>,
    catalog_ctx: &mut Option<CatalogCtx>,
) -> Result<UpdateSeedPolicy, UpdateError> {
    // `updateConfig.ignoreDependencies` applies only when no selector was
    // supplied and remains scoped by the included direct groups.
    let ignore_patterns =
        scope.config.update_config.ignore_dependencies.as_deref().unwrap_or_default();
    let ignore_matcher = (!ignore_patterns.is_empty()).then(|| create_matcher(ignore_patterns));
    let is_ignored =
        |name: &str| ignore_matcher.as_ref().is_some_and(|matcher| matcher.matches(name));
    if scope.latest && !scope.save {
        emit_latest_ignored::<Reporter>(rewrite_ctx.manifest);
    }
    for (name, group, previous) in scope.direct {
        if is_ignored(name) {
            continue;
        }
        record_direct_update(
            scope,
            plan,
            rewrite_ctx,
            latest_chain,
            catalog_ctx,
            (name, *group, previous),
        )
        .await?;
    }
    if scope.updates_all_groups && ignore_patterns.is_empty() {
        // A bare, ungated update re-resolves the whole graph.
        return Ok(UpdateSeedPolicy::DropAll { max_depth: scope.max_depth });
    }
    let nothing_dropped = plan.drop_targets.is_empty();
    widen_drop_targets_to_lockfile(scope, plan, nothing_dropped, &is_ignored);
    Ok(plan.drop_only(scope.max_depth))
}

/// One direct dependency of a selector-less update.
async fn record_direct_update(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
    rewrite_ctx: &LatestRewriteCtx<'_, '_>,
    latest_chain: &mut Option<LatestResolverChain>,
    catalog_ctx: &mut Option<CatalogCtx>,
    declared: (&String, DependencyGroup, &String),
) -> Result<(), UpdateError> {
    let (name, group, previous) = declared;
    if scope.latest
        && scope.save
        && let Some(specifier) =
            latest_specifier(rewrite_ctx, latest_chain, catalog_ctx, name, previous).await?
    {
        plan.rewrites.push((name.clone(), group, specifier));
    }
    if scope.save && !scope.latest {
        plan.bump_targets.entry(name.clone()).or_insert_with(|| (group, previous.clone()));
    }
    plan.drop_targets.insert(name.clone(), None);
    Ok(())
}

/// An update that covers every group also drops the pins of the packages only
/// the lockfile names, so nothing transitive stays behind.
fn widen_drop_targets_to_lockfile(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
    nothing_dropped: bool,
    is_ignored: &impl Fn(&str) -> bool,
) {
    if !scope.updates_all_groups || (scope.latest && nothing_dropped) {
        return;
    }
    let Some(snapshots) = scope.lockfile.and_then(|lockfile| lockfile.snapshots.as_ref()) else {
        return;
    };
    for key in snapshots.keys() {
        let name = key.name.to_string();
        if !is_ignored(&name) {
            plan.drop_targets.insert(name, None);
        }
    }
}

/// Bare-name selectors with a depth: every matching name updates, at any
/// depth.
fn name_matched_seed_policy(scope: &UpdateScope<'_>, plan: &mut UpdatePlan) -> UpdateSeedPolicy {
    let patterns =
        scope.selectors.iter().map(|selector| selector.pattern.clone()).collect::<Vec<_>>();
    let matcher = create_matcher(&patterns);
    for (name, group, previous) in scope.direct {
        if matcher.matches(name) {
            if scope.save {
                plan.bump_targets.entry(name.clone()).or_insert_with(|| (*group, previous.clone()));
            }
            plan.drop_targets.insert(name.clone(), None);
        }
    }
    widen_drop_targets_by_matcher(scope.lockfile, plan, &matcher);
    plan.drop_only(scope.max_depth)
}

/// Lockfile names keep transitive-only matches in the update scope.
fn widen_drop_targets_by_matcher(
    lockfile: Option<&Lockfile>,
    plan: &mut UpdatePlan,
    matcher: &pnpm_config::matcher::Matcher,
) {
    let Some(snapshots) = lockfile.and_then(|lockfile| lockfile.snapshots.as_ref()) else {
        return;
    };
    for key in snapshots.keys() {
        let name = key.name.to_string();
        if matcher.matches(&name) {
            plan.drop_targets.insert(name, None);
        }
    }
}

/// Selectors that may name a version: only what they match updates.
async fn selector_seed_policy<Reporter: self::Reporter>(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
    rewrite_ctx: &LatestRewriteCtx<'_, '_>,
    latest_chain: &mut Option<LatestResolverChain>,
    catalog_ctx: &mut Option<CatalogCtx>,
) -> Result<Option<UpdateSeedPolicy>, UpdateError> {
    let patterns =
        scope.selectors.iter().map(|selector| selector.pattern.clone()).collect::<Vec<_>>();
    let matcher = create_matcher(&patterns);
    let expanded = expand_update_selectors(scope.selectors);
    let matched_direct = scope
        .direct
        .iter()
        .filter(|(name, _, _)| matcher.matches(name))
        .cloned()
        .collect::<Vec<_>>();
    if matched_direct.is_empty() {
        // An unmatched `--latest` selector is a no-op. Deeper versioned
        // selectors can still target lockfile names but cannot force that
        // version.
        if scope.depth == 0 || scope.latest {
            return Ok(None);
        }
        widen_drop_targets_by_selectors(scope.lockfile, plan, &expanded);
        return Ok(Some(plan.drop_only(scope.max_depth)));
    }
    if scope.latest && !scope.save {
        emit_latest_ignored::<Reporter>(rewrite_ctx.manifest);
    }
    for (name, group, previous) in &matched_direct {
        record_matched_direct_update::<Reporter>(
            scope,
            plan,
            MatchedRewriteInputs { rewrite_ctx, latest_chain, catalog_ctx, expanded: &expanded },
            (name, *group, previous),
        )
        .await?;
    }
    Ok(Some(plan.drop_only(scope.max_depth)))
}

fn widen_drop_targets_by_selectors(
    lockfile: Option<&Lockfile>,
    plan: &mut UpdatePlan,
    expanded: &[ParsedSelector],
) {
    let Some(snapshots) = lockfile.and_then(|lockfile| lockfile.snapshots.as_ref()) else {
        return;
    };
    let target_matcher = create_matcher(
        &expanded.iter().map(|selector| selector.pattern.clone()).collect::<Vec<_>>(),
    );
    for key in snapshots.keys() {
        let name = key.name.to_string();
        if target_matcher.matches(&name) {
            insert_update_target(&mut plan.drop_targets, expanded, &name);
        }
    }
}

/// What one matched direct dependency is rewritten against.
struct MatchedRewriteInputs<'a, 'ctx, 'borrow> {
    rewrite_ctx: &'a LatestRewriteCtx<'ctx, 'borrow>,
    latest_chain: &'a mut Option<LatestResolverChain>,
    catalog_ctx: &'a mut Option<CatalogCtx>,
    expanded: &'a [ParsedSelector],
}

/// What a selector does to one matched direct dependency.
enum MatchedRewrite {
    /// The selector cannot apply, so the dependency stays out of the update
    /// entirely.
    Skipped,
    /// The dependency is in the update, with the declaration it rewrites to.
    Target(Option<String>),
}

async fn record_matched_direct_update<Reporter: self::Reporter>(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
    inputs: MatchedRewriteInputs<'_, '_, '_>,
    declared: (&String, DependencyGroup, &String),
) -> Result<(), UpdateError> {
    let (name, group, _) = declared;
    let expanded = inputs.expanded;
    let MatchedRewrite::Target(rewrite) =
        matched_direct_rewrite::<Reporter>(scope, plan, inputs, declared).await?
    else {
        return Ok(());
    };
    insert_update_target(
        &mut plan.drop_targets,
        expanded,
        &update_target_name(scope.selectors, name),
    );
    if let Some(specifier) = rewrite {
        plan.rewrites.push((name.clone(), group, specifier));
    }
    Ok(())
}

async fn matched_direct_rewrite<Reporter: self::Reporter>(
    scope: &UpdateScope<'_>,
    plan: &mut UpdatePlan,
    inputs: MatchedRewriteInputs<'_, '_, '_>,
    declared: (&String, DependencyGroup, &String),
) -> Result<MatchedRewrite, UpdateError> {
    let (name, group, previous) = declared;
    let MatchedRewriteInputs { rewrite_ctx, latest_chain, catalog_ctx, .. } = inputs;
    // The two sources are exclusive: `--latest` rejects versioned selectors
    // above, so under it no selector carries a version.
    if scope.latest {
        // `--latest` reaches past the declared range by design, which a
        // manifest that keeps its specifiers can't record.
        if !scope.save {
            return Ok(MatchedRewrite::Target(None));
        }
        let specifier =
            latest_specifier(rewrite_ctx, latest_chain, catalog_ctx, name, previous).await?;
        return Ok(MatchedRewrite::Target(specifier));
    }
    let requested = scope
        .selectors
        .iter()
        .find(|selector| matcher_one(&selector.pattern).matches(name))
        .and_then(|selector| selector.version.clone());
    // Seeded whatever the manifest ends up recording, so the install locks
    // the version that was asked for. A selector naming a range or a tag is
    // not a version and seeds nothing.
    if let Some(version) = requested.as_deref() {
        crate::install_with_fresh_lockfile::prefer_requested_version(
            &mut plan.preferred_versions_override,
            name,
            version,
        );
    }
    if !scope.save {
        // An update that doesn't save keeps the manifest's specifier, and
        // whatever resolution settles on has to satisfy it — a frozen install
        // rejects the lockfile otherwise.
        let Some(requested) = requested.as_deref() else {
            return Ok(MatchedRewrite::Target(None));
        };
        return Ok(kept_range_rewrite::<Reporter>(rewrite_ctx, name, requested, previous));
    }
    let tag = requested
        .as_deref()
        .filter(|specifier| get_version_selector_type(specifier) == Some(VersionSelectorType::Tag));
    if let Some(tag) = tag {
        let rewritten = tag_rewrite(
            rewrite_ctx,
            latest_chain,
            &mut plan.preferred_versions_override,
            scope.range_spec_style,
            (name, previous, tag),
            requested.clone(),
        )
        .await?;
        return Ok(MatchedRewrite::Target(rewritten));
    }
    let Some(requested) = requested else {
        plan.bump_targets.entry(name.clone()).or_insert_with(|| (group, previous.clone()));
        return Ok(MatchedRewrite::Target(None));
    };
    Ok(MatchedRewrite::Target(Some(requested_version_rewrite(
        &requested,
        previous,
        scope.range_spec_style,
    ))))
}

/// The declaration a `<name>@<requested>` selector writes over `previous`.
///
/// A version is recorded under the operator the manifest already pins, the
/// way the npm resolver's `calc_specifier` records a version it has just
/// picked, so `pnpm update react@19.3.0` moves `^19.2.8` to `^19.3.0`
/// (pnpm/pnpm#14745). A range, a tag, or an entry that is not a registry
/// range names no version to pin and is written as requested.
fn requested_version_rewrite(
    requested: &str,
    previous: &str,
    default_style: RangeSpecStyle,
) -> String {
    let Ok(version) = Version::parse(requested) else {
        return requested.to_string();
    };
    let Some((prefix, declared_range)) = split_npm_alias(previous) else {
        return requested.to_string();
    };
    let range = calc_version_range(
        &version,
        infer_range_spec_style(declared_range),
        infer_range_spec_style(requested),
        default_style,
    );
    format!("{prefix}{range}")
}

/// The declaration an update that does not save may write: only a version the
/// kept range already admits.
fn kept_range_rewrite<Reporter: self::Reporter>(
    rewrite_ctx: &LatestRewriteCtx<'_, '_>,
    name: &str,
    requested: &str,
    previous: &str,
) -> MatchedRewrite {
    match judge_against_kept_range(requested, previous) {
        KeptRangeVerdict::Admitted => MatchedRewrite::Target(Some(requested.to_string())),
        KeptRangeVerdict::Excluded => {
            Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: format!(
                    r#"Skipping "{name}@{requested}": it doesn't satisfy "{previous}", which the manifest keeps when updating without saving."#,
                ),
                prefix: package_manifest_prefix(rewrite_ctx.manifest),
            }));
            MatchedRewrite::Skipped
        }
        KeptRangeVerdict::Undecided => {
            Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: format!(
                    r#"Ignoring "{name}@{requested}": the manifest keeps "{previous}" when updating without saving, so "{name}" was updated within that range instead."#,
                ),
                prefix: package_manifest_prefix(rewrite_ctx.manifest),
            }));
            MatchedRewrite::Target(None)
        }
    }
}

/// A dist tag names no version until it is resolved, so an entry pinning a
/// version or a range records what the tag resolved to, keeping the operator
/// it already pins. An entry that already tracks a tag keeps tracking one.
/// Anything else — a `catalog:` reference, a `workspace:` or `npm:` alias, a
/// path or git dependency — declares something no version round-trips, so it
/// stands and the selector reaches the install as a preference only.
async fn tag_rewrite(
    rewrite_ctx: &LatestRewriteCtx<'_, '_>,
    latest_chain: &mut Option<LatestResolverChain>,
    preferred_versions_override: &mut PreferredVersions,
    range_spec_style: RangeSpecStyle,
    declared: (&str, &str, &str),
    requested: Option<String>,
) -> Result<Option<String>, UpdateError> {
    let (name, previous, tag) = declared;
    let rewritten = match get_version_selector_type(previous) {
        Some(VersionSelectorType::Version | VersionSelectorType::Range) => {
            match tag_version(rewrite_ctx, latest_chain, name, tag).await? {
                Some(version) => {
                    crate::install_with_fresh_lockfile::prefer_requested_version(
                        preferred_versions_override,
                        name,
                        &version.to_string(),
                    );
                    Some(calc_version_range(
                        &version,
                        infer_range_spec_style(previous),
                        None,
                        range_spec_style,
                    ))
                }
                None => requested,
            }
        }
        Some(VersionSelectorType::Tag) => requested,
        None => None,
    };
    // A declaration that already says what the selector settles on is not a
    // rewrite; recording it would mark the manifest dirty and persist it for
    // nothing.
    Ok(rewritten.filter(|specifier| specifier != previous))
}

/// Route each rewrite through the catalog mode, returning the workspace
/// directory whose manifest holds the catalogs when any were consulted.
fn reconcile_catalog_rewrites<Reporter: self::Reporter>(
    manifest: &PackageManifest,
    config: &Config,
    latest: bool,
    direct: &[(String, DependencyGroup, String)],
    rewrites: &mut Vec<(String, DependencyGroup, String)>,
    catalog_ctx: &mut Option<CatalogCtx>,
    updated_catalogs: &mut Catalogs,
) -> Result<Option<PathBuf>, UpdateError> {
    if rewrites.is_empty() || (config.catalog_mode == CatalogMode::Manual && catalog_ctx.is_none())
    {
        return Ok(None);
    }
    let ctx = ensure_catalog_ctx(catalog_ctx, manifest, config)?;
    let mut reconciled = Vec::with_capacity(rewrites.len());
    for (name, group, specifier) in std::mem::take(rewrites) {
        let previous = direct
            .iter()
            .find(|(previous_name, previous_group, _)| {
                *previous_name == name && *previous_group == group
            })
            .map(|(_, _, previous_specifier)| previous_specifier.as_str());
        let reconciliation = reconcile_rewrite::<Reporter>(
            config,
            ctx,
            latest,
            updated_catalogs,
            (&name, &specifier, previous),
        )?;
        if let Some(specifier) = reconciliation {
            reconciled.push((name, group, specifier));
        }
    }
    *rewrites = reconciled;
    Ok(ctx.workspace_dir_opt.clone().or_else(|| Some(ctx.manifest_dir.clone())))
}

/// The specifier one rewrite records in the manifest, or `None` when the
/// catalog mode moved it into a catalog instead.
fn reconcile_rewrite<Reporter: self::Reporter>(
    config: &Config,
    ctx: &CatalogCtx,
    latest: bool,
    updated_catalogs: &mut Catalogs,
    rewrite: (&str, &str, Option<&str>),
) -> Result<Option<String>, UpdateError> {
    let (name, specifier, previous) = rewrite;
    if latest && let Some(catalog_name) = previous.and_then(parse_catalog_protocol) {
        updated_catalogs
            .entry(catalog_name.to_string())
            .or_default()
            .insert(name.to_string(), specifier.to_string());
        return Ok(None);
    }
    if config.catalog_mode == CatalogMode::Manual {
        return Ok(Some(specifier.to_string()));
    }
    let dependency =
        CatalogModeDep { alias: name, bare_specifier: specifier, prev_specifier: previous };
    let decision = decide_catalog::<Reporter>(
        config.catalog_mode,
        None,
        &ctx.catalogs,
        &dependency,
        &ctx.prefix,
    )
    .map_err(UpdateError::CatalogVersionMismatch)?;
    match decision {
        CatalogDecision::KeepDirect => Ok(Some(specifier.to_string())),
        CatalogDecision::Catalog { manifest_specifier, updated_entry } => {
            if let Some(entry) = updated_entry {
                updated_catalogs
                    .entry(entry.catalog_name)
                    .or_default()
                    .insert(name.to_string(), entry.specifier);
            }
            Ok(Some(manifest_specifier))
        }
    }
}

async fn prepare_selected_manifests<Reporter: self::Reporter>(
    projects: &mut [pnpm_workspace::Project],
    selected_indices: &[usize],
    workspace_root: &Path,
    update: UpdateView<'_>,
    owned: &UpdateOwned,
) -> Result<SelectedUpdatePreparation, UpdateError> {
    // One picker across every selected project: it is created on first
    // use, so a selection that resolves no `latest` tag never builds one.
    let mut latest_chain = None;
    let mut prepared_all = SelectedUpdatePreparation::default();

    // Once per command, across every selected project: a selector that is a
    // direct dependency of one project is legitimately versioned even where a
    // sibling only reaches it transitively. `--depth 0` reports
    // `NoPackageInDependencies` instead, and `--latest` rejects versioned
    // selectors outright.
    if !update.latest && update.depth > 0 {
        let selectors = parse_selectors(update.packages);
        let manifests =
            selected_indices.iter().map(|&index| &projects[index].manifest).collect::<Vec<_>>();
        reject_versions_of_indirect_update_specs::<Reporter>(
            &selectors,
            &manifests,
            &owned.include_direct,
            &workspace_root.to_string_lossy(),
        )?;
    }

    for &index in selected_indices {
        let Some(prepared) = prepare_manifest::<Reporter>(
            &mut projects[index].manifest,
            update,
            owned,
            prepared_all.catalogs_override.as_ref(),
            &mut latest_chain,
        )
        .await?
        else {
            continue;
        };
        let importer_id =
            pnpm_workspace::importer_id_from_root_dir(workspace_root, &projects[index].root_dir);
        prepared_all.merge(index, importer_id, prepared);
    }

    // A recursive `--latest` that matches nothing is an error, unlike the
    // single-project one that quietly returns: with no project left to
    // mutate there is nothing for the run to have meant.
    if update.depth == 0 && !update.packages.is_empty() && !prepared_all.any_work {
        return Err(UpdateError::NoPackageInDependencies);
    }

    Ok(prepared_all)
}

/// One project's own share of a recursive update, as the workspace-wide
/// per-importer policy records it. `None` leaves the importer out of the
/// policy map, which reads as keeping every pin it has.
fn importer_seed_policy(seed_policy: UpdateSeedPolicy) -> Option<ImporterUpdateSeedPolicy> {
    match seed_policy {
        UpdateSeedPolicy::KeepAll => None,
        UpdateSeedPolicy::DropAll { .. } => Some(ImporterUpdateSeedPolicy::DropAll),
        UpdateSeedPolicy::DropOnly { targets, .. } => {
            Some(ImporterUpdateSeedPolicy::DropOnly(targets))
        }
        UpdateSeedPolicy::KeepAllResolveAll
        | UpdateSeedPolicy::FixLockfile
        | UpdateSeedPolicy::RefreshRevisions => {
            unreachable!("manifest preparation never uses a whole-graph seed policy")
        }
        UpdateSeedPolicy::ByImporter { .. } => {
            unreachable!("per-manifest preparation never produces importer policies")
        }
    }
}

fn merge_catalogs(target: &mut Catalogs, updates: &Catalogs) {
    for (catalog_name, entries) in updates {
        let catalog = target.entry(catalog_name.clone()).or_default();
        for (dependency, specifier) in entries {
            catalog.insert(dependency.clone(), specifier.clone());
        }
    }
}

/// Write the ranges the install settled on into `manifest`, reporting
/// whether anything changed. The alias keeps the group it is declared under:
/// an update moves a range, it never moves a dependency between groups.
///
/// `announce_initial` emits the manifest's pre-rewrite shape, which the
/// reporter pairs with the one [`persist_manifest`] emits. Manifest
/// preparation already announced a manifest it rewrote before resolving, so
/// only a manifest this is the first to touch needs it.
fn apply_bumped_manifest_specs<Reporter: self::Reporter>(
    manifest: &mut PackageManifest,
    bumped: &BTreeMap<String, (DependencyGroup, String)>,
    announce_initial: bool,
) -> bool {
    let declared = bumped
        .iter()
        .filter(|(alias, (group, _))| {
            manifest.dependencies([*group]).any(|(name, _)| name == alias.as_str())
        })
        .collect::<Vec<_>>();
    if declared.is_empty() {
        return false;
    }
    if announce_initial {
        emit_initial_package_manifest::<Reporter>(manifest);
    }
    for (alias, (group, specifier)) in declared {
        // Written in place rather than through `add_dependency`, which
        // moves the alias into the target group by deleting it from the
        // others. An update moves a range, never a dependency.
        manifest.value_mut()[<&str>::from(*group)][alias] =
            serde_json::Value::String(specifier.clone());
    }
    true
}

fn persist_selected_manifests<Reporter: self::Reporter>(
    projects: &mut [pnpm_workspace::Project],
    selected_indices: &[usize],
) -> Result<(), UpdateError> {
    for &index in selected_indices {
        persist_manifest::<Reporter>(&mut projects[index].manifest)?;
    }
    Ok(())
}

fn persist_manifest<Reporter: self::Reporter>(
    manifest: &mut PackageManifest,
) -> Result<(), UpdateError> {
    let updated = manifest.save_and_get_written_value().map_err(UpdateError::SaveManifest)?;
    let prefix = package_manifest_prefix(manifest);
    Reporter::emit(&LogEvent::PackageManifest(PackageManifestLog {
        level: LogLevel::Debug,
        message: PackageManifestMessage::Updated { prefix, updated },
    }));
    Ok(())
}

/// One direct dependency `--workspace` re-points at the workspace copy
/// of the same name.
struct WorkspaceLinkTarget {
    name: String,
    group: DependencyGroup,
    /// The specifier the manifest declares today, which decides the range
    /// operator the rewritten `workspace:` specifier keeps.
    declared: String,
    /// The range the selector asked for (`*` for a bare `foo`), which the
    /// workspace version has to satisfy.
    wanted_range: String,
}

/// The direct dependencies `--workspace` re-points, in manifest order.
///
/// With no selectors every direct dependency that a workspace project
/// publishes is linked (minus `updateConfig.ignoreDependencies`); the
/// rest keep their registry specifiers. With selectors, each *matched*
/// direct dependency must be a workspace package — naming one that isn't
/// is the failure the `--workspace` help text advertises.
fn workspace_link_targets(
    selectors: &[ParsedSelector],
    direct: &[(String, DependencyGroup, String)],
    workspace_packages: &WorkspacePackages,
    config: &Config,
) -> Result<Vec<WorkspaceLinkTarget>, UpdateError> {
    if selectors.is_empty() {
        return Ok(all_workspace_link_targets(direct, workspace_packages, config));
    }
    let mut targets = Vec::new();
    let patterns = selectors.iter().map(|selector| selector.pattern.clone()).collect::<Vec<_>>();
    let matcher = create_matcher(&patterns);
    // Per-selector matchers, compiled once, map a matched dependency back
    // to the selector that claimed it — and so to the version it asked for.
    let claims = selectors
        .iter()
        .map(|selector| (matcher_one(&selector.pattern), selector.version.as_deref()))
        .collect::<Vec<_>>();
    for (name, group, declared) in direct {
        if !matcher.matches(name.as_str()) {
            continue;
        }
        if !workspace_packages.contains_key(name) {
            return Err(UpdateError::WorkspacePackageNotFound(name.clone()));
        }
        let wanted = claims
            .iter()
            .find(|(matcher, _)| matcher.matches(name))
            .and_then(|(_, version)| *version)
            .unwrap_or("*");
        targets.push(WorkspaceLinkTarget {
            name: name.clone(),
            group: *group,
            declared: declared.clone(),
            wanted_range: wanted.strip_prefix("workspace:").unwrap_or(wanted).to_string(),
        });
    }
    Ok(targets)
}

/// Without a selector, `--workspace` relinks every direct dependency the
/// workspace itself provides, minus the ignored ones.
fn all_workspace_link_targets(
    direct: &[(String, DependencyGroup, String)],
    workspace_packages: &WorkspacePackages,
    config: &Config,
) -> Vec<WorkspaceLinkTarget> {
    let ignore_patterns = config.update_config.ignore_dependencies.as_deref().unwrap_or_default();
    let ignore_matcher = (!ignore_patterns.is_empty()).then(|| create_matcher(ignore_patterns));
    direct
        .iter()
        .filter(|(name, _, _)| {
            !ignore_matcher.as_ref().is_some_and(|matcher| matcher.matches(name.as_str()))
                && workspace_packages.contains_key(name)
        })
        .map(|(name, group, declared)| WorkspaceLinkTarget {
            name: name.clone(),
            group: *group,
            declared: declared.clone(),
            wanted_range: "*".to_string(),
        })
        .collect()
}

/// The `workspace:` specifier `--workspace` writes for a linked
/// dependency.
///
/// `--workspace` is an explicit request for the protocol, so unlike
/// `pnpm add` this never declines on [`SaveWorkspaceProtocol::Off`] —
/// the setting only chooses the shape.
fn workspace_specifier(
    target: &WorkspaceLinkTarget,
    versions: &WorkspacePackagesByVersion,
    protocol: SaveWorkspaceProtocol,
    default_pin: RangeSpecStyle,
) -> String {
    // Nothing satisfies the requested range: keep it, so the install
    // reports it as `NO_MATCHING_VERSION_INSIDE_WORKSPACE` against the
    // range the user asked for.
    let Some(version) = pick_workspace_version(versions, &target.wanted_range) else {
        return format!("workspace:{}", target.wanted_range);
    };
    calc_specifier_for_workspace_dep(
        DeclaredSpecifiers { prev: Some(&target.declared), bare: None },
        None,
        &target.name,
        Some(&version),
        protocol,
        default_pin,
    )
}

/// The workspace version a `workspace:<range>` specifier would resolve
/// to. A range that isn't semver is a dist-tag, which the workspace
/// answers with its highest version.
fn pick_workspace_version(versions: &WorkspacePackagesByVersion, range: &str) -> Option<String> {
    let range = if node_semver::Range::parse(range).is_ok() { range } else { "*" };
    resolve_workspace_range(range, &versions.keys().cloned().collect::<Vec<_>>())
}

/// `--latest` reaches past the declared range by design, which a manifest that
/// keeps its specifiers can't record, so the update stays inside the range and
/// says so once per project.
fn emit_latest_ignored<Reporter: self::Reporter>(manifest: &PackageManifest) {
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Warn,
        message: r#"Ignoring "--latest": the manifest keeps its version ranges when updating without saving, so dependencies were updated within them instead."#.to_string(),
        prefix: package_manifest_prefix(manifest),
    }));
}

/// What an update that doesn't save may do with a requested specifier, given
/// the specifier the manifest keeps.
enum KeptRangeVerdict {
    /// A version the kept range admits: resolution can be pointed at it.
    Admitted,
    /// A version the kept range excludes: the dependency is left alone.
    Excluded,
    /// Nothing that can be judged before resolution — a range or a dist tag,
    /// which names a version only once resolution has run, or a kept
    /// specifier that isn't a semver range. The kept specifier decides.
    Undecided,
}

/// Judge a requested specifier against the range the manifest keeps.
///
/// Only a concrete version gets a verdict. Matching a version against a range
/// is exact; deciding whether one *range* is contained by another is not —
/// implementations disagree around prerelease boundaries — so a range is left
/// [`Undecided`] rather than guessed at.
///
/// [`Undecided`]: KeptRangeVerdict::Undecided
fn judge_against_kept_range(requested: &str, kept: &str) -> KeptRangeVerdict {
    let (Ok(requested), Ok(kept)) =
        (node_semver::Version::parse(requested), node_semver::Range::parse(kept))
    else {
        return KeptRangeVerdict::Undecided;
    };
    if requested.satisfies(&kept) { KeptRangeVerdict::Admitted } else { KeptRangeVerdict::Excluded }
}

/// The selectors an update selector stands for. An `npm:` selector
/// contributes a second one for the aliased package, because that -- not
/// the alias -- is the name the resolver resolves the edge under; it
/// carries the aliased spec's own version so the expansion scopes the same
/// version line the user asked for.
fn expand_update_selectors(selectors: &[ParsedSelector]) -> Vec<ParsedSelector> {
    let mut expanded = Vec::with_capacity(selectors.len());
    for selector in selectors {
        expanded.push(ParsedSelector {
            pattern: selector.pattern.clone(),
            version: selector.version.clone(),
        });
        let Some(aliased) =
            selector.version.as_deref().and_then(|version| version.strip_prefix("npm:"))
        else {
            continue;
        };
        let alias = parse_update_param(aliased);
        let pattern = if selector.pattern.starts_with('!') {
            format!("!{}", alias.pattern)
        } else {
            alias.pattern
        };
        expanded.push(ParsedSelector { pattern, version: alias.version });
    }
    expanded
}

/// Record `name` as an update target once per selector that claims it: a
/// selector pinning an exact version scopes the target to that version's
/// line, while a bare or ranged one widens it to every version. Negated
/// selectors exclude names, never versions, so they claim nothing here --
/// the matcher that found `name` has already applied them.
fn insert_update_target(targets: &mut UpdateTargets, selectors: &[ParsedSelector], name: &str) {
    let mut claimed = false;
    for selector in selectors.iter().filter(|selector| !selector.pattern.starts_with('!')) {
        if !matcher_one(&selector.pattern).matches(name) {
            continue;
        }
        claimed = true;
        targets.insert(name.to_string(), selector.version.as_deref().and_then(VersionLine::parse));
    }
    if !claimed {
        targets.insert(name.to_string(), None);
    }
}

/// Whether any of `manifests` declares a dependency `selector` names, so the
/// update has a manifest entry to write the requested version into.
fn selector_matches_a_direct_dependency(
    selector: &ParsedSelector,
    manifests: &[&PackageManifest],
    include_direct: &[DependencyGroup],
) -> bool {
    let matcher = matcher_one(&selector.pattern);
    manifests.iter().any(|manifest| {
        manifest.dependencies(include_direct.iter().copied()).any(|(name, _)| matcher.matches(name))
    })
}

/// `pacquet update <dep>@<version>` where `<dep>` matches no direct dependency
/// has nowhere to record the version. An update resolves such a target the way
/// a fresh install would -- which a command-line version cannot influence -- so
/// honoring the request would mean writing a lockfile entry no manifest backs,
/// and the next fresh resolve would undo it. Neither npm nor Yarn accepts a
/// version here either. Fail rather than resolve to something else and leave
/// the caller a zero exit status to read.
///
/// A range or a tag names no single version to record, so updating within the
/// dependents' ranges is a reasonable reading of it: those only warn. A
/// negated selector excludes names rather than requesting one, so it is not
/// judged here at all.
///
/// The override the hint recommends is scoped to the dependents' declared
/// range so it cannot violate any consumer's range; that range lives in the
/// dependents' manifests, which this layer does not read, hence the
/// placeholder.
fn reject_versions_of_indirect_update_specs<Reporter: self::Reporter>(
    selectors: &[ParsedSelector],
    manifests: &[&PackageManifest],
    include_direct: &[DependencyGroup],
    prefix: &str,
) -> Result<(), UpdateError> {
    let mut pinned = Vec::new();
    for selector in selectors {
        let Some(version) = selector.version.as_deref() else { continue };
        // A negated selector excludes names; a version on one asks for nothing.
        if selector.pattern.starts_with('!')
            || selector_matches_a_direct_dependency(selector, manifests, include_direct)
        {
            continue;
        }
        let pattern = &selector.pattern;
        if node_semver::Version::parse(version).is_err() {
            Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: format!(
                    r#""{pattern}" is not a direct dependency, so the requested "{version}" is ignored — "{pattern}" is updated to what a fresh install would resolve."#,
                ),
                prefix: prefix.to_string(),
            }));
            continue;
        }
        pinned.push((pattern.clone(), version.to_string()));
    }
    if pinned.is_empty() {
        return Ok(());
    }
    let subjects = pinned
        .iter()
        .map(|(pattern, version)| format!(r#""{pattern}" (requested "{version}")"#))
        .collect::<Vec<_>>()
        .join(", ");
    let tail = if pinned.len() == 1 {
        "is not a direct dependency, so the requested version cannot"
    } else {
        "are not direct dependencies, so the requested versions cannot"
    };
    let overrides = pinned
        .iter()
        .map(|(pattern, version)| format!("    {pattern}@<declared range>: {version}"))
        .collect::<Vec<_>>()
        .join("\n");
    let names = pinned.iter().map(|(pattern, _)| pattern.as_str()).collect::<Vec<_>>().join(" ");
    Err(UpdateError::UpdateVersionOnIndirectDep {
        message: format!("{subjects} {tail} be recorded."),
        hint: format!(
            "An update resolves a transitive dependency the way a fresh install would, so a version on the command line has no effect on it. To pin one, add an override scoped to the range its dependents declare to pnpm-workspace.yaml:\n\n  overrides:\n{overrides}\n\nTo update it within the range its dependents already declare, drop the version: pnpm update {names}",
        ),
    })
}

/// The name an update target for `matched` is keyed by. A manifest keys a
/// dependency by its alias, but the resolver matches update targets — and
/// [`UpdateSeedPolicy::DropOnly`] keys them — by the package name the edge
/// resolves under, which an `npm:` or `jsr:` selector states separately from
/// the alias. Falls back to the alias, which is the name for every other
/// selector shape.
fn update_target_name(selectors: &[ParsedSelector], matched: &str) -> String {
    selectors
        .iter()
        .filter(|selector| matcher_one(&selector.pattern).matches(matched))
        .filter_map(|selector| {
            real_package_name_of(Some(matched), Some(selector.version.as_deref()?))
        })
        .find(|name| name.as_ref() != matched)
        .map_or_else(|| matched.to_string(), std::borrow::Cow::into_owned)
}

/// Compile a single pattern into a matcher. Used to map a matched direct
/// dependency back to the selector that claimed it (so a versioned
/// selector's version is applied to the right dep).
fn matcher_one(pattern: &str) -> pnpm_config::matcher::Matcher {
    create_matcher(std::slice::from_ref(&pattern.to_string()))
}

/// The workspace catalogs and the directories needed to read the existing
/// `catalog:` entries (to preserve their range operators) and write the
/// bumped ones back to `pnpm-workspace.yaml`.
struct CatalogCtx {
    catalogs: Catalogs,
    /// The workspace root, or `None` when the project is not part of a
    /// workspace (entries are then written next to `package.json`).
    workspace_dir_opt: Option<std::path::PathBuf>,
    manifest_dir: std::path::PathBuf,
    /// Workspace (or project) directory as a string, for warning messages.
    prefix: String,
}

/// Borrow the effective catalogs, reading them on first use.
fn ensure_catalog_ctx<'slot>(
    slot: &'slot mut Option<CatalogCtx>,
    manifest: &PackageManifest,
    config: &Config,
) -> Result<&'slot CatalogCtx, UpdateError> {
    if slot.is_none() {
        *slot = Some(read_catalog_ctx(manifest, config)?);
    }
    Ok(slot.as_ref().expect("just populated"))
}

fn effective_specifier(
    catalog_ctx: &mut Option<CatalogCtx>,
    manifest: &PackageManifest,
    config: &Config,
    prev: &str,
    name: &str,
) -> Result<String, UpdateError> {
    if let Some(catalog_name) = parse_catalog_protocol(prev) {
        let ctx = ensure_catalog_ctx(catalog_ctx, manifest, config)?;
        if let Some(spec) = ctx.catalogs.get(catalog_name).and_then(|catalog| catalog.get(name)) {
            return Ok(spec.clone());
        }
    }
    Ok(prev.to_string())
}

/// Read the effective catalogs and the directories around them.
///
/// The catalogs prefer a post-`updateConfig` pnpmfile hook's output
/// (`config.catalogs`, the authoritative complete set) over the raw
/// `pnpm-workspace.yaml` read, matching `Install::run` so an update never
/// resolves `catalog:` deps against stale on-disk catalogs when a hook
/// changed them. Workspace discovery still drives where bumped entries are
/// written back.
fn read_catalog_ctx(
    manifest: &PackageManifest,
    config: &Config,
) -> Result<CatalogCtx, UpdateError> {
    let manifest_dir =
        manifest.path().parent().expect("manifest path always has a parent dir").to_path_buf();
    let workspace_dir_opt =
        pnpm_workspace::find_workspace_dir(&manifest_dir).map_err(UpdateError::FindWorkspaceDir)?;
    let catalogs = if let Some(catalogs) = config.catalogs.clone() {
        catalogs
    } else {
        let workspace_manifest = match workspace_dir_opt.as_deref() {
            Some(dir) => pnpm_workspace::read_workspace_manifest(dir)
                .map_err(UpdateError::ReadWorkspaceManifest)?,
            None => None,
        };
        get_catalogs_from_workspace_manifest(workspace_manifest.as_ref())
            .map_err(UpdateError::InvalidCatalogsConfiguration)?
    };
    let prefix =
        workspace_dir_opt.as_deref().unwrap_or(&manifest_dir).to_string_lossy().into_owned();
    Ok(CatalogCtx { catalogs, workspace_dir_opt, manifest_dir, prefix })
}

fn read_catalog_ctx_with_catalogs(
    manifest: &PackageManifest,
    catalogs: Catalogs,
) -> Result<CatalogCtx, UpdateError> {
    let manifest_dir =
        manifest.path().parent().expect("manifest path always has a parent dir").to_path_buf();
    let workspace_dir_opt =
        pnpm_workspace::find_workspace_dir(&manifest_dir).map_err(UpdateError::FindWorkspaceDir)?;
    let prefix =
        workspace_dir_opt.as_deref().unwrap_or(&manifest_dir).to_string_lossy().into_owned();
    Ok(CatalogCtx { catalogs, workspace_dir_opt, manifest_dir, prefix })
}

/// The `--latest` inputs that are the same for every direct dependency of a
/// project, gathered so [`latest_specifier`] takes them as one argument.
struct LatestRewriteCtx<'a, 'borrow> {
    manifest: &'borrow PackageManifest,
    config: &'a Config,
    http_client_arc: &'borrow Arc<ThrottledClient>,
    resolution_observer: Option<&'borrow Arc<dyn crate::ResolutionObserver>>,
    range_spec_style: RangeSpecStyle,
    lockfile_only: bool,
}

/// The specifier `--latest` should write for `name`, or `None` when no
/// resolver claims the dependency and its manifest entry therefore stands.
///
/// The answer is the resolvers': the chain is asked to resolve the
/// dependency with [`UpdateBehavior::Latest`], and whichever resolver
/// claims it reports back the specifier its own protocol round-trips to —
/// the npm picker takes the higher of the declared range and the `latest`
/// tag, the `runtime:` resolvers re-resolve within the spec the manifest
/// already declares, and the local resolvers echo their spec unchanged.
/// Nothing here needs to know which protocols those are.
async fn latest_specifier(
    ctx: &LatestRewriteCtx<'_, '_>,
    chain: &mut Option<LatestResolverChain>,
    catalog_ctx: &mut Option<CatalogCtx>,
    name: &str,
    previous: &str,
) -> Result<Option<String>, UpdateError> {
    let effective = effective_specifier(catalog_ctx, ctx.manifest, ctx.config, previous, name)?;
    // `preserveWorkspaceProtocol` is always on under `update --latest`, so a
    // `workspace:` entry keeps its text whatever version the workspace
    // package is at. Asking the chain would also hand the npm resolver a
    // spec it answers only against the install's workspace-package map,
    // which manifest preparation has not built.
    if effective.starts_with("workspace:") {
        return Ok(None);
    }
    // A dist-tag names no version of its own, so the version behind it moving
    // leaves the declaration saying exactly what was asked for. Rewriting it
    // to that version would drop the instruction to track the tag.
    if get_version_selector_type(&effective) == Some(VersionSelectorType::Tag) {
        return Ok(None);
    }
    let chain = ensure_latest_resolver_chain(chain, ctx)?;
    // The entry being resolved is also the entry whose operator the rewrite
    // keeps, so it is the previous specifier as well.
    let wanted = WantedDependency {
        alias: Some(name.to_string()),
        bare_specifier: Some(effective.clone()),
        prev_specifier: Some(effective.clone()),
        ..WantedDependency::default()
    };
    let manifest_dir =
        ctx.manifest.path().parent().expect("manifest path always has a parent dir").to_path_buf();
    let opts = ResolveOptions {
        project_dir: manifest_dir.clone(),
        lockfile_dir: manifest_dir,
        default_tag: Some("latest".to_string()),
        update: UpdateBehavior::Latest,
        calc_specifier: true,
        range_spec_style: Some(ctx.range_spec_style),
        published_by: chain.published_by,
        published_by_exclude: chain.published_by_exclude.clone(),
        dry_run: ctx.lockfile_only,
        ..ResolveOptions::default()
    };
    let resolved = Resolver::resolve(&chain.resolver, &wanted, &opts)
        .await
        .map_err(|error| UpdateError::ResolveLatest { name: name.to_string(), error })?;
    // A resolver that reports back what the manifest already says has
    // nothing to rewrite. Recording it anyway would mark the manifest dirty
    // and persist it, which for a `runtime:` dependency means rewriting the
    // entry into `devEngines.runtime` — a change the user never asked for.
    Ok(resolved
        .and_then(|result| result.normalized_bare_specifier)
        .filter(|specifier| *specifier != effective))
}

/// The version dist tag `tag` names for `name`, or `None` when no resolver
/// in the chain claims the dependency.
///
/// An explicit `<name>@<tag>` selector asks for the version behind exactly
/// that tag, so — unlike [`latest_specifier`], which reaches for whichever
/// of the declared range and the `latest` tag is higher — the tag is the
/// whole specifier resolved here.
async fn tag_version(
    ctx: &LatestRewriteCtx<'_, '_>,
    chain: &mut Option<LatestResolverChain>,
    name: &str,
    tag: &str,
) -> Result<Option<Version>, UpdateError> {
    let chain = ensure_latest_resolver_chain(chain, ctx)?;
    let wanted = WantedDependency {
        alias: Some(name.to_string()),
        bare_specifier: Some(tag.to_string()),
        ..WantedDependency::default()
    };
    let manifest_dir =
        ctx.manifest.path().parent().expect("manifest path always has a parent dir").to_path_buf();
    let opts = ResolveOptions {
        project_dir: manifest_dir.clone(),
        lockfile_dir: manifest_dir,
        default_tag: Some(tag.to_string()),
        published_by: chain.published_by,
        published_by_exclude: chain.published_by_exclude.clone(),
        dry_run: ctx.lockfile_only,
        ..ResolveOptions::default()
    };
    let resolved = Resolver::resolve(&chain.resolver, &wanted, &opts).await.map_err(|error| {
        UpdateError::ResolveTag { name: name.to_string(), tag: tag.to_string(), error }
    })?;
    Ok(resolved.and_then(|result| result.name_ver).map(|name_ver| name_ver.suffix))
}

/// The resolvers that can answer "what is the latest for this dependency",
/// built on first use so an update whose deps are all local opens no
/// client. Deliberately excludes the git, tarball and local-path
/// resolvers: they have no notion of a `latest`, and asking them would
/// clone or download during manifest preparation only to be told the
/// specifier stands.
struct LatestResolverChain {
    resolver: DefaultResolver,
    published_by: Option<DateTime<Utc>>,
    published_by_exclude: Option<PackageVersionPolicy>,
}

fn ensure_latest_resolver_chain<'chain>(
    chain: &'chain mut Option<LatestResolverChain>,
    ctx: &LatestRewriteCtx<'_, '_>,
) -> Result<&'chain LatestResolverChain, UpdateError> {
    if chain.is_none() {
        let extra_excludes = ctx
            .resolution_observer
            .and_then(|observer| observer.minimum_release_age_exclude_override());
        let policy =
            PickPolicy::from_config_with_extra_excludes(ctx.config, extra_excludes.as_deref())
                .map_err(UpdateError::MinimumReleaseAgeExclude)?;
        let npm_resolver: Arc<dyn Resolver> = Arc::new(
            create_configured_npm_resolver(ctx.config, Arc::clone(ctx.http_client_arc), &policy)
                .map_err(UpdateError::InvalidNamedRegistry)?,
        );
        let mut node_resolver = NodeResolver::new_with_auth(
            Arc::clone(ctx.http_client_arc),
            Arc::clone(&ctx.config.auth_headers),
        );
        node_resolver.node_download_mirrors.clone_from(&ctx.config.node_download_mirrors);
        node_resolver.offline = ctx.config.offline;
        node_resolver.cache_dir = Some(ctx.config.cache_dir.clone());
        let resolver = DefaultResolver::new(vec![
            Box::new(Arc::clone(&npm_resolver)) as Box<dyn Resolver>,
            Box::new(node_resolver),
            Box::new(DenoResolver::new(Arc::clone(ctx.http_client_arc), Arc::clone(&npm_resolver))),
            Box::new(BunResolver::new(Arc::clone(ctx.http_client_arc), Arc::clone(&npm_resolver))),
            Box::new(YarnResolver::new(
                Arc::clone(ctx.http_client_arc),
                ctx.config.tls.strict_ssl.unwrap_or(true),
            )),
        ]);
        *chain = Some(LatestResolverChain {
            resolver,
            published_by: policy.published_by,
            published_by_exclude: policy.published_by_exclude,
        });
    }
    Ok(chain.as_ref().expect("chain initialized above"))
}

/// Whether `bare_specifier` is a `workspace:` spec that points at a local
/// path (e.g. `workspace:../packages/foo/dist`) rather than a version range
/// (`workspace:*`, `workspace:^1.0.0`). Such specs are preserved verbatim on
/// `--latest` instead of being resolved against the registry, since the path
/// may target a publish directory that a normalized range would drop.
///
/// These are kept out of the registry-resolution path via
/// `preserveWorkspaceProtocol`, which is always on under `update --latest`
/// (the override that derives it from `linkWorkspacePackages` only runs under
/// `--workspace`, and `--workspace` cannot be combined with `--latest`).
pub(crate) fn is_workspace_local_path_specifier(bare_specifier: &str) -> bool {
    let Some(pref) = bare_specifier.strip_prefix("workspace:") else {
        return false;
    };
    let is_windows_drive = {
        let mut chars = pref.chars();
        chars.next().is_some_and(|first| first.is_ascii_alphabetic()) && chars.next() == Some(':')
    };
    pref.starts_with('.') || pref.starts_with('/') || pref.starts_with("~/") || is_windows_drive
}

#[cfg(test)]
mod tests;
