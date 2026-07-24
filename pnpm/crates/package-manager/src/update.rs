use crate::{
    CatalogDecision, CatalogModeDep, CatalogVersionMismatchError, DIRECT_GROUPS,
    ImporterUpdateSeedPolicy, Install, InstallError, ResolvedPackages, UpdateSeedPolicy,
    WorkspaceInstallSelection,
    catalog_cleanup::{
        WriteWorkspaceCatalogsError, write_workspace_catalogs, write_workspace_catalogs_selected,
    },
    decide_catalog, emit_initial_package_manifest, package_manifest_prefix,
    resolution_policy::PickPolicy,
    selected_project_indices,
};
use chrono::{DateTime, Utc};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_catalogs_config::{
    InvalidCatalogsConfigurationError, get_catalogs_from_workspace_manifest,
};
use pacquet_catalogs_protocol_parser::parse_catalog_protocol;
use pacquet_catalogs_types::Catalogs;
use pacquet_config::{
    CatalogMode, Config, matcher::create_matcher, version_policy::PackageVersionPolicy,
};
use pacquet_engine_runtime_bun_resolver::BunResolver;
use pacquet_engine_runtime_deno_resolver::DenoResolver;
use pacquet_engine_runtime_node_resolver::NodeResolver;
use pacquet_lockfile::{Lockfile, MaybeLazyLockfile};
use pacquet_network::ThrottledClient;
use pacquet_package_manifest::{DependencyGroup, PackageManifest, PackageManifestError};
use pacquet_registry::PinnedVersion;
use pacquet_reporter::{LogEvent, LogLevel, PackageManifestLog, PackageManifestMessage, Reporter};
use pacquet_resolving_default_resolver::DefaultResolver;
use pacquet_resolving_npm_resolver::{
    InMemoryPackageMetaCache, NpmResolver, merge_named_registries, shared_packument_fetch_locker,
    shared_picked_manifest_cache,
};
use pacquet_resolving_resolver_base::{ResolveOptions, Resolver, UpdateBehavior, WantedDependency};
use pacquet_tarball::MemCache;
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

/// Everything `pacquet update` (alias `up` / `upgrade`) does.
///
/// Runs on pacquet's always-fresh-resolve install path. Its behavior has
/// two halves:
///
/// * **Compatible bump** (no `--latest`): the matched names have their
///   lockfile pins withheld from the preferred-versions seed
///   ([`UpdateSeedPolicy`]) so the resolver re-picks the highest version
///   satisfying the manifest range. `package.json` is left untouched —
///   the manifest is only rewritten for deps marked `updateSpec`, which
///   compatible updates are not.
/// * **`--latest`**: each matched *direct* dependency's `latest` tag is
///   fetched and written into `package.json`, reusing the range operator
///   the dependency already pinned (`^` stays `^`, `~` stays `~`, an exact
///   pin stays exact) and falling back to the configured default
///   otherwise. The follow-up install then resolves the new range.
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
    /// `--save-exact` / `-E`: write the resolved version without a range
    /// operator when rewriting the manifest under `--latest`. Only applies
    /// to dependencies whose current specifier has no recoverable pin; an
    /// existing `^`/`~`/exact range is preserved over this default.
    pub save_exact: bool,
    /// `--save` (default) / `--no-save`. When `false`, the manifest is
    /// not persisted: the `--latest` / versioned-selector range rewrites
    /// still drive resolution (so `pnpm-lock.yaml` updates) but
    /// `package.json` on disk is left untouched.
    pub save: bool,
    /// Dependency groups the update considers when choosing which direct
    /// dependencies to match, derived from
    /// `--prod` / `--dev` / `--no-optional`. Note: the *materialized*
    /// dependency set is always all three groups (the `node_modules`
    /// layout is unchanged); this only narrows the update scope.
    pub include_direct: Vec<DependencyGroup>,
    /// `--depth`. Only its `> 0` predicate is consulted (the `depth > 0`
    /// gate on the name matcher); `usize::MAX` stands in for the
    /// `Infinity` default.
    pub depth: usize,
    /// CLI-merged `supportedArchitectures`, forwarded to the install.
    pub supported_architectures: Option<pacquet_package_is_installable::SupportedArchitectures>,
    /// `--lockfile-only`: re-resolve and rewrite `pnpm-lock.yaml` without
    /// materializing `node_modules`. Forwarded to the install.
    pub lockfile_only: bool,
    /// Sink notified for each resolved tarball package, and the source of
    /// the optional resolver-time [`PackageVersionGuard`]. `None` for a
    /// plain `pacquet update`; `pacquet audit --fix update` installs one
    /// whose guard rejects vulnerable versions so the resolver falls back
    /// to a safe one.
    ///
    /// [`PackageVersionGuard`]: pacquet_resolving_resolver_base::PackageVersionGuard
    pub resolution_observer: Option<Arc<dyn crate::ResolutionObserver>>,
}

/// Error type of [`Update`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum UpdateError {
    /// `--latest` was combined with a versioned selector (`foo@2`).
    #[display("Specs are not allowed to be used with --latest ({_0})")]
    #[diagnostic(code(ERR_PNPM_LATEST_WITH_SPEC))]
    LatestWithSpec(#[error(not(source))] String),

    /// Package selectors were given with `--depth 0` but none matched a
    /// direct dependency.
    #[display("None of the specified packages were found in the dependencies.")]
    #[diagnostic(code(ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES))]
    NoPackageInDependencies,

    /// A resolver failed while computing the specifier `--latest` should
    /// write for a direct dependency.
    #[display("Failed to resolve the latest version of {name}: {error}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_UPDATE_RESOLVE_LATEST))]
    ResolveLatest {
        name: String,
        #[error(source)]
        error: pacquet_resolving_resolver_base::ResolveError,
    },

    /// A `named-registries` alias is misconfigured.
    #[diagnostic(transparent)]
    InvalidNamedRegistry(
        #[error(source)] pacquet_resolving_npm_resolver::MergeNamedRegistriesError,
    ),

    /// `minimumReleaseAgeExclude` contained an invalid rule.
    #[display("Invalid value in minimumReleaseAgeExclude: {_0}")]
    #[diagnostic(code(ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE_EXCLUDE))]
    MinimumReleaseAgeExclude(#[error(source)] pacquet_config::version_policy::VersionPolicyError),

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
            save_exact,
            save,
            include_direct,
            depth,
            supported_architectures,
            lockfile_only,
            resolution_observer,
        } = self;

        crate::minimum_release_age::ensure_strict_minimum_release_age_can_save(config, save)
            .map_err(UpdateError::MinimumReleaseAge)?;

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
            persist_manifest: should_persist_manifest,
            catalogs_override,
            ..
        } = prepared;
        Install {
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
            dependency_groups: DIRECT_GROUPS,
            frozen_lockfile: false,
            // `update` always re-resolves against the registry, so the
            // auto-frozen / repeat-install fast paths must not fire.
            prefer_frozen_lockfile: Some(false),
            ignore_manifest_check: false,
            skip_runtimes: config.skip_runtimes,
            trust_lockfile: config.trust_lockfile,
            update_checksums: false,
            // A targeted `pacquet update <pkg>` is a partial install
            // (pnpm's `installSome`); a bare `pacquet update` is a full
            // install that runs the project's own lifecycle scripts.
            is_full_install: packages.is_empty(),
            installs_only: true,
            resolved_packages,
            supported_architectures,
            node_linker: config.node_linker,
            lockfile_only,
            dry_run: false,
            update_seed_policy: seed_policy,
            auth_override: None,
            resolution_observer,
            peer_issues_sink: None,
            catalogs_override,
            disable_optimistic_repeat_install: false,
            pnpmfile_hook_override: None,
            workspace_projects_override: None,
        }
        .run::<Reporter>()
        .await
        .map_err(UpdateError::Install)?;

        if should_persist_manifest {
            persist_manifest::<Reporter>(manifest)?;
        }

        Ok(())
    }

    pub async fn run_selected<Reporter: self::Reporter + 'static>(
        self,
        projects: &mut [pacquet_workspace::Project],
        ordered_groups: &[Vec<PathBuf>],
        ordered_dirs: &[PathBuf],
        selected_dirs: &HashSet<PathBuf>,
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
            save_exact,
            save,
            include_direct,
            depth,
            supported_architectures,
            lockfile_only,
            resolution_observer,
        } = self;

        crate::minimum_release_age::ensure_strict_minimum_release_age_can_save(config, save)
            .map_err(UpdateError::MinimumReleaseAge)?;

        let selected_indices = selected_project_indices(projects, ordered_dirs, selected_dirs);
        if selected_indices.is_empty() {
            return Ok(());
        }
        let workspace_root = config.workspace_dir.as_deref().unwrap_or_else(|| {
            manifest.path().parent().expect("manifest path always has a parent dir")
        });
        let prepared = prepare_selected_manifests::<Reporter>(
            projects,
            &selected_indices,
            workspace_root,
            &http_client_arc,
            config,
            lockfile,
            packages,
            latest,
            save_exact,
            save,
            &include_direct,
            depth,
            lockfile_only,
            resolution_observer.as_ref(),
        )
        .await?;
        if !prepared.any_work {
            return Ok(());
        }
        if save {
            let workspace_dir =
                prepared.workspace_dir_for_catalogs.as_deref().unwrap_or(workspace_root);
            write_workspace_catalogs_selected(
                config,
                workspace_dir,
                &prepared.updated_catalogs,
                projects,
            )
            .map_err(UpdateError::WriteWorkspaceManifest)?;
        }

        Install {
            tarball_mem_cache,
            http_client,
            http_client_arc,
            config,
            manifest,
            emit_initial_manifest: false,
            lockfile: MaybeLazyLockfile::Loaded(lockfile),
            lockfile_path,
            dependency_groups: DIRECT_GROUPS,
            frozen_lockfile: false,
            prefer_frozen_lockfile: Some(false),
            ignore_manifest_check: false,
            skip_runtimes: config.skip_runtimes,
            trust_lockfile: config.trust_lockfile,
            update_checksums: false,
            is_full_install: packages.is_empty(),
            installs_only: true,
            resolved_packages,
            supported_architectures,
            node_linker: config.node_linker,
            lockfile_only,
            dry_run: false,
            update_seed_policy: UpdateSeedPolicy::ByImporter(prepared.seed_policies),
            auth_override: None,
            resolution_observer,
            peer_issues_sink: None,
            catalogs_override: prepared.catalogs_override,
            disable_optimistic_repeat_install: false,
            pnpmfile_hook_override: None,
            workspace_projects_override: None,
        }
        .run_selected::<Reporter>(WorkspaceInstallSelection {
            all_projects: projects,
            ordered_groups,
            ordered_dirs,
            selected_dirs,
            active_manifest_is_standin,
        })
        .await
        .map_err(UpdateError::Install)?;

        persist_selected_manifests::<Reporter>(projects, &prepared.persist_indices)?;
        Ok(())
    }
}

struct UpdatePreparation {
    seed_policy: UpdateSeedPolicy,
    persist_manifest: bool,
    updated_catalogs: Catalogs,
    catalogs_override: Option<Catalogs>,
    workspace_dir_for_catalogs: Option<PathBuf>,
}

struct SelectedUpdatePreparation {
    seed_policies: BTreeMap<String, ImporterUpdateSeedPolicy>,
    persist_indices: Vec<usize>,
    updated_catalogs: Catalogs,
    catalogs_override: Option<Catalogs>,
    workspace_dir_for_catalogs: Option<PathBuf>,
    any_work: bool,
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
    catalogs_seed: Option<&Catalogs>,
    latest_chain: &mut Option<LatestResolverChain>,
    lockfile_only: bool,
    resolution_observer: Option<&Arc<dyn crate::ResolutionObserver>>,
) -> Result<Option<UpdatePreparation>, UpdateError> {
    // `pacquet update` has no `--save-prefix` flag yet, so `save_exact`
    // selects between an exact pin and the default caret range.
    let pinned_version = PinnedVersion::from_save_options(save_exact, None);
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
    let mut drop_names = HashSet::new();
    let mut rewrites = Vec::new();
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
        pinned_version,
        lockfile_only,
    };

    let seed_policy = if selectors.is_empty() {
        // `updateConfig.ignoreDependencies` applies only when no selector was
        // supplied and remains scoped by the included direct groups.
        let ignore_patterns =
            config.update_config.ignore_dependencies.as_deref().unwrap_or_default();
        let ignore_matcher = (!ignore_patterns.is_empty()).then(|| create_matcher(ignore_patterns));
        let is_ignored =
            |name: &str| ignore_matcher.as_ref().is_some_and(|matcher| matcher.matches(name));
        for (name, group, previous) in &direct {
            if is_ignored(name) {
                continue;
            }
            if latest
                && let Some(specifier) =
                    latest_specifier(&rewrite_ctx, latest_chain, &mut catalog_ctx, name, previous)
                        .await?
            {
                rewrites.push((name.clone(), *group, specifier));
            }
            drop_names.insert(name.clone());
        }
        if updates_all_groups && ignore_patterns.is_empty() {
            // A bare, ungated update re-resolves the whole graph.
            UpdateSeedPolicy::DropAll
        } else {
            if updates_all_groups
                && !(latest && drop_names.is_empty())
                && let Some(snapshots) = lockfile.and_then(|lockfile| lockfile.snapshots.as_ref())
            {
                for key in snapshots.keys() {
                    let name = key.name.to_string();
                    if !is_ignored(&name) {
                        drop_names.insert(name);
                    }
                }
            }
            UpdateSeedPolicy::DropOnly(drop_names)
        }
    } else if use_name_matcher {
        let patterns =
            selectors.iter().map(|selector| selector.pattern.clone()).collect::<Vec<_>>();
        let matcher = create_matcher(&patterns);
        for (name, _, _) in &direct {
            if matcher.matches(name) {
                drop_names.insert(name.clone());
            }
        }
        // Lockfile names keep transitive-only matches in the update scope.
        if let Some(snapshots) = lockfile.and_then(|lockfile| lockfile.snapshots.as_ref()) {
            for key in snapshots.keys() {
                let name = key.name.to_string();
                if matcher.matches(&name) {
                    drop_names.insert(name);
                }
            }
        }
        UpdateSeedPolicy::DropOnly(drop_names)
    } else {
        let patterns =
            selectors.iter().map(|selector| selector.pattern.clone()).collect::<Vec<_>>();
        let matcher = create_matcher(&patterns);
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
                for key in snapshots.keys() {
                    let name = key.name.to_string();
                    if matcher.matches(&name) {
                        drop_names.insert(name);
                    }
                }
            }
            for selector in &selectors {
                let Some(version) = selector.version.as_deref() else { continue };
                tracing::warn!(
                    target: "pacquet_package_manager::update",
                    pattern = selector.pattern,
                    version,
                    r#""{}" is not a direct dependency, so the requested version "{version}" is ignored — "{}" is updated to what a fresh install would resolve. To force a version of a transitive dependency, add an override scoped to the range its dependents declare to pnpm-workspace.yaml, e.g.: overrides: {{ "{}@<declared range>": "{version}" }}"#,
                    selector.pattern,
                    selector.pattern,
                    selector.pattern,
                );
            }
        } else {
            for (name, group, previous) in &matched_direct {
                drop_names.insert(name.clone());
                // The two sources are exclusive: `--latest` rejects versioned
                // selectors above, so under it no selector carries a version.
                let rewrite = if latest {
                    latest_specifier(&rewrite_ctx, latest_chain, &mut catalog_ctx, name, previous)
                        .await?
                } else {
                    selectors
                        .iter()
                        .find(|selector| matcher_one(&selector.pattern).matches(name))
                        .and_then(|selector| selector.version.clone())
                };
                if let Some(specifier) = rewrite {
                    rewrites.push((name.clone(), *group, specifier));
                }
            }
        }
        UpdateSeedPolicy::DropOnly(drop_names)
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
        persist_manifest,
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
    projects: &mut [pacquet_workspace::Project],
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
    lockfile_only: bool,
    resolution_observer: Option<&Arc<dyn crate::ResolutionObserver>>,
) -> Result<SelectedUpdatePreparation, UpdateError> {
    // One picker across every selected project: it is created on first
    // use, so a selection that resolves no `latest` tag never builds one.
    let mut latest_chain = None;
    let mut seed_policies = BTreeMap::new();
    let mut persist_indices = Vec::new();
    let mut updated_catalogs = Catalogs::new();
    let mut catalogs_override = None;
    let mut workspace_dir_for_catalogs = None;
    let mut any_work = false;

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
            pacquet_workspace::importer_id_from_root_dir(workspace_root, &projects[index].root_dir);
        match prepared.seed_policy {
            UpdateSeedPolicy::KeepAll => {}
            UpdateSeedPolicy::DropAll => {
                seed_policies.insert(importer_id, ImporterUpdateSeedPolicy::DropAll);
            }
            UpdateSeedPolicy::DropOnly(names) => {
                seed_policies.insert(importer_id, ImporterUpdateSeedPolicy::DropOnly(names));
            }
            UpdateSeedPolicy::ByImporter(_) => {
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

    if depth == 0 && !packages.is_empty() && !latest && !any_work {
        return Err(UpdateError::NoPackageInDependencies);
    }

    Ok(SelectedUpdatePreparation {
        seed_policies,
        persist_indices,
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

fn persist_selected_manifests<Reporter: self::Reporter>(
    projects: &mut [pacquet_workspace::Project],
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

/// Compile a single pattern into a matcher. Used to map a matched direct
/// dependency back to the selector that claimed it (so a versioned
/// selector's version is applied to the right dep).
fn matcher_one(pattern: &str) -> pacquet_config::matcher::Matcher {
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
    let workspace_dir_opt = pacquet_workspace::find_workspace_dir(&manifest_dir)
        .map_err(UpdateError::FindWorkspaceDir)?;
    let catalogs = if let Some(catalogs) = config.catalogs.clone() {
        catalogs
    } else {
        let workspace_manifest = match workspace_dir_opt.as_deref() {
            Some(dir) => pacquet_workspace::read_workspace_manifest(dir)
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
    let workspace_dir_opt = pacquet_workspace::find_workspace_dir(&manifest_dir)
        .map_err(UpdateError::FindWorkspaceDir)?;
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
    pinned_version: PinnedVersion,
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
        pinned_version: Some(ctx.pinned_version),
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
        let named_registries =
            merge_named_registries(&ctx.config.named_registries.clone().into_iter().collect())
                .map_err(UpdateError::InvalidNamedRegistry)?;
        let npm_resolver: Arc<dyn Resolver> = Arc::new(NpmResolver {
            registries: ctx.config.resolved_registries().into_iter().collect(),
            named_registries,
            http_client: Arc::clone(ctx.http_client_arc),
            auth_headers: Arc::clone(&ctx.config.auth_headers),
            meta_cache: Arc::<InMemoryPackageMetaCache>::default(),
            fetch_locker: shared_packument_fetch_locker(),
            picked_manifest_cache: shared_picked_manifest_cache(),
            cache_dir: Some(ctx.config.cache_dir.clone()),
            offline: ctx.config.offline,
            prefer_offline: ctx.config.prefer_offline,
            ignore_missing_time_field: ctx.config.minimum_release_age_ignore_missing_time,
            full_metadata: policy.full_metadata,
            filter_metadata: policy.full_metadata,
            retry_opts: crate::retry_config::retry_opts_from_config(ctx.config),
        });
        let mut node_resolver = NodeResolver::new(Arc::clone(ctx.http_client_arc));
        node_resolver.node_download_mirrors.clone_from(&ctx.config.node_download_mirrors);
        node_resolver.offline = ctx.config.offline;
        let resolver = DefaultResolver::new(vec![
            Box::new(Arc::clone(&npm_resolver)) as Box<dyn Resolver>,
            Box::new(node_resolver),
            Box::new(DenoResolver::new(Arc::clone(ctx.http_client_arc), Arc::clone(&npm_resolver))),
            Box::new(BunResolver::new(Arc::clone(ctx.http_client_arc), Arc::clone(&npm_resolver))),
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
