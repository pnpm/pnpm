use super::{
    LockfileResolution, NoMatchingVersionError, Package, PackageVersion, PickedFromRegistry,
    RegistryPackageSpec, RegistryPackageSpecType, RegistryPick, ResolveError,
    ResolveFromWorkspaceError, ResolveFromWorkspaceOptions, ResolveOptions, ResolveResult,
    SavedSpecifierOptions, TrustPolicy, UpdateBehavior, Version, WantedDependency,
    WorkspacePackages, is_not_found_error, latest_allowed_by_policy, parse_bare_specifier,
    pick_matching_local_version_or_null, redact_and_sanitize, resolve_from_local_package,
    try_resolve_from_workspace_packages,
};

/// Retry a failed registry pick against the workspace. `prefer_workspace_error`
/// decides which error wins when the workspace has the package but not a
/// matching version.
pub(super) fn workspace_fallback(
    workspace_packages: Option<&std::sync::Arc<WorkspacePackages>>,
    spec: &RegistryPackageSpec,
    wanted_dependency: &WantedDependency,
    opts: &ResolveOptions,
    registry_error: ResolveError,
    prefer_workspace_error: bool,
) -> Result<Option<ResolveResult>, ResolveError> {
    let Some(workspace_packages) = workspace_packages else {
        return Err(registry_error);
    };
    match try_workspace_fallback(workspace_packages, spec, wanted_dependency, opts) {
        Ok(result) => Ok(Some(result)),
        Err(ws_err @ ResolveFromWorkspaceError::NoMatchingVersionInsideWorkspace { .. })
            if prefer_workspace_error =>
        {
            Err(Box::new(ws_err))
        }
        Err(_) => Err(registry_error),
    }
}

/// The workspace project that shadows the registry pick, carrying the
/// registry's own `latest` so the reporter can still name it.
pub(super) fn workspace_shadow_pick(
    workspace_packages: Option<&std::sync::Arc<WorkspacePackages>>,
    spec: &RegistryPackageSpec,
    picked: &PickedFromRegistry,
    wanted_dependency: &WantedDependency,
    opts: &ResolveOptions,
) -> Option<ResolveResult> {
    if spec.revision.is_some() {
        return None;
    }
    let mut result =
        try_workspace_shadow(workspace_packages?, spec, &picked.version, wanted_dependency, opts)?;
    result.latest = latest_allowed_by_policy(
        &picked.meta,
        opts.published_by,
        opts.published_by_exclude.as_ref(),
    )
    .map(str::to_string);
    Some(result)
}

/// The local package `preferWorkspacePackages` picks over the registry, when
/// exactly one workspace project answers to the name and one of its versions
/// matches.
///
/// A store-manifest peek, once pacquet grows one, has to run before this fast
/// path — the TypeScript counterpart
/// (`pnpm11/resolving/npm-resolver/src/index.ts`) documents why.
pub(super) fn prefer_workspace_pick(
    workspace_packages: Option<&std::sync::Arc<WorkspacePackages>>,
    spec: &RegistryPackageSpec,
    wanted_dependency: &WantedDependency,
    opts: &ResolveOptions,
) -> Option<ResolveResult> {
    let eligible = opts.prefer_workspace_packages
        && spec.revision.is_none()
        && opts.trust_policy != Some(TrustPolicy::NoDowngrade)
        && !opts.update_checksums
        && !opts.inject_workspace_packages
        && !wanted_dependency.injected.unwrap_or(false);
    if !eligible {
        return None;
    }
    let matching_name = workspace_packages?.get(spec.name.as_str())?;
    if matching_name.len() != 1 {
        return None;
    }
    let local_version = pick_matching_local_version_or_null(matching_name, spec)?;
    let local_package = matching_name.get(&local_version)?;
    Some(resolve_from_local_package(
        local_package,
        wanted_dependency,
        false,
        opts.project_dir.as_path(),
        opts.lockfile_dir.as_path(),
        saved_specifier_options(opts),
    ))
}

/// The spec a wanted dependency names, or `None` when the npm resolver does
/// not claim it.
pub(super) fn wanted_spec(
    wanted_dependency: &WantedDependency,
    default_tag: &str,
    registry: &str,
) -> Option<RegistryPackageSpec> {
    if let Some(bare) = wanted_dependency.bare_specifier.as_deref() {
        return parse_bare_specifier(
            bare,
            wanted_dependency.alias.as_deref(),
            default_tag,
            registry,
        );
    }
    let alias = wanted_dependency.alias.as_deref().filter(|alias| !alias.is_empty())?;
    Some(default_tag_spec(alias, default_tag))
}

/// Whether a latest-version lookup should report "no latest" instead of
/// failing. `minimumReleaseAge` filters the packument before the pick, so
/// every published version can be too young to match — a policy outcome the
/// caller renders as "nothing to update to", not an error.
pub(crate) fn swallowed_as_no_latest(err: &ResolveError, opts: &ResolveOptions) -> bool {
    opts.published_by.is_some() && err.is::<NoMatchingVersionError>()
}

/// The `ERR_PNPM_NO_MATCHING_VERSION` error for a registry that publishes the
/// package but nothing the request accepts.
pub(crate) fn no_matching_version(
    wanted_dependency: &WantedDependency,
    registry: &str,
    meta: &Package,
) -> ResolveError {
    let dep = match wanted_dependency.alias.as_deref() {
        Some(alias) => {
            format!("{alias}@{}", wanted_dependency.bare_specifier.as_deref().unwrap_or_default())
        }
        None => wanted_dependency.bare_specifier.clone().unwrap_or_default(),
    };
    Box::new(NoMatchingVersionError::new(dep, redact_and_sanitize(registry), meta))
}

/// Registry pick was unavailable (no matching version or fetch
/// error); try the workspace as a fallback via
/// [`try_resolve_from_workspace_packages`]. The caller decides which
/// workspace errors to surface and which to swallow in favour of the
/// original registry outcome.
pub(super) fn try_workspace_fallback(
    workspace_packages: &WorkspacePackages,
    spec: &RegistryPackageSpec,
    wanted_dependency: &WantedDependency,
    opts: &ResolveOptions,
) -> Result<ResolveResult, ResolveFromWorkspaceError> {
    let ws_opts = workspace_fallback_options(opts);
    try_resolve_from_workspace_packages(workspace_packages, spec, wanted_dependency, &ws_opts)
}

/// Registry pick succeeded; check whether a workspace package
/// shadows it: exact `name@version` match wins; otherwise a higher
/// workspace version wins; otherwise `preferWorkspacePackages` wins.
pub(super) fn try_workspace_shadow(
    workspace_packages: &WorkspacePackages,
    spec: &RegistryPackageSpec,
    picked: &PackageVersion,
    wanted_dependency: &WantedDependency,
    opts: &ResolveOptions,
) -> Option<ResolveResult> {
    let matching_name = workspace_packages.get(picked.name.as_str())?;
    let hard_link = opts.inject_workspace_packages || wanted_dependency.injected.unwrap_or(false);
    let project_dir = opts.project_dir.as_path();
    let lockfile_dir = opts.lockfile_dir.as_path();

    let picked_version_string = picked.version.to_string();
    if let Some(matched) = matching_name.get(&picked_version_string) {
        return Some(resolve_from_local_package(
            matched,
            wanted_dependency,
            hard_link,
            project_dir,
            lockfile_dir,
            saved_specifier_options(opts),
        ));
    }

    let local_version = pick_matching_local_version_or_null(matching_name, spec)?;
    let local_parsed = Version::parse(&local_version).ok()?;
    let prefer = opts.prefer_workspace_packages || local_parsed > picked.version;
    if !prefer {
        return None;
    }
    let local_package = matching_name.get(&local_version)?;
    Some(resolve_from_local_package(
        local_package,
        wanted_dependency,
        hard_link,
        project_dir,
        lockfile_dir,
        saved_specifier_options(opts),
    ))
}

/// Build the [`ResolveFromWorkspaceOptions`] bag the workspace
/// fallback helper expects. `registry` and `default_tag` are unused on
/// the fallback path (the spec has already been parsed against the
/// registry) so dummy values are passed through.
pub(super) fn workspace_fallback_options(opts: &ResolveOptions) -> ResolveFromWorkspaceOptions<'_> {
    const UNUSED: &str = "";
    ResolveFromWorkspaceOptions {
        project_dir: opts.project_dir.as_path(),
        lockfile_dir: opts.lockfile_dir.as_path(),
        registry: UNUSED,
        default_tag: UNUSED,
        workspace_packages: opts.workspace_packages.as_deref(),
        inject_workspace_packages: opts.inject_workspace_packages,
        saved_specifier: saved_specifier_options(opts),
    }
}

/// Project the specifier-writing knobs out of [`ResolveOptions`] for the
/// workspace entry point, which carries its own options struct.
pub(super) fn saved_specifier_options(opts: &ResolveOptions) -> SavedSpecifierOptions {
    SavedSpecifierOptions {
        calc_specifier: opts.calc_specifier,
        range_spec_style: opts.range_spec_style,
        save_workspace_protocol: opts.save_workspace_protocol,
    }
}

/// `bare_specifier` is absent but `alias` is present: synthesize a tag
/// spec pointing at the default tag.
pub(super) fn default_tag_spec(alias: &str, default_tag: &str) -> RegistryPackageSpec {
    RegistryPackageSpec {
        name: alias.to_string(),
        fetch_spec: default_tag.to_string(),
        spec_type: RegistryPackageSpecType::Tag,
        revision: None,
        normalized_bare_specifier: None,
    }
}

/// The workspace packages a pick may prefer over the registry, when the
/// spec and the update mode allow a workspace resolution to stand.
pub(super) fn workspace_packages_active<'o>(
    opts: &'o ResolveOptions,
    spec: &RegistryPackageSpec,
) -> Option<&'o std::sync::Arc<WorkspacePackages>> {
    let can_keep_workspace_resolution = opts
        .current_pkg
        .as_ref()
        .is_none_or(|current| matches!(current.resolution, LockfileResolution::Directory(_)));
    (spec.revision.is_none()
        && opts.link_workspace_packages.enabled_at_depth(0)
        && (opts.update != UpdateBehavior::Patches || can_keep_workspace_resolution))
        .then_some(opts.workspace_packages.as_ref())
        .flatten()
}

/// The workspace's answer when the registry had none. Neither the
/// registry nor the workspace having a matching version, the workspace
/// mismatch error carries the available local versions, which is the
/// actionable detail (pnpm/pnpm#1379); on a registry error the mismatch
/// is preferred only when the registry said "not found", while auth,
/// network, and server errors propagate as-is.
pub(super) fn workspace_fallback_for(
    outcome: Result<RegistryPick, ResolveError>,
    wanted_dependency: &WantedDependency,
    registry: &str,
    workspace_packages_active: Option<&std::sync::Arc<WorkspacePackages>>,
    spec: &RegistryPackageSpec,
    opts: &ResolveOptions,
) -> Result<Option<ResolveResult>, ResolveError> {
    match outcome {
        Ok(RegistryPick::Picked(_)) => unreachable!("picked outcomes never fall back"),
        Ok(RegistryPick::NoMatchingVersion(meta)) => workspace_fallback(
            workspace_packages_active,
            spec,
            wanted_dependency,
            opts,
            no_matching_version(wanted_dependency, registry, &meta),
            true,
        ),
        Err(err) => {
            let prefer_workspace_error = is_not_found_error(err.as_ref());
            workspace_fallback(
                workspace_packages_active,
                spec,
                wanted_dependency,
                opts,
                err,
                prefer_workspace_error,
            )
        }
    }
}
