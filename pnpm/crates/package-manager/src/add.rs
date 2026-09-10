mod install;
use install::{
    AddSeed, add_install, merged_catalogs_override, project_seed_policy, selected_add_seed,
};

mod specifier;
use specifier::{normalized_save_specifier, resolve_added_dependency, workspace_packages_for_add};

mod registry;

mod aliasless;

mod manifest;
use manifest::{
    catalog_version_requests, finish_selected_add, persist_manifest, prepare_selected_add,
    prepare_single_add,
};

use crate::{
    CatalogVersionMismatchError, InstallError, ResolvedPackages, SelectedProjects,
    catalog_cleanup::{WriteWorkspaceCatalogsError, post_install_prune},
    defer_ignored_builds,
    resolve_latest::LatestPicker,
    selected_project_indices,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pipe_trait::Pipe;
use pnpm_catalogs_config::InvalidCatalogsConfigurationError;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_engine_runtime_node_resolver::NodeResolverError;
use pnpm_lockfile::Lockfile;
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::{DependencyGroup, PackageManifest, PackageManifestError};
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::Reporter;
use pnpm_resolving_jsr_specifier_parser::ParseJsrSpecifierError;
use pnpm_resolving_local_resolver::ResolveLocalError;
use pnpm_resolving_npm_resolver::{
    InMemoryPackageMetaCache, PackumentFetchLocker, PickPackageError, shared_packument_fetch_locker,
};
use pnpm_resolving_resolver_base::{GitResolveError, WorkspacePackages};
use pnpm_tarball::MemCache;

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
    /// [`RangeSpecStyle::from_save_options`].
    pub range_spec_style: RangeSpecStyle,
    /// `--save-catalog-name=<name>` (with `--save-catalog` a shorthand for
    /// `default`), or the `saveCatalogName` config default. When `Some`,
    /// the added dependency is written as `catalog:` / `catalog:<name>`
    /// and recorded in `pnpm-workspace.yaml` even under
    /// [`pnpm_config::CatalogMode::Manual`].
    pub save_catalog_name: Option<String>,
    /// CLI-merged `supportedArchitectures` forwarded to the
    /// `Install` run that follows the manifest mutation. See
    /// [`Install::supported_architectures`](crate::Install::supported_architectures).
    pub supported_architectures: Option<pnpm_package_is_installable::SupportedArchitectures>,
    /// `--lockfile-only`: add the dependency to the manifest and write
    /// `pnpm-lock.yaml`, but skip materializing `node_modules`. Forwarded
    /// to the follow-up `Install` run. See [`Install::lockfile_only`](crate::Install::lockfile_only).
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
    FindWorkspaceDir(#[error(source)] pnpm_workspace::FindWorkspaceDirError),

    /// Reading `pnpm-workspace.yaml` failed while applying `catalogMode`.
    #[diagnostic(transparent)]
    ReadWorkspaceManifest(#[error(source)] pnpm_workspace::ReadWorkspaceManifestError),

    /// `pnpm-workspace.yaml`'s catalog sections are misconfigured.
    #[diagnostic(transparent)]
    InvalidCatalogsConfiguration(#[error(source)] InvalidCatalogsConfigurationError),

    /// `catalogMode: strict` and the added version disagreed with the
    /// catalog entry for that package.
    #[diagnostic(transparent)]
    CatalogVersionMismatch(#[error(source)] CatalogVersionMismatchError),

    /// Writing the auto-cataloged entry back to `pnpm-workspace.yaml`
    /// (or the `catalogPrune` pass it runs) failed.
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
        source: pnpm_resolving_resolver_base::ResolveError,
    },

    /// The git dependency's `git ls-remote` failed. Kept as the diagnostic the
    /// resolver raised, which already names the specifier and carries the
    /// `ERR_PNPM_GIT_RESOLVE_FAILED` code and its remediation.
    #[diagnostic(transparent)]
    GitResolve(#[error(source)] GitResolveError),

    #[display("Could not determine the package name of git dependency {specifier:?}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_ADD_GIT_PACKAGE_NAME))]
    GitPackageName { specifier: String },

    #[display("Invalid package name {name:?} in git dependency {specifier:?}")]
    #[diagnostic(code(ERR_PNPM_INVALID_PACKAGE_NAME))]
    InvalidGitPackageName { specifier: String, name: String },

    /// Reading the directory or tarball an alias-less local selector names
    /// failed. Kept as the diagnostic the local resolver raised, which
    /// already names the offending path and carries its own code.
    #[diagnostic(transparent)]
    ResolveLocal(#[error(source)] ResolveLocalError),

    /// The tarball fetch itself failed. Kept as the diagnostic the fetcher
    /// raised, so `pnpm add <url>` reports the same code an install of the
    /// same URL does (`ERR_PNPM_TARBALL_HTTP_STATUS`, ...) rather than one
    /// only this command can produce.
    #[diagnostic(transparent)]
    TarballResolve(#[error(source)] Box<pnpm_tarball::TarballError>),

    /// Anything else the tarball resolver raised — a malformed URL, a
    /// transport failure the fetcher doesn't classify.
    ///
    /// The cause is carried as already-redacted text rather than as an
    /// `#[error(source)]`: `reqwest` echoes the request URL back in its own
    /// message, and miette renders every frame of a source chain, so an
    /// attached cause would print the URL this variant took care to redact.
    #[display("Failed to resolve tarball dependency {specifier:?}: {reason}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_ADD_RESOLVE_TARBALL))]
    ResolveTarball {
        #[error(not(source))]
        specifier: String,
        reason: String,
    },

    #[display("Could not determine the package name of dependency {specifier:?}")]
    #[diagnostic(code(ERR_PNPM_MISSING_PACKAGE_NAME))]
    MissingPackageName { specifier: String },

    #[display("Invalid package name {name:?} in dependency {specifier:?}")]
    #[diagnostic(code(ERR_PNPM_INVALID_PACKAGE_NAME))]
    InvalidPackageName { specifier: String, name: String },

    /// Resolving a `node@runtime:<spec>` selector against the Node.js
    /// release index (to pin the manifest to the picked version) failed.
    #[diagnostic(transparent)]
    ResolveRuntimeSpec(#[error(source)] NodeResolverError),

    /// A `jsr:` add selector that names no package, an unscoped one, or
    /// one npm would reject. Kept as the diagnostic the parser raised, so
    /// `pacquet add jsr:foo` reports the same code an install of the same
    /// specifier does.
    #[diagnostic(transparent)]
    ParseJsrSpecifier(#[error(source)] ParseJsrSpecifierError),

    /// `minimumReleaseAgeExclude` contained an invalid rule.
    #[display("Invalid value in minimumReleaseAgeExclude: {_0}")]
    #[diagnostic(code(ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE_EXCLUDE))]
    MinimumReleaseAgeExclude(#[error(source)] pnpm_config::version_policy::VersionPolicyError),
}

impl<'a, DependencyGroupList> Add<'a, DependencyGroupList>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    /// Separate what every step reads from what the install consumes, and
    /// the manifest the add rewrites.
    fn split(self) -> (AddView<'a>, AddOwned, &'a mut PackageManifest) {
        (
            AddView {
                resolved_packages: self.resolved_packages,
                http_client: self.http_client,
                config: self.config,
                lockfile: self.lockfile,
                lockfile_path: self.lockfile_path,
                package_names: self.package_names,
                range_spec_style: self.range_spec_style,
                lockfile_only: self.lockfile_only,
            },
            AddOwned {
                tarball_mem_cache: self.tarball_mem_cache,
                http_client_arc: self.http_client_arc,
                dependency_groups: self
                    .dependency_groups
                    .map(|groups| groups.into_iter().collect()),
                save_catalog_name: self.save_catalog_name,
                supported_architectures: self.supported_architectures,
            },
            self.manifest,
        )
    }

    pub async fn run<Reporter: self::Reporter + 'static>(self) -> Result<(), AddError> {
        let (add, owned, manifest) = self.split();
        begin::<Reporter>(add, &owned);
        let (catalog_ctx, updated_catalogs) =
            prepare_single_add::<Reporter>(add, &owned, manifest).await?;
        let (dropped_pins, preferred_versions_override) = catalog_version_requests(
            add.package_names,
            manifest,
            &catalog_ctx.catalogs,
            add.lockfile,
            add.config,
            owned.save_catalog_name.as_deref(),
        );
        let ignored_builds = add_install(
            add,
            owned,
            manifest,
            AddSeed {
                seed_policies: project_seed_policy(add.config, manifest, dropped_pins),
                preferred_versions_override,
                catalogs_override: merged_catalogs_override(
                    catalog_ctx.catalogs,
                    &updated_catalogs,
                ),
            },
        )
        .run::<Reporter>()
        .await
        .pipe(defer_ignored_builds)
        .map_err(AddError::Install)?;

        persist_manifest::<Reporter>(manifest)?;

        post_install_prune(add.config, Some(&catalog_ctx.workspace_dir), manifest)
            .map_err(AddError::WriteWorkspaceManifest)?;

        if let Some(ignored_builds) = ignored_builds {
            return Err(AddError::Install(ignored_builds));
        }
        Ok(())
    }

    pub async fn run_selected<Reporter: self::Reporter + 'static>(
        self,
        selected: SelectedProjects<'_>,
    ) -> Result<(), AddError> {
        let (add, owned, manifest) = self.split();
        begin::<Reporter>(add, &owned);
        let selected_indices = selected_project_indices(
            selected.projects,
            selected.ordered_dirs,
            selected.selected_dirs,
        );
        if selected_indices.is_empty() {
            return Ok(());
        }
        let prepared =
            prepare_selected_add::<Reporter>(selected.projects, &selected_indices, add, &owned)
                .await?;
        let seed = selected_add_seed(
            add,
            &owned,
            manifest,
            (selected.projects, &selected_indices),
            prepared.catalogs_override,
            &prepared.catalogs,
        );
        let ignored_builds = Box::pin(
            add_install(add, owned, manifest, seed).run_selected::<Reporter>(selected.selection()),
        )
        .await
        .pipe(defer_ignored_builds)
        .map_err(AddError::Install)?;

        finish_selected_add::<Reporter>(
            add,
            manifest,
            selected.projects,
            &selected_indices,
            &prepared.workspace_dir,
            ignored_builds,
        )
    }
}

/// The add's borrowed and `Copy` inputs, as one value every step reads.
#[derive(Clone, Copy)]
struct AddView<'a> {
    resolved_packages: &'a ResolvedPackages,
    http_client: &'a ThrottledClient,
    config: &'static Config,
    lockfile: Option<&'a Lockfile>,
    lockfile_path: Option<&'a std::path::Path>,
    package_names: &'a [String],
    range_spec_style: RangeSpecStyle,
    lockfile_only: bool,
}

/// The add's owned inputs, consumed by the install it runs.
struct AddOwned {
    tarball_mem_cache: std::sync::Arc<MemCache>,
    http_client_arc: std::sync::Arc<ThrottledClient>,
    dependency_groups: Option<Vec<DependencyGroup>>,
    save_catalog_name: Option<String>,
    supported_architectures: Option<pnpm_package_is_installable::SupportedArchitectures>,
}

fn begin<Reporter: self::Reporter>(add: AddView<'_>, owned: &AddOwned) {
    add.http_client.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);
    owned.http_client_arc.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);
}

/// What one add pass shares across the selectors it resolves, and across
/// the projects a selected add touches: the `latest` picker (created on
/// first use, so a pass that resolves no `latest` tag never builds one),
/// the packument cache and the fetch locker.
struct AddResolution<'a> {
    latest_picker: tokio::sync::OnceCell<LatestPicker<'a>>,
    meta_cache: std::sync::Arc<InMemoryPackageMetaCache>,
    fetch_locker: PackumentFetchLocker,
}

impl AddResolution<'_> {
    fn new() -> Self {
        Self {
            latest_picker: tokio::sync::OnceCell::new(),
            meta_cache: std::sync::Arc::new(InMemoryPackageMetaCache::default()),
            fetch_locker: shared_packument_fetch_locker(),
        }
    }
}

/// What every selector of an add resolves against.
struct AddResolveInputs<'a, 'r> {
    add: AddView<'a>,
    http_client_arc: &'r std::sync::Arc<ThrottledClient>,
    /// One checkout per repository and commit for every alias-less git
    /// selector this command resolves.
    git_source_cache: &'r std::sync::Arc<pnpm_git_fetcher::GitSourceCache>,
    resolution: &'r AddResolution<'a>,
    save_catalog_name: Option<&'r str>,
    catalogs: &'r Catalogs,
    prefix: &'r str,
    workspace_packages: Option<&'r WorkspacePackages>,
}

#[cfg(test)]
mod tests;
