pub(crate) use latest::is_workspace_local_path_specifier;

mod install;
use install::{
    UnsavedManifests, UpdateSeed, hook_selected_manifests, nothing_to_update,
    run_prepared_selected_update, run_prepared_update,
};

mod prepare;
use prepare::{
    ReadPackageHook, apply_read_package_hook_to_update_manifest, prepare_manifest,
    prepare_selected_manifests, update_read_package_hook,
};

mod workspace;
use workspace::{WorkspaceLinkTarget, workspace_specifier};

mod latest;

use latest::{
    LatestResolverChain, LatestRewriteCtx, emit_latest_ignored, latest_specifier, tag_version,
};

mod catalogs;
use catalogs::CatalogCtx;

mod rewrite;
use rewrite::{MatchedRewriteInputs, record_matched_direct_update};

mod seed_policy;

mod selectors;
use selectors::{parse_selectors, reject_versions_of_indirect_update_specs};

use crate::{
    CatalogVersionMismatchError, InstallError, ProjectMutation, ResolvedPackages,
    WorkspaceInstallSelection, catalog_cleanup::WriteWorkspaceCatalogsError,
    package_manifest_prefix, selected_project_indices,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_catalogs_config::InvalidCatalogsConfigurationError;
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::{DependencyGroup, PackageManifest, PackageManifestError};
use pnpm_reporter::Reporter;
use pnpm_resolving_resolver_base::WorkspacePackages;
use pnpm_tarball::MemCache;
use std::{
    collections::HashSet,
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
///   ([`UpdateSeedPolicy`](crate::UpdateSeedPolicy)) so the resolver re-picks the highest version
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
        begin::<Reporter>(update, &owned);
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
        run_prepared_update::<Reporter>(update, owned, manifest, site, unsaved, prepared).await
    }

    pub async fn run_selected<Reporter: self::Reporter + 'static>(
        self,
        selected: SelectedProjects<'_>,
    ) -> Result<(), UpdateError> {
        let (update, owned, manifest) = self.split();
        begin::<Reporter>(update, &owned);
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
        let prepared = prepare_selected_manifests::<Reporter>(
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
        run_prepared_selected_update::<Reporter>(
            update, owned, manifest, selected, site, unsaved, prepared,
        )
        .await
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

/// Route the clients' warnings through the reporter.
fn begin<Reporter: self::Reporter>(update: UpdateView<'_>, owned: &UpdateOwned) {
    update.http_client.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);
    owned.http_client_arc.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);
}

fn manifest_dir(manifest: &PackageManifest) -> &Path {
    manifest.path().parent().expect("manifest path always has a parent dir")
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

#[cfg(test)]
mod tests;
