use crate::{
    CatalogDecision, CatalogModeDep, CatalogVersionMismatchError, DIRECT_GROUPS,
    ImporterUpdateSeedPolicy, Install, InstallError, ProjectMutation, ResolvedPackages,
    UpdateSeedPolicy, WorkspaceInstallSelection,
    catalog_cleanup::{
        WriteWorkspaceCatalogsError, post_install_prune, write_workspace_catalogs,
        write_workspace_catalogs_selected,
    },
    decide_catalog, defer_ignored_builds, emit_initial_package_manifest, included_direct_groups,
    manifest_spec_bumps::ManifestSpecBumps,
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
    #[display("{_0}")]
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

impl Update<'_> {
    pub async fn run<Reporter: self::Reporter + 'static>(self) -> Result<(), UpdateError> {
        let Update {
            tarball_mem_cache,
            resolved_packages,
            http_client,
            http_client_arc,
            config,
            manifest,
            lockfile,
            lockfile_path,
            packages,
            latest,
            patches,
            save_exact,
            save,
            include_direct,
            depth,
            workspace_packages,
            supported_architectures,
            lockfile_only,
            resolution_observer,
        } = self;
        http_client.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);
        http_client_arc.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);

        crate::minimum_release_age::ensure_strict_minimum_release_age_can_save(config, save)
            .map_err(UpdateError::MinimumReleaseAge)?;

        let manifest_dir =
            manifest.path().parent().expect("manifest path always has a parent dir").to_path_buf();
        let workspace_root = crate::install::lockfile_root_dir(config, &manifest_dir)
            .map_err(UpdateError::FindWorkspaceDir)?;
        let read_package_hook = (!save && !config.ignore_pnpmfile)
            .then(|| update_read_package_hook::<Reporter>(&workspace_root, config))
            .transpose()?
            .flatten();
        let mut read_package_hooked_manifest_paths = HashSet::new();
        if let Some((hook, log)) = read_package_hook.as_ref() {
            apply_read_package_hook_to_update_manifest(manifest, hook, log).await?;
            read_package_hooked_manifest_paths.insert(manifest.path().to_path_buf());
        }
        let lockfile_specifier_project_manifests =
            (!save).then(|| vec![(manifest_dir.clone(), manifest.clone())]);
        if !latest && depth > 0 {
            let selectors =
                packages.iter().map(|input| parse_update_param(input)).collect::<Vec<_>>();
            reject_versions_of_indirect_update_specs::<Reporter>(
                &selectors,
                &[manifest],
                &include_direct,
                &package_manifest_prefix(manifest),
            )?;
        }
        let mut latest_chain = None;
        let Some(prepared) = prepare_manifest::<Reporter>(
            manifest,
            &http_client_arc,
            config,
            lockfile,
            packages,
            latest,
            save_exact,
            save,
            &include_direct,
            depth,
            workspace_packages,
            None,
            &mut latest_chain,
            lockfile_only,
            resolution_observer.as_ref(),
        )
        .await?
        else {
            return if depth == 0 && !packages.is_empty() && !latest {
                Err(UpdateError::NoPackageInDependencies)
            } else {
                Ok(())
            };
        };
        if save {
            write_workspace_catalogs(
                config,
                prepared.workspace_dir_for_catalogs.as_deref(),
                &prepared.updated_catalogs,
                manifest,
            )
            .map_err(UpdateError::WriteWorkspaceManifest)?;
        }
        let UpdatePreparation {
            seed_policy,
            preferred_versions_override,
            persist_manifest: should_persist_manifest,
            catalogs_override,
            workspace_dir_for_catalogs,
            bump_targets,
            ..
        } = prepared;
        let seed_policy = if patches { UpdateSeedPolicy::RefreshRevisions } else { seed_policy };
        let importer_id = pnpm_workspace::importer_id_from_root_dir(&workspace_root, &manifest_dir);
        let bumps = (!bump_targets.is_empty()).then(|| ManifestSpecBumps {
            targets: BTreeMap::from([(importer_id.clone(), bump_targets)]),
            range_spec_style: RangeSpecStyle::from_save_options(save_exact, None),
            applied: Mutex::default(),
        });
        let install = Install {
            tarball_mem_cache,
            http_client,
            http_client_arc,
            config,
            manifest,
            emit_initial_manifest: false,
            lockfile: MaybeLazyLockfile::Loaded(lockfile),
            lockfile_path,
            // `include` is always all-true for updates: the materialized
            // `node_modules` layout must not change just because the
            // update scope was narrowed.
            dependency_groups: included_direct_groups(config.optional),
            frozen_lockfile: false,
            // `update` always re-resolves against the registry, so the
            // auto-frozen / repeat-install fast paths must not fire.
            prefer_frozen_lockfile: Some(false),
            ignore_manifest_check: false,
            skip_runtimes: config.skip_runtimes,
            trust_lockfile: config.trust_lockfile,
            update_checksums: patches,
            mutation: update_mutation(packages, latest),
            installs_only: true,
            resolved_packages,
            supported_architectures,
            node_linker: config.node_linker,
            lockfile_only,
            dry_run: false,
            persist_policy_excludes: save,
            update_seed_policy: seed_policy,
            preferred_versions_override: Some(preferred_versions_override),
            auth_override: None,
            resolution_observer,
            peer_issues_sink: None,
            deps_requiring_build_sink: None,
            catalogs_override,
            disable_optimistic_repeat_install: false,
            pnpmfile_hook_override: read_package_hook.as_ref().map(|(hook, _)| Arc::clone(hook)),
            workspace_projects_override: None,
        };
        let ignored_builds = match lockfile_specifier_project_manifests {
            Some(manifests) => {
                install
                    .run_with_lockfile_specifier_project_manifests::<Reporter>(
                        manifests,
                        read_package_hooked_manifest_paths,
                    )
                    .await
            }
            None => match bumps.as_ref() {
                Some(bumps) => install.run_with_manifest_spec_bumps::<Reporter>(bumps).await,
                None => install.run::<Reporter>().await,
            },
        }
        .pipe(defer_ignored_builds)
        .map_err(UpdateError::Install)?;

        let applied = bumps.map(|bumps| bumps.applied.into_inner().expect("never poisoned"));
        let bumped_manifest = applied
            .as_ref()
            .and_then(|applied| applied.manifests.get(&importer_id))
            .is_some_and(|bumped| {
                apply_bumped_manifest_specs::<Reporter>(manifest, bumped, !should_persist_manifest)
            });
        if should_persist_manifest || bumped_manifest {
            persist_manifest::<Reporter>(manifest)?;
        }
        if save
            && let Some(applied) = applied.as_ref().filter(|applied| !applied.catalogs.is_empty())
        {
            write_workspace_catalogs(
                config,
                workspace_dir_for_catalogs.as_deref(),
                &applied.catalogs,
                manifest,
            )
            .map_err(UpdateError::WriteWorkspaceManifest)?;
        }

        if save {
            post_install_prune(config, workspace_dir_for_catalogs.as_deref(), manifest)
                .map_err(UpdateError::WriteWorkspaceManifest)?;
        }

        if let Some(ignored_builds) = ignored_builds {
            return Err(UpdateError::Install(ignored_builds));
        }
        Ok(())
    }

    pub async fn run_selected<Reporter: self::Reporter + 'static>(
        self,
        projects: &mut [pnpm_workspace::Project],
        project_dependencies: &indexmap::IndexMap<PathBuf, Vec<PathBuf>>,
        ordered_dirs: &[PathBuf],
        selected_dirs: &HashSet<PathBuf>,
        install_dirs: &HashSet<PathBuf>,
        active_manifest_is_standin: bool,
    ) -> Result<(), UpdateError> {
        let Update {
            tarball_mem_cache,
            resolved_packages,
            http_client,
            http_client_arc,
            config,
            manifest,
            lockfile,
            lockfile_path,
            packages,
            latest,
            patches,
            save_exact,
            save,
            include_direct,
            depth,
            workspace_packages,
            supported_architectures,
            lockfile_only,
            resolution_observer,
        } = self;
        http_client.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);
        http_client_arc.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);

        crate::minimum_release_age::ensure_strict_minimum_release_age_can_save(config, save)
            .map_err(UpdateError::MinimumReleaseAge)?;

        let selected_indices = selected_project_indices(projects, ordered_dirs, selected_dirs);
        if selected_indices.is_empty() {
            return Ok(());
        }
        let workspace_root = crate::install::lockfile_root_dir(
            config,
            manifest.path().parent().expect("manifest path always has a parent dir"),
        )
        .map_err(UpdateError::FindWorkspaceDir)?;
        let read_package_hook = (!save && !config.ignore_pnpmfile)
            .then(|| update_read_package_hook::<Reporter>(&workspace_root, config))
            .transpose()?
            .flatten();
        let mut read_package_hooked_manifest_paths = HashSet::new();
        if let Some((hook, log)) = read_package_hook.as_ref() {
            for project in projects.iter_mut() {
                if read_package_hooked_manifest_paths.insert(project.manifest.path().to_path_buf())
                {
                    apply_read_package_hook_to_update_manifest(&mut project.manifest, hook, log)
                        .await?;
                }
            }
            if read_package_hooked_manifest_paths.insert(manifest.path().to_path_buf()) {
                apply_read_package_hook_to_update_manifest(manifest, hook, log).await?;
            }
        }
        let lockfile_specifier_project_manifests = (!save).then(|| {
            selected_indices
                .iter()
                .map(|&index| (projects[index].root_dir.clone(), projects[index].manifest.clone()))
                .collect::<Vec<_>>()
        });
        let mut prepared = prepare_selected_manifests::<Reporter>(
            projects,
            &selected_indices,
            &workspace_root,
            &http_client_arc,
            config,
            lockfile,
            packages,
            latest,
            save_exact,
            save,
            &include_direct,
            depth,
            workspace_packages,
            lockfile_only,
            resolution_observer.as_ref(),
        )
        .await?;
        if !prepared.any_work {
            return Ok(());
        }
        if save {
            let workspace_dir =
                prepared.workspace_dir_for_catalogs.as_deref().unwrap_or(&workspace_root);
            write_workspace_catalogs_selected(
                config,
                workspace_dir,
                &prepared.updated_catalogs,
                projects,
            )
            .map_err(UpdateError::WriteWorkspaceManifest)?;
        }

        let bumps = (!prepared.bump_targets.is_empty()).then(|| ManifestSpecBumps {
            targets: std::mem::take(&mut prepared.bump_targets),
            range_spec_style: RangeSpecStyle::from_save_options(save_exact, None),
            applied: Mutex::default(),
        });
        let install = Install {
            tarball_mem_cache,
            http_client,
            http_client_arc,
            config,
            manifest,
            emit_initial_manifest: false,
            lockfile: MaybeLazyLockfile::Loaded(lockfile),
            lockfile_path,
            dependency_groups: included_direct_groups(config.optional),
            frozen_lockfile: false,
            prefer_frozen_lockfile: Some(false),
            ignore_manifest_check: false,
            skip_runtimes: config.skip_runtimes,
            trust_lockfile: config.trust_lockfile,
            update_checksums: patches,
            mutation: update_mutation(packages, latest),
            installs_only: true,
            resolved_packages,
            supported_architectures,
            node_linker: config.node_linker,
            lockfile_only,
            dry_run: false,
            persist_policy_excludes: save,
            update_seed_policy: if patches {
                UpdateSeedPolicy::RefreshRevisions
            } else {
                UpdateSeedPolicy::ByImporter {
                    policies: prepared.seed_policies,
                    max_depth: UpdateDepth::new(depth),
                }
            },
            preferred_versions_override: Some(prepared.preferred_versions_override),
            auth_override: None,
            resolution_observer,
            peer_issues_sink: None,
            deps_requiring_build_sink: None,
            catalogs_override: prepared.catalogs_override,
            disable_optimistic_repeat_install: false,
            pnpmfile_hook_override: read_package_hook.as_ref().map(|(hook, _)| Arc::clone(hook)),
            workspace_projects_override: None,
        };
        let selection = WorkspaceInstallSelection {
            all_projects: projects,
            project_dependencies,
            ordered_dirs,
            selected_dirs,
            install_dirs,
            active_manifest_is_standin,
            workspace_cycles: crate::PrecomputedWorkspaceCycles::Unknown,
        };
        let ignored_builds = match lockfile_specifier_project_manifests {
            Some(manifests) => {
                install
                    .run_selected_with_lockfile_specifier_project_manifests::<Reporter>(
                        selection,
                        manifests,
                        read_package_hooked_manifest_paths,
                    )
                    .await
            }
            None => match bumps.as_ref() {
                Some(bumps) => {
                    install
                        .run_selected_with_manifest_spec_bumps::<Reporter>(selection, bumps)
                        .await
                }
                None => install.run_selected::<Reporter>(selection).await,
            },
        }
        .pipe(defer_ignored_builds)
        .map_err(UpdateError::Install)?;

        let applied = bumps.map(|bumps| bumps.applied.into_inner().expect("never poisoned"));
        let mut persist_indices = prepared.persist_indices;
        if let Some(applied) = applied.as_ref() {
            for (index, project) in projects.iter_mut().enumerate() {
                let importer_id =
                    pnpm_workspace::importer_id_from_root_dir(&workspace_root, &project.root_dir);
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
        }
        persist_selected_manifests::<Reporter>(projects, &persist_indices)?;
        if save
            && let Some(applied) = applied.as_ref().filter(|applied| !applied.catalogs.is_empty())
        {
            let workspace_dir =
                prepared.workspace_dir_for_catalogs.as_deref().unwrap_or(&workspace_root);
            write_workspace_catalogs_selected(config, workspace_dir, &applied.catalogs, projects)
                .map_err(UpdateError::WriteWorkspaceManifest)?;
        }

        if save {
            let workspace_dir =
                prepared.workspace_dir_for_catalogs.as_deref().unwrap_or(&workspace_root);
            post_install_prune(config, Some(workspace_dir), manifest)
                .map_err(UpdateError::WriteWorkspaceManifest)?;
        }
        if let Some(ignored_builds) = ignored_builds {
            return Err(UpdateError::Install(ignored_builds));
        }
        Ok(())
    }
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

#[expect(
    clippy::too_many_arguments,
    reason = "manifest preparation consumes the update command's matching inputs"
)]
async fn prepare_manifest<Reporter: self::Reporter>(
    manifest: &mut PackageManifest,
    http_client_arc: &Arc<ThrottledClient>,
    config: &Config,
    lockfile: Option<&Lockfile>,
    packages: &[String],
    latest: bool,
    save_exact: bool,
    save: bool,
    include_direct: &[DependencyGroup],
    depth: usize,
    workspace_packages: Option<&WorkspacePackages>,
    catalogs_seed: Option<&Catalogs>,
    latest_chain: &mut Option<LatestResolverChain>,
    lockfile_only: bool,
    resolution_observer: Option<&Arc<dyn crate::ResolutionObserver>>,
) -> Result<Option<UpdatePreparation>, UpdateError> {
    // `pacquet update` has no `--save-prefix` flag yet, so `save_exact`
    // selects between an exact pin and the default caret range.
    let range_spec_style = RangeSpecStyle::from_save_options(save_exact, None);
    let selectors = packages.iter().map(|input| parse_update_param(input)).collect::<Vec<_>>();
    // `--latest` forbids versioned selectors.
    if latest {
        let with_spec = packages
            .iter()
            .zip(&selectors)
            .filter(|(_, selector)| selector.version.is_some())
            .map(|(raw, _)| raw.as_str())
            .collect::<Vec<_>>();
        if !with_spec.is_empty() {
            return Err(UpdateError::LatestWithSpec(with_spec.join(", ")));
        }
    }

    // Snapshot direct dependencies before mutation so matching and rewrites
    // both see the original manifest shape.
    let direct = include_direct
        .iter()
        .flat_map(|&group| {
            manifest
                .dependencies([group])
                .map(move |(name, spec)| (name.to_string(), group, spec.to_string()))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let updates_all_groups = DIRECT_GROUPS.iter().all(|group| include_direct.contains(group));
    // Catalogs stay lazy unless an earlier selected project already produced
    // the complete in-memory catalog set for this batch.
    let mut catalog_ctx = catalogs_seed
        .map(|catalogs| read_catalog_ctx_with_catalogs(manifest, catalogs.clone()))
        .transpose()?;
    let mut drop_targets = UpdateTargets::default();
    let mut rewrites = Vec::new();
    // A compatible bump cannot name its version before the resolve, so the
    // matched names are collected here and the install reports back what it
    // settled on.
    let mut bump_targets = HashMap::new();
    let max_depth = UpdateDepth::new(depth);
    // Bare-name selectors with depth update matching names at any depth.
    let use_name_matcher = !selectors.is_empty()
        && selectors.iter().all(|selector| selector.version.is_none())
        && depth > 0
        && !latest;

    let rewrite_ctx = LatestRewriteCtx {
        manifest,
        config,
        http_client_arc,
        resolution_observer,
        range_spec_style,
        lockfile_only,
    };

    // `--workspace` with nothing to link falls through to the ordinary
    // branches below: a selector that matched no direct dependency still
    // updates that name deeper in the graph, and an empty selector list
    // still updates every direct dependency.
    let workspace_targets = workspace_packages
        .map(|packages| workspace_link_targets(&selectors, &direct, packages, config))
        .transpose()?
        .unwrap_or_default();

    let mut preferred_versions_override = PreferredVersions::new();
    let seed_policy = if let Some(workspace_packages) =
        workspace_packages.filter(|_| !workspace_targets.is_empty())
    {
        for target in workspace_targets {
            let specifier = workspace_specifier(
                &target,
                &workspace_packages[&target.name],
                config.save_workspace_protocol,
                range_spec_style,
            );
            drop_targets.insert(target.name.clone(), None);
            rewrites.push((target.name, target.group, specifier));
        }
        UpdateSeedPolicy::DropOnly { targets: drop_targets, max_depth }
    } else if selectors.is_empty() {
        // `updateConfig.ignoreDependencies` applies only when no selector was
        // supplied and remains scoped by the included direct groups.
        let ignore_patterns =
            config.update_config.ignore_dependencies.as_deref().unwrap_or_default();
        let ignore_matcher = (!ignore_patterns.is_empty()).then(|| create_matcher(ignore_patterns));
        let is_ignored =
            |name: &str| ignore_matcher.as_ref().is_some_and(|matcher| matcher.matches(name));
        if latest && !save {
            emit_latest_ignored::<Reporter>(rewrite_ctx.manifest);
        }
        for (name, group, previous) in &direct {
            if is_ignored(name) {
                continue;
            }
            if latest
                && save
                && let Some(specifier) =
                    latest_specifier(&rewrite_ctx, latest_chain, &mut catalog_ctx, name, previous)
                        .await?
            {
                rewrites.push((name.clone(), *group, specifier));
            }
            if save && !latest {
                bump_targets.entry(name.clone()).or_insert_with(|| (*group, previous.clone()));
            }
            drop_targets.insert(name.clone(), None);
        }
        if updates_all_groups && ignore_patterns.is_empty() {
            // A bare, ungated update re-resolves the whole graph.
            UpdateSeedPolicy::DropAll { max_depth }
        } else {
            if updates_all_groups
                && !(latest && drop_targets.is_empty())
                && let Some(snapshots) = lockfile.and_then(|lockfile| lockfile.snapshots.as_ref())
            {
                for key in snapshots.keys() {
                    let name = key.name.to_string();
                    if !is_ignored(&name) {
                        drop_targets.insert(name, None);
                    }
                }
            }
            UpdateSeedPolicy::DropOnly { targets: drop_targets, max_depth }
        }
    } else if use_name_matcher {
        let patterns =
            selectors.iter().map(|selector| selector.pattern.clone()).collect::<Vec<_>>();
        let matcher = create_matcher(&patterns);
        for (name, group, previous) in &direct {
            if matcher.matches(name) {
                if save {
                    bump_targets.entry(name.clone()).or_insert_with(|| (*group, previous.clone()));
                }
                drop_targets.insert(name.clone(), None);
            }
        }
        // Lockfile names keep transitive-only matches in the update scope.
        if let Some(snapshots) = lockfile.and_then(|lockfile| lockfile.snapshots.as_ref()) {
            for key in snapshots.keys() {
                let name = key.name.to_string();
                if matcher.matches(&name) {
                    drop_targets.insert(name, None);
                }
            }
        }
        UpdateSeedPolicy::DropOnly { targets: drop_targets, max_depth }
    } else {
        let patterns =
            selectors.iter().map(|selector| selector.pattern.clone()).collect::<Vec<_>>();
        let matcher = create_matcher(&patterns);
        let expanded = expand_update_selectors(&selectors);
        let matched_direct =
            direct.iter().filter(|(name, _, _)| matcher.matches(name)).cloned().collect::<Vec<_>>();
        if matched_direct.is_empty() {
            if depth == 0 {
                return Ok(None);
            }
            // An unmatched `--latest` selector is a no-op. Deeper versioned
            // selectors can still target lockfile names but cannot force that
            // version.
            if latest {
                return Ok(None);
            }
            if let Some(snapshots) = lockfile.and_then(|lockfile| lockfile.snapshots.as_ref()) {
                let target_matcher = create_matcher(
                    &expanded.iter().map(|selector| selector.pattern.clone()).collect::<Vec<_>>(),
                );
                for key in snapshots.keys() {
                    let name = key.name.to_string();
                    if target_matcher.matches(&name) {
                        insert_update_target(&mut drop_targets, &expanded, &name);
                    }
                }
            }
        } else {
            if latest && !save {
                emit_latest_ignored::<Reporter>(rewrite_ctx.manifest);
            }
            for (name, group, previous) in &matched_direct {
                // The two sources are exclusive: `--latest` rejects versioned
                // selectors above, so under it no selector carries a version.
                let rewrite = if latest {
                    // `--latest` reaches past the declared range by design,
                    // which a manifest that keeps its specifiers can't record.
                    if save {
                        latest_specifier(
                            &rewrite_ctx,
                            latest_chain,
                            &mut catalog_ctx,
                            name,
                            previous,
                        )
                        .await?
                    } else {
                        None
                    }
                } else {
                    let requested = selectors
                        .iter()
                        .find(|selector| matcher_one(&selector.pattern).matches(name))
                        .and_then(|selector| selector.version.clone());
                    // Seeded whatever the manifest ends up recording, so the
                    // install locks the version that was asked for. A selector
                    // naming a range or a tag is not a version and seeds
                    // nothing.
                    if let Some(version) = requested.as_deref() {
                        crate::install_with_fresh_lockfile::prefer_requested_version(
                            &mut preferred_versions_override,
                            name,
                            version,
                        );
                    }
                    // An update that doesn't save keeps the manifest's
                    // specifier, and whatever resolution settles on has to
                    // satisfy it — a frozen install rejects the lockfile
                    // otherwise.
                    if !save && let Some(requested) = requested.as_deref() {
                        match judge_against_kept_range(requested, previous) {
                            KeptRangeVerdict::Admitted => Some(requested.to_string()),
                            KeptRangeVerdict::Excluded => {
                                Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                                    level: LogLevel::Warn,
                                    message: format!(
                                        r#"Skipping "{name}@{requested}": it doesn't satisfy "{previous}", which the manifest keeps when updating without saving."#,
                                    ),
                                    prefix: package_manifest_prefix(rewrite_ctx.manifest),
                                }));
                                continue;
                            }
                            KeptRangeVerdict::Undecided => {
                                Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                                    level: LogLevel::Warn,
                                    message: format!(
                                        r#"Ignoring "{name}@{requested}": the manifest keeps "{previous}" when updating without saving, so "{name}" was updated within that range instead."#,
                                    ),
                                    prefix: package_manifest_prefix(rewrite_ctx.manifest),
                                }));
                                None
                            }
                        }
                    } else if save
                        && let Some(tag) = requested.as_deref().filter(|specifier| {
                            get_version_selector_type(specifier) == Some(VersionSelectorType::Tag)
                        })
                    {
                        // A dist tag names no version until it is resolved, so
                        // an entry pinning a version or a range records what the
                        // tag resolved to, keeping the operator it already pins.
                        // An entry that already tracks a tag keeps tracking one.
                        // Anything else — a `catalog:` reference, a `workspace:`
                        // or `npm:` alias, a path or git dependency — declares
                        // something no version round-trips, so it stands and the
                        // selector reaches the install as a preference only.
                        let rewritten = match get_version_selector_type(previous) {
                            Some(VersionSelectorType::Version | VersionSelectorType::Range) => {
                                match tag_version(&rewrite_ctx, latest_chain, name, tag).await? {
                                    Some(version) => {
                                        crate::install_with_fresh_lockfile::prefer_requested_version(
                                            &mut preferred_versions_override,
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
                        // A declaration that already says what the selector
                        // settles on is not a rewrite; recording it would mark
                        // the manifest dirty and persist it for nothing.
                        rewritten.filter(|specifier| specifier != previous)
                    } else {
                        if save && requested.is_none() {
                            bump_targets
                                .entry(name.clone())
                                .or_insert_with(|| (*group, previous.clone()));
                        }
                        requested
                    }
                };
                insert_update_target(
                    &mut drop_targets,
                    &expanded,
                    &update_target_name(&selectors, name),
                );
                if let Some(specifier) = rewrite {
                    rewrites.push((name.clone(), *group, specifier));
                }
            }
        }
        UpdateSeedPolicy::DropOnly { targets: drop_targets, max_depth }
    };

    // Reconcile only manifest rewrites. Existing `catalog:` references retain
    // their group, and non-manual catalog modes may promote direct versions.
    let mut updated_catalogs = Catalogs::new();
    let mut workspace_dir_for_catalogs = None;
    if !rewrites.is_empty() && (config.catalog_mode != CatalogMode::Manual || catalog_ctx.is_some())
    {
        let ctx = ensure_catalog_ctx(&mut catalog_ctx, manifest, config)?;
        let mut reconciled = Vec::with_capacity(rewrites.len());
        for (name, group, specifier) in rewrites {
            let previous = direct
                .iter()
                .find(|(previous_name, previous_group, _)| {
                    *previous_name == name && *previous_group == group
                })
                .map(|(_, _, previous_specifier)| previous_specifier.as_str());
            if latest && let Some(catalog_name) = previous.and_then(parse_catalog_protocol) {
                updated_catalogs
                    .entry(catalog_name.to_string())
                    .or_default()
                    .insert(name, specifier);
                continue;
            }
            if config.catalog_mode == CatalogMode::Manual {
                reconciled.push((name, group, specifier));
                continue;
            }
            let dependency = CatalogModeDep {
                alias: &name,
                bare_specifier: &specifier,
                prev_specifier: previous,
            };
            match decide_catalog::<Reporter>(
                config.catalog_mode,
                None,
                &ctx.catalogs,
                &dependency,
                &ctx.prefix,
            )
            .map_err(UpdateError::CatalogVersionMismatch)?
            {
                CatalogDecision::KeepDirect => reconciled.push((name, group, specifier)),
                CatalogDecision::Catalog { manifest_specifier, updated_entry } => {
                    if let Some(entry) = updated_entry {
                        updated_catalogs
                            .entry(entry.catalog_name)
                            .or_default()
                            .insert(name.clone(), entry.specifier);
                    }
                    reconciled.push((name, group, manifest_specifier));
                }
            }
        }
        rewrites = reconciled;
        workspace_dir_for_catalogs =
            ctx.workspace_dir_opt.clone().or_else(|| Some(ctx.manifest_dir.clone()));
    }

    // `--no-save` still mutates the in-memory manifest used for resolution,
    // while leaving package.json and reporter manifest events untouched.
    let persist_manifest = save && !rewrites.is_empty();
    if persist_manifest {
        emit_initial_package_manifest::<Reporter>(manifest);
    }
    for (name, group, specifier) in &rewrites {
        manifest.add_dependency(name, specifier, *group).map_err(UpdateError::UpdateManifest)?;
    }
    // The install must resolve against the complete catalog set even when
    // `--no-save` deliberately skips the workspace-manifest write.
    let catalogs_override = (!updated_catalogs.is_empty()).then(|| {
        let mut merged = catalog_ctx.as_ref().map(|ctx| ctx.catalogs.clone()).unwrap_or_default();
        merge_catalogs(&mut merged, &updated_catalogs);
        merged
    });
    Ok(Some(UpdatePreparation {
        seed_policy,
        preferred_versions_override,
        persist_manifest,
        bump_targets,
        updated_catalogs,
        catalogs_override,
        workspace_dir_for_catalogs,
    }))
}

#[expect(
    clippy::too_many_arguments,
    reason = "selected update preparation reuses the command's matching inputs"
)]
async fn prepare_selected_manifests<Reporter: self::Reporter>(
    projects: &mut [pnpm_workspace::Project],
    selected_indices: &[usize],
    workspace_root: &Path,
    http_client_arc: &Arc<ThrottledClient>,
    config: &Config,
    lockfile: Option<&Lockfile>,
    packages: &[String],
    latest: bool,
    save_exact: bool,
    save: bool,
    include_direct: &[DependencyGroup],
    depth: usize,
    workspace_packages: Option<&WorkspacePackages>,
    lockfile_only: bool,
    resolution_observer: Option<&Arc<dyn crate::ResolutionObserver>>,
) -> Result<SelectedUpdatePreparation, UpdateError> {
    // One picker across every selected project: it is created on first
    // use, so a selection that resolves no `latest` tag never builds one.
    let mut latest_chain = None;
    let mut seed_policies = BTreeMap::new();
    let mut persist_indices = Vec::new();
    let mut bump_targets = BTreeMap::new();
    let mut preferred_versions_override = PreferredVersions::new();
    let mut updated_catalogs = Catalogs::new();
    let mut catalogs_override = None;
    let mut workspace_dir_for_catalogs = None;
    let mut any_work = false;

    // Once per command, across every selected project: a selector that is a
    // direct dependency of one project is legitimately versioned even where a
    // sibling only reaches it transitively. `--depth 0` reports
    // `NoPackageInDependencies` instead, and `--latest` rejects versioned
    // selectors outright.
    if !latest && depth > 0 {
        let selectors = packages.iter().map(|input| parse_update_param(input)).collect::<Vec<_>>();
        let manifests =
            selected_indices.iter().map(|&index| &projects[index].manifest).collect::<Vec<_>>();
        reject_versions_of_indirect_update_specs::<Reporter>(
            &selectors,
            &manifests,
            include_direct,
            &workspace_root.to_string_lossy(),
        )?;
    }

    for &index in selected_indices {
        let Some(prepared) = prepare_manifest::<Reporter>(
            &mut projects[index].manifest,
            http_client_arc,
            config,
            lockfile,
            packages,
            latest,
            save_exact,
            save,
            include_direct,
            depth,
            workspace_packages,
            catalogs_override.as_ref(),
            &mut latest_chain,
            lockfile_only,
            resolution_observer,
        )
        .await?
        else {
            continue;
        };
        any_work = true;
        let importer_id =
            pnpm_workspace::importer_id_from_root_dir(workspace_root, &projects[index].root_dir);
        for (name, selectors) in prepared.preferred_versions_override {
            preferred_versions_override.entry(name).or_default().extend(selectors);
        }
        if !prepared.bump_targets.is_empty() {
            bump_targets.insert(importer_id.clone(), prepared.bump_targets);
        }
        match prepared.seed_policy {
            UpdateSeedPolicy::KeepAll => {}
            UpdateSeedPolicy::KeepAllResolveAll
            | UpdateSeedPolicy::FixLockfile
            | UpdateSeedPolicy::RefreshRevisions => {
                unreachable!("manifest preparation never uses a whole-graph seed policy")
            }
            UpdateSeedPolicy::DropAll { .. } => {
                seed_policies.insert(importer_id, ImporterUpdateSeedPolicy::DropAll);
            }
            UpdateSeedPolicy::DropOnly { targets, .. } => {
                seed_policies.insert(importer_id, ImporterUpdateSeedPolicy::DropOnly(targets));
            }
            UpdateSeedPolicy::ByImporter { .. } => {
                unreachable!("per-manifest preparation never produces importer policies")
            }
        }
        if prepared.persist_manifest {
            persist_indices.push(index);
        }
        merge_catalogs(&mut updated_catalogs, &prepared.updated_catalogs);
        if let Some(complete_catalogs) = prepared.catalogs_override {
            catalogs_override = Some(complete_catalogs);
        }
        if workspace_dir_for_catalogs.is_none() {
            workspace_dir_for_catalogs = prepared.workspace_dir_for_catalogs;
        }
    }

    // A recursive `--latest` that matches nothing is an error, unlike the
    // single-project one that quietly returns: with no project left to
    // mutate there is nothing for the run to have meant.
    if depth == 0 && !packages.is_empty() && !any_work {
        return Err(UpdateError::NoPackageInDependencies);
    }

    Ok(SelectedUpdatePreparation {
        seed_policies,
        preferred_versions_override,
        persist_indices,
        bump_targets,
        updated_catalogs,
        catalogs_override,
        workspace_dir_for_catalogs,
        any_work,
    })
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
    let mut targets = Vec::new();
    if selectors.is_empty() {
        let ignore_patterns =
            config.update_config.ignore_dependencies.as_deref().unwrap_or_default();
        let ignore_matcher = (!ignore_patterns.is_empty()).then(|| create_matcher(ignore_patterns));
        for (name, group, declared) in direct {
            let ignored =
                ignore_matcher.as_ref().is_some_and(|matcher| matcher.matches(name.as_str()));
            if ignored || !workspace_packages.contains_key(name) {
                continue;
            }
            targets.push(WorkspaceLinkTarget {
                name: name.clone(),
                group: *group,
                declared: declared.clone(),
                wanted_range: "*".to_string(),
            });
        }
        return Ok(targets);
    }

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
    let wanted = WantedDependency {
        alias: Some(name.to_string()),
        bare_specifier: Some(effective.clone()),
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
            Box::new(YarnResolver::new(Arc::clone(ctx.http_client_arc))),
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
