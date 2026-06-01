use crate::{Install, InstallError, ResolvedPackages};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::Config;
use pacquet_lockfile::Lockfile;
use pacquet_network::ThrottledClient;
use pacquet_package_manifest::{DependencyGroup, PackageManifest, PackageManifestError};
use pacquet_registry::{PackageTag, PackageVersion};
use pacquet_reporter::{LogEvent, LogLevel, PackageManifestLog, PackageManifestMessage, Reporter};
use pacquet_tarball::MemCache;

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
    pub package_name: &'a str, // TODO: 1. support version range, 2. multiple arguments, 3. name this `packages`
    pub save_exact: bool,      // TODO: add `save-exact` to `.npmrc`, merge configs, and remove this
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
    #[display("Failed save the manifest file: {_0}")]
    SaveManifest(#[error(source)] PackageManifestError),
    #[diagnostic(transparent)]
    Install(#[error(source)] InstallError),
}

impl<'a, ListDependencyGroups, DependencyGroupList>
    Add<'a, ListDependencyGroups, DependencyGroupList>
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
            resolved_packages,
            supported_architectures,
            lockfile_only,
        } = self;

        let latest_version = PackageVersion::fetch_from_registry(
            package_name,
            PackageTag::Latest, // TODO: add support for specifying tags
            http_client,
            &config.registry,
            &config.auth_headers,
        )
        .await
        .expect("resolve latest tag"); // TODO: properly propagate this error

        let version_range = latest_version.serialize(save_exact);
        for dependency_group in list_dependency_groups() {
            manifest
                .add_dependency(package_name, &version_range, dependency_group)
                .map_err(AddError::AddDependencyToManifest)?;
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
