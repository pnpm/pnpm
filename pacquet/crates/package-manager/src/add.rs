use crate::{
    CatalogDecision, CatalogModeDep, CatalogVersionMismatchError, Install, InstallError,
    ResolvedPackages, UpdateSeedPolicy, decide_catalog,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_catalogs_config::{
    InvalidCatalogsConfigurationError, get_catalogs_from_workspace_manifest,
};
use pacquet_catalogs_types::Catalogs;
use pacquet_config::Config;
use pacquet_lockfile::{Lockfile, MaybeLazyLockfile};
use pacquet_lockfile_preferred_versions::get_preferred_versions_from_lockfile_and_manifests;
use pacquet_network::ThrottledClient;
use pacquet_package_manifest::{DependencyGroup, PackageManifest, PackageManifestError};
use pacquet_registry::{PackageTag, PackageVersion, PinnedVersion};
use pacquet_reporter::{LogEvent, LogLevel, PackageManifestLog, PackageManifestMessage, Reporter};
use pacquet_resolving_git_resolver::{HostedGit, HostedOpts};
use pacquet_resolving_npm_resolver::{
    InMemoryPackageMetaCache, PickPackageContext, PickPackageError, PickPackageOptions,
    parse_bare_specifier, pick_package, pick_registry_for_package, shared_packument_fetch_locker,
    which_version_is_pinned,
};
use pacquet_tarball::MemCache;
use pacquet_workspace_manifest_writer::{UpdateWorkspaceManifestError, update_workspace_manifest};

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
    /// failed.
    #[diagnostic(transparent)]
    WriteWorkspaceManifest(#[error(source)] UpdateWorkspaceManifestError),

    #[diagnostic(transparent)]
    Install(#[error(source)] InstallError),

    /// Fetching a brand-new dependency's `latest` tag from the registry
    /// failed while resolving the version to add.
    #[display("Failed to resolve the latest version of {name}: {error}")]
    #[diagnostic(code(pacquet_package_manager::add_resolve_latest))]
    ResolveLatest {
        name: String,
        #[error(source)]
        error: pacquet_registry::RegistryError,
    },

    /// Resolving an explicit `add <name>@<spec>` specifier against the
    /// registry (to pin the manifest range to a concrete version) failed.
    #[diagnostic(transparent)]
    ResolveSpec(#[error(source)] Box<PickPackageError>),

    /// `minimumReleaseAgeExclude` contained an invalid rule.
    #[diagnostic(transparent)]
    MinimumReleaseAgeExclude(#[error(source)] pacquet_config::version_policy::VersionPolicyError),
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
            pinned_version,
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

        // The dependency's current specifier *in the group(s) this add
        // targets*, so a re-add keeps the existing range / `catalog:`
        // reference of the bucket being written. Scanning only the target
        // groups (rather than every group) avoids preserving a different
        // group's specifier when the same package exists in more than one
        // bucket with different specs.
        let prev_specifier = manifest
            .dependencies(list_dependency_groups())
            .find(|(name, _)| *name == package_name)
            .map(|(_, spec)| spec.to_string());

        // The bare specifier to reconcile against the catalogs:
        // - an explicit `@<version>` is resolved to a concrete version and
        //   recorded with the range operator it (or the existing entry)
        //   pins, mirroring pnpm — `pnpm add foo@^7` records `^7.8.4`, not
        //   `^7`. Specifiers that aren't a plain registry range/tag/version
        //   for this package (protocols, `npm:` aliases) stay verbatim;
        // - a re-add with no version keeps the dependency's current
        //   specifier verbatim (a `catalog:` reference, a range, or an
        //   exact pin), matching pnpm — `pnpm add <existing>` without a
        //   version leaves the declared range untouched;
        // - a brand-new dependency fetches and pins the `latest` range.
        let bare_specifier = match (explicit_spec, prev_specifier.as_deref()) {
            (Some(spec), prev) => resolve_explicit_registry_spec(
                package_name,
                spec,
                prev,
                config,
                http_client,
                pinned_version,
                lockfile_only,
                lockfile,
                manifest,
            )
            .await?
            .unwrap_or_else(|| normalized_save_specifier(spec)),
            (None, Some(prev)) => prev.to_string(),
            (None, None) => {
                let registries: std::collections::HashMap<String, String> =
                    config.resolved_registries().into_iter().collect();
                let registry = pick_registry_for_package(&registries, package_name, None);
                let latest = PackageVersion::fetch_from_registry(
                    package_name,
                    PackageTag::Latest,
                    http_client,
                    &registry,
                    &config.auth_headers,
                )
                .await
                .map_err(|error| AddError::ResolveLatest {
                    name: package_name.to_string(),
                    error,
                })?;
                latest.serialize(pinned_version)
            }
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
            lockfile: MaybeLazyLockfile::Loaded(lockfile),
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
            dry_run: false,
            // `add` keeps every lockfile pin; the freshly-added range
            // is the only thing that re-resolves. `update`'s bump is a
            // separate operation.
            update_seed_policy: UpdateSeedPolicy::KeepAll,
            auth_override: None,
            resolution_observer: None,
            catalogs_override: None,
        }
        .run::<Reporter>()
        .await
        .map_err(AddError::Install)?;

        let updated = manifest.save_and_get_written_value().map_err(AddError::SaveManifest)?;

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
            message: PackageManifestMessage::Updated { prefix, updated },
        }));

        Ok(())
    }
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
#[allow(
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
    lockfile_only: bool,
    lockfile: Option<&Lockfile>,
    manifest: &PackageManifest,
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

    let policy = crate::resolution_policy::PickPolicy::from_config(config)
        .map_err(AddError::MinimumReleaseAgeExclude)?;
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
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        http_client,
        auth_headers: &config.auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(&config.cache_dir),
        offline: config.offline,
        prefer_offline: config.prefer_offline,
        ignore_missing_time_field: config.minimum_release_age_ignore_missing_time,
        full_metadata: policy.full_metadata,
        filter_metadata: policy.full_metadata,
        retry_opts: crate::retry_config::retry_opts_from_config(config),
    };
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
        dry_run: lockfile_only,
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

    // pnpm's calcSpecifier precedence: the existing entry's operator wins
    // over the spec's, which wins over the configured default. Only a
    // registry-style previous specifier carries a meaningful operator —
    // `which_version_is_pinned` forward-scans for a version substring, so a
    // path/URL prev (e.g. `file:../foo-2.0.0.tgz`) would otherwise be misread
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
/// version. Mirrors the separator rule pnpm's `parseWantedDependency` uses.
fn split_name_spec(input: &str) -> (&str, Option<&str>) {
    match input.get(1..).and_then(|rest| rest.find('@')).map(|offset| offset + 1) {
        Some(idx) => (&input[..idx], Some(&input[idx + 1..])),
        None => (input, None),
    }
}

/// The specifier `pacquet add <name>@<spec>` saves when `<spec>` isn't a plain
/// registry range. A hosted-git request — a bare `owner/repo#committish`
/// shorthand or a GitHub / GitLab / Bitbucket URL — is rewritten to its
/// `github:` / `gitlab:` / `bitbucket:` shortcut form, the same
/// `normalizedBareSpecifier` pnpm saves. Everything else (`file:`, `link:`,
/// `workspace:`, `npm:` aliases, tarball URLs) is kept verbatim.
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

#[cfg(test)]
mod tests;
