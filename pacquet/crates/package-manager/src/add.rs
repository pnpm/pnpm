use crate::{
    CatalogDecision, CatalogModeDep, CatalogVersionMismatchError, Install, InstallError,
    ResolvedPackages, UpdateSeedPolicy, decide_catalog,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_catalogs_config::{
    InvalidCatalogsConfigurationError, get_catalogs_from_workspace_manifest,
};
use pacquet_catalogs_protocol_parser::parse_catalog_protocol;
use pacquet_catalogs_types::Catalogs;
use pacquet_config::Config;
use pacquet_lockfile::Lockfile;
use pacquet_network::ThrottledClient;
use pacquet_package_manifest::{DependencyGroup, PackageManifest, PackageManifestError};
use pacquet_registry::{PackageTag, PackageVersion};
use pacquet_reporter::{LogEvent, LogLevel, PackageManifestLog, PackageManifestMessage, Reporter};
use pacquet_tarball::MemCache;
use pacquet_workspace_manifest_writer::{UpdateWorkspaceManifestError, update_workspace_manifest};

/// This subroutine does everything `pacquet add` is supposed to do.
#[must_use]
pub struct Add<'a, ListDependencyGroups, DependencyGroupList>
where
    ListDependencyGroups: Fn() -> DependencyGroupList,
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
    pub list_dependency_groups: ListDependencyGroups, // must be a function because it is called multiple times
    pub package_name: &'a str, // may carry a `@<version>` suffix; TODO: multiple arguments, name this `packages`
    pub save_exact: bool,      // TODO: add `save-exact` to `.npmrc`, merge configs, and remove this
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
    /// failed.
    #[diagnostic(transparent)]
    WriteWorkspaceManifest(#[error(source)] UpdateWorkspaceManifestError),

    #[diagnostic(transparent)]
    Install(#[error(source)] InstallError),
}

impl<ListDependencyGroups, DependencyGroupList> Add<'_, ListDependencyGroups, DependencyGroupList>
where
    ListDependencyGroups: Fn() -> DependencyGroupList,
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
            list_dependency_groups,
            package_name,
            save_exact,
            save_catalog_name,
            resolved_packages,
            supported_architectures,
            lockfile_only,
        } = self;

        let (package_name, explicit_spec) = split_name_spec(package_name);

        // Read the workspace catalogs so `catalogMode` / `--save-catalog`
        // can reconcile the added version against them, mirroring the read
        // pnpm's `installSome` does before resolving. `manifest_dir` is
        // owned so it can outlive the manifest mutation below.
        let manifest_dir =
            manifest.path().parent().expect("manifest path always has a parent dir").to_path_buf();
        let workspace_dir_opt = pacquet_workspace::find_workspace_dir(&manifest_dir)
            .map_err(AddError::FindWorkspaceDir)?;
        let workspace_manifest = match workspace_dir_opt.as_deref() {
            Some(dir) => pacquet_workspace::read_workspace_manifest(dir)
                .map_err(AddError::ReadWorkspaceManifest)?,
            None => None,
        };
        let catalogs = get_catalogs_from_workspace_manifest(workspace_manifest.as_ref())
            .map_err(AddError::InvalidCatalogsConfiguration)?;
        let prefix =
            workspace_dir_opt.as_deref().unwrap_or(&manifest_dir).to_string_lossy().into_owned();

        // The dependency's current manifest specifier, so a re-add of a
        // `catalog:` dependency keeps its catalog reference.
        let prev_specifier = manifest
            .dependencies([
                DependencyGroup::Prod,
                DependencyGroup::Dev,
                DependencyGroup::Optional,
                DependencyGroup::Peer,
            ])
            .find(|(name, _)| *name == package_name)
            .map(|(_, spec)| spec.to_string());

        // The bare specifier to reconcile against the catalogs: the
        // explicit `@<version>`, the preserved `catalog:` reference on a
        // re-add, or otherwise the freshly-fetched `latest` range.
        let bare_specifier = match explicit_spec {
            Some(spec) => spec.to_string(),
            None => match prev_specifier.as_deref() {
                Some(prev) if parse_catalog_protocol(prev).is_some() => prev.to_string(),
                _ => {
                    let latest = PackageVersion::fetch_from_registry(
                        package_name,
                        PackageTag::Latest,
                        http_client,
                        &config.registry,
                        &config.auth_headers,
                    )
                    .await
                    .expect("resolve latest tag"); // TODO: properly propagate this error
                    latest.serialize(save_exact)
                }
            },
        };

        let mut updated_catalogs = Catalogs::new();
        let dep = CatalogModeDep {
            alias: package_name,
            bare_specifier: &bare_specifier,
            prev_specifier: prev_specifier.as_deref(),
        };
        let manifest_specifier = match decide_catalog::<Reporter>(
            config.catalog_mode,
            save_catalog_name.as_deref(),
            &catalogs,
            &dep,
            &prefix,
        )
        .map_err(AddError::CatalogVersionMismatch)?
        {
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

        for dependency_group in list_dependency_groups() {
            manifest
                .add_dependency(package_name, &manifest_specifier, dependency_group)
                .map_err(AddError::AddDependencyToManifest)?;
        }

        // Write the new catalog entry to `pnpm-workspace.yaml` before the
        // install so the resolver reads it back and the lockfile's
        // `catalogs:` snapshot records the resolved version.
        if !updated_catalogs.is_empty() {
            let workspace_dir = workspace_dir_opt.unwrap_or(manifest_dir);
            update_workspace_manifest(&workspace_dir, &updated_catalogs)
                .map_err(AddError::WriteWorkspaceManifest)?;
        }

        Install {
            tarball_mem_cache,
            http_client,
            http_client_arc,
            config,
            manifest,
            lockfile,
            lockfile_path,
            dependency_groups: list_dependency_groups(),
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
            // `pacquet add` is a partial install (pnpm's
            // `mutation: 'installSome'`), so the root project's own
            // lifecycle scripts must not run — mirroring pnpm's
            // `mutation === 'install'` filter.
            is_full_install: false,
            resolved_packages,
            supported_architectures,
            node_linker: config.node_linker,
            lockfile_only,
            // `add` keeps every lockfile pin; the freshly-added range
            // is the only thing that re-resolves. `update`'s bump is a
            // separate operation.
            update_seed_policy: UpdateSeedPolicy::KeepAll,
            auth_override: None,
            resolution_observer: None,
        }
        .run::<Reporter>()
        .await
        .map_err(AddError::Install)?;

        manifest.save().map_err(AddError::SaveManifest)?;

        // `pnpm:package-manifest updated` mirrors pnpm's emit at
        // <https://github.com/pnpm/pnpm/blob/086c5e91e8/installing/deps-resolver/src/index.ts#L238>:
        // fires once after the manifest is rewritten so consumers
        // (e.g. the audit pipeline that diffs initial vs updated)
        // see the post-add shape. `prefix` is the manifest's parent
        // directory, matching what `Install::run` derived for the
        // matching `initial` event.
        //
        // `parent()` only returns `None` when the path has no parent
        // (a root or empty); fall back to the manifest path itself
        // so a degenerate `package.json` placed directly at `/`
        // doesn't crash the post-save emit. `to_string_lossy`
        // coerces non-UTF-8 path bytes to U+FFFD instead of
        // panicking.
        let prefix = manifest
            .path()
            .parent()
            .unwrap_or_else(|| manifest.path())
            .to_string_lossy()
            .into_owned();
        Reporter::emit(&LogEvent::PackageManifest(PackageManifestLog {
            level: LogLevel::Debug,
            message: PackageManifestMessage::Updated { prefix, updated: manifest.value().clone() },
        }));

        Ok(())
    }
}

/// Split a `pacquet add` argument into its package name and optional
/// `@<version>` part. The version separator is the first `@` at or after
/// index 1, so a leading scope `@` (`@scope/pkg`) is never mistaken for a
/// version. Mirrors the separator rule pnpm's `parseWantedDependency` uses.
fn split_name_spec(input: &str) -> (&str, Option<&str>) {
    match input.get(1..).and_then(|rest| rest.find('@')).map(|offset| offset + 1) {
        Some(idx) => (&input[..idx], Some(&input[idx + 1..])),
        None => (input, None),
    }
}
