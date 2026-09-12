use super::{
    Arc, CurrentPkg, GitResolveError, NoMatchingVersionError, Path, PreferredVersionsOverlay,
    RegistryResponseError, ResolveDependencyTreeError, ResolveError, ResolveOptions, Resolver,
    SharedWorkspaceWantedKey, TreeCtx, WantedDependency, WantedKey, WorkspaceFinalWantedKey,
    lock_recoverable, render_specifier,
};

/// Convert a workspace directory resolution into the representation shared by
/// every importer. A `link:` target is stored relative to the lockfile root;
/// an injected `file:` target already has that representation.
pub(super) fn canonical_workspace_resolution(
    result: &pnpm_resolving_resolver_base::ResolveResult,
    project_dir: &Path,
    lockfile_dir: &Path,
) -> Option<pnpm_resolving_resolver_base::ResolveResult> {
    if result.resolved_via != "workspace" {
        return None;
    }
    let pnpm_lockfile::LockfileResolution::Directory(directory_resolution) = &result.resolution
    else {
        return None;
    };
    if result.id.as_str().strip_prefix("file:") == Some(&directory_resolution.directory) {
        return Some(result.clone());
    }
    if result.id.as_str().strip_prefix("link:") != Some(&directory_resolution.directory) {
        return None;
    }

    let target = Path::new(&directory_resolution.directory);
    let absolute_target = if target.is_absolute() {
        pnpm_fs::lexical_normalize(target)
    } else {
        pnpm_fs::lexical_normalize(&project_dir.join(target))
    };
    let canonical_target = pathdiff::diff_paths(&absolute_target, lockfile_dir)
        .unwrap_or(absolute_target)
        .display()
        .to_string()
        .replace('\\', "/");
    let mut canonical = result.clone();
    canonical.id =
        pnpm_resolving_resolver_base::PkgResolutionId::from(format!("link:{canonical_target}"));
    let pnpm_lockfile::LockfileResolution::Directory(canonical_directory) =
        &mut canonical.resolution
    else {
        unreachable!("the cloned workspace resolution remains a directory")
    };
    canonical_directory.directory = canonical_target;
    Some(canonical)
}

/// Render a canonical workspace resolution for one consuming importer before
/// its manifest hooks run.
pub(super) fn render_workspace_resolution(
    canonical: &pnpm_resolving_resolver_base::ResolveResult,
    anchor: &crate::link_target::ImporterAnchor,
    project_dir: &Path,
    lockfile_dir: &Path,
) -> pnpm_resolving_resolver_base::ResolveResult {
    let mut rendered = canonical.clone();
    let pnpm_lockfile::LockfileResolution::Directory(directory_resolution) =
        &mut rendered.resolution
    else {
        unreachable!("the shared workspace cache contains only directory resolutions")
    };
    if canonical.id.as_str().starts_with("file:") {
        return rendered;
    }

    let target = directory_resolution.directory.as_str();
    let consumer_target = anchor.target_relative_to_importer(target).unwrap_or_else(|| {
        let target = Path::new(target);
        let absolute_target = if target.is_absolute() {
            pnpm_fs::lexical_normalize(target)
        } else {
            pnpm_fs::lexical_normalize(&lockfile_dir.join(target))
        };
        let project_dir = pnpm_fs::lexical_normalize(project_dir);
        pathdiff::diff_paths(&absolute_target, project_dir)
            .unwrap_or(absolute_target)
            .display()
            .to_string()
            .replace('\\', "/")
    });
    rendered.id =
        pnpm_resolving_resolver_base::PkgResolutionId::from(format!("link:{consumer_target}"));
    directory_resolution.directory = consumer_target;
    rendered
}

/// Remove the consumer directory from a named `workspace:` request while
/// retaining every other input the explicit workspace resolver reads.
pub(super) fn shared_workspace_key(
    ctx: &TreeCtx,
    cache_key: &WantedKey,
    wanted: &WantedDependency,
    opts: &ResolveOptions,
) -> Option<SharedWorkspaceWantedKey> {
    #[cfg(not(debug_assertions))]
    let _ = opts;
    let bare_specifier = wanted.bare_specifier.as_deref()?;
    if !bare_specifier.starts_with("workspace:") || bare_specifier.starts_with("workspace:.") {
        return None;
    }
    #[cfg(debug_assertions)]
    {
        let importer_key_covers_edge = ctx.workspace_resolution_options_key.matches_options(opts);
        debug_assert!(
            importer_key_covers_edge,
            "the importer-wide workspace-resolution key must describe every edge's options",
        );
    }
    // The consumer scope is exactly what the shared key's hash and
    // equality drop. Every `workspace:` selector carries one, so its
    // absence means this edge is not the shape assumed here.
    cache_key.fields().6.as_ref()?;
    Some(SharedWorkspaceWantedKey::new(
        cache_key.clone(),
        wanted.prev_specifier.clone(),
        &ctx.workspace_resolution_options_key,
    ))
}

/// Look the wanted edge up in the per-wanted dedup cache or run the resolver
/// chain and the manifest-hook pipeline, caching the `Arc<ResolveResult>`
/// under `cache_key`. Eligible named workspace selectors add two more layers:
/// a canonical resolver result, shared by every importer, and a hook-processed
/// result keyed by the `link:` this importer renders, shared only with the
/// importers that render the same one. A second importer therefore always
/// skips the resolver chain, and skips the hooks as well when its rendered
/// link matches. Concurrent first-callers can both miss and resolve in parallel —
/// the resolver's own per-cache-key fetch locker coalesces the network work,
/// and the second `or_insert` loses the race harmlessly.
pub(super) async fn resolve_wanted_cached<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    wanted: &WantedDependency,
    opts: &ResolveOptions,
    pick_overlay: Option<&Arc<PreferredVersionsOverlay>>,
    cache_key: WantedKey,
) -> Result<Arc<pnpm_resolving_resolver_base::ResolveResult>, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    let cached =
        lock_recoverable(&ctx.workspace.resolved_by_wanted).get(&cache_key).map(Arc::clone);
    if let Some(result) = cached {
        return Ok(result);
    }
    let owned_opts = per_wanted_opts(opts, pick_overlay, &cache_key);
    let opts = owned_opts.as_ref().unwrap_or(opts);
    let shared_workspace_key = shared_workspace_cache_key(ctx, &cache_key, wanted, opts);
    let cached_workspace = shared_workspace_key.as_ref().and_then(|key| {
        lock_recoverable(&ctx.workspace.resolved_workspace_by_wanted).get(key).map(Arc::clone)
    });
    let mut canonical_workspace = cached_workspace;
    let mut result = resolve_or_reuse_workspace(
        ctx,
        resolver,
        wanted,
        opts,
        shared_workspace_key.as_ref(),
        &mut canonical_workspace,
    )
    .await?;
    let workspace_final_key =
        workspace_result_key(shared_workspace_key, canonical_workspace.as_deref(), &result.id);
    if let Some(key) = workspace_final_key.as_ref()
        && let Some(cached) = lock_recoverable(&ctx.workspace.resolved_workspace_final_by_wanted)
            .get(key)
            .map(Arc::clone)
    {
        // Both return paths record the project-scoped entry, so the lookup at
        // the top of this function stays authoritative: a repeat of this edge
        // costs one lookup rather than a shared-key rebuild and a re-render.
        lock_recoverable(&ctx.workspace.resolved_by_wanted)
            .entry(cache_key)
            .or_insert_with(|| Arc::clone(&cached));
        return Ok(cached);
    }
    if result.manifest.is_none() {
        result.manifest = Some(Arc::new(fallback_manifest(wanted, opts.current_pkg.as_ref())));
    }
    apply_manifest_hooks(ctx, &mut result).await?;

    Ok(cache_resolved_wanted(ctx, cache_key, workspace_final_key, result))
}

/// Apply the configured manifest hooks to the resolved manifest fragment
/// before anything downstream sees it. `readPackageHook` (today:
/// `packageExtensions`) clones the inner `Value` only when it modifies it, so
/// unrelated manifests keep sharing the resolver's cached `Arc`. Overrides run
/// last so a pnpmfile hook that replaced the manifest cannot erase them — see
/// `WorkspaceTreeCtx::overrides_hook`.
pub(super) async fn apply_manifest_hooks(
    ctx: &TreeCtx,
    result: &mut pnpm_resolving_resolver_base::ResolveResult,
) -> Result<(), ResolveDependencyTreeError> {
    if let Some(hook) = ctx.workspace.manifest_hook.as_ref()
        && let Some(manifest) = result.manifest.take()
    {
        result.manifest = Some(hook(manifest));
    }

    if let Some(pnpmfile_hook) = ctx.workspace.pnpmfile_hook.as_ref()
        && let Some(manifest) = result.manifest.take()
    {
        let log = ctx.workspace.read_package_log.clone().unwrap_or_else(|| Arc::new(|_| {}));
        // Directory resolutions carry their directory so the hook can tell a
        // workspace project's dependency instance apart from a registry
        // manifest — see `HookContext::dir`.
        let dir = match &result.resolution {
            pnpm_lockfile::LockfileResolution::Directory(directory_resolution) => {
                Some(directory_resolution.directory.clone())
            }
            _ => None,
        };
        let hook_ctx = pnpm_hooks::HookContext { log, dir };

        let updated = pnpmfile_hook
            .read_package((*manifest).clone(), hook_ctx)
            .await
            .map_err(ResolveDependencyTreeError::PnpmfileHook)?;
        result.manifest = Some(updated);
    }

    if let Some(hook) = ctx.workspace.overrides_hook.as_ref()
        && let Some(manifest) = result.manifest.take()
    {
        result.manifest = Some(hook(manifest));
    }
    Ok(())
}

/// Render the shared workspace resolution when one is already cached for this
/// edge, otherwise run the resolver chain and record the canonical form the
/// next importer renders from.
pub(super) async fn resolve_or_reuse_workspace<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    wanted: &WantedDependency,
    opts: &ResolveOptions,
    shared_workspace_key: Option<&SharedWorkspaceWantedKey>,
    canonical_workspace: &mut Option<Arc<pnpm_resolving_resolver_base::ResolveResult>>,
) -> Result<pnpm_resolving_resolver_base::ResolveResult, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    if let Some(canonical) = canonical_workspace.as_deref() {
        // A `workspace:` edge never runs under the per-edge project-dir
        // override (that fires for `file:` specifiers only), so the
        // importer-wide anchor is exactly this edge's anchor.
        #[cfg(debug_assertions)]
        {
            let anchor_inputs_describe_this_edge = opts.project_dir == ctx.base_opts.project_dir
                && opts.lockfile_dir == ctx.base_opts.lockfile_dir;
            debug_assert!(
                anchor_inputs_describe_this_edge,
                "the importer-wide link anchor must describe every workspace edge",
            );
        }
        return Ok(render_workspace_resolution(
            canonical,
            &ctx.base_link_anchor,
            &opts.project_dir,
            &opts.lockfile_dir,
        ));
    }
    let result = resolver.resolve(wanted, opts).await.map_err(map_resolve_error)?;
    let Some(result) = result else {
        return Err(ResolveDependencyTreeError::SpecNotSupported {
            specifier: render_specifier(wanted),
        });
    };
    if let Some(shared_workspace_key) = shared_workspace_key
        && let Some(canonical) =
            canonical_workspace_resolution(&result, &opts.project_dir, &opts.lockfile_dir)
    {
        let canonical = Arc::new(canonical);
        *canonical_workspace = Some(Arc::clone(
            lock_recoverable(&ctx.workspace.resolved_workspace_by_wanted)
                .entry(shared_workspace_key.clone())
                .or_insert(canonical),
        ));
    }
    Ok(result)
}

/// Combine two per-package opts adjustments into one clone, or `None` when
/// neither applies. The `update_requested` flag is scoped per
/// wanted-dependency — true only when the package's real name (parsed from
/// `bare_specifier` for npm-aliases, folded from the jsr specifier for jsr
/// deps) is in the update target list — so the picker's held-back-update
/// warning fires only for the packages the user actually asked to update.
pub(super) fn per_wanted_opts(
    opts: &ResolveOptions,
    pick_overlay: Option<&Arc<PreferredVersionsOverlay>>,
    cache_key: &WantedKey,
) -> Option<ResolveOptions> {
    let needs_overlay = !cache_key.fields().8.is_empty();
    let update_target = cache_key.fields().10;
    let needs_update = update_target != opts.update_requested;
    if !needs_overlay && !needs_update {
        return None;
    }
    let mut owned = opts.clone();
    if needs_overlay {
        owned.preferred_versions_overlay = pick_overlay.map(Arc::clone);
    }
    if needs_update {
        owned.update_requested = update_target;
    }
    Some(owned)
}

/// Stand in for the manifest a resolver didn't supply (pnpm's
/// `getManifestFromResponse`).
///
/// Every consumer downstream of the resolver chain reads the package's
/// identity off its manifest — most of all
/// [`build_pkg_id_with_patch_hash`](crate::resolve_dependency_tree::manifest::build_pkg_id_with_patch_hash), which has no name to prefix the dep
/// path with when there is none, leaving a bare `file:<path>` / URL that
/// keys no `packages:` row. A package with no `package.json` of its own
/// still has to install, so it borrows an identity; `0.0.0` is the
/// version pnpm writes into `packages:` for such a package.
pub(super) fn fallback_manifest(
    wanted: &WantedDependency,
    current_pkg: Option<&CurrentPkg>,
) -> pnpm_resolving_resolver_base::DependencyManifest {
    if let Some(current) = current_pkg
        && let Some(name) = current.name.as_deref().filter(|name| !name.is_empty())
        && let Some(version) = current.version.as_deref().filter(|version| !version.is_empty())
    {
        return serde_json::json!({ "name": name, "version": version });
    }
    // A specifier's last path segment is the closest thing to a name an
    // unaliased dep carries: `file:./no-manifest-1.0.0.tgz` and
    // `https://host/no-manifest-1.0.0.tgz` both name the archive.
    let name = if let Some(alias) = wanted.alias.as_deref().filter(|alias| !alias.is_empty()) {
        alias
    } else {
        let specifier = wanted.bare_specifier.as_deref().unwrap_or_default();
        specifier.rsplit('/').next().unwrap_or_default()
    };
    serde_json::json!({ "name": name, "version": "0.0.0" })
}

/// Wrap a resolver-chain failure, keeping the pnpm error code of the ones
/// that carry one. The chain hands back a type-erased
/// [`ResolveError`], which drops the `miette::Diagnostic` facet, so the codes
/// that are part of pnpm's public contract are recovered by downcast; every
/// other failure keeps the generic envelope.
pub(super) fn map_resolve_error(err: ResolveError) -> ResolveDependencyTreeError {
    let err = match err.downcast::<NoMatchingVersionError>() {
        Ok(no_matching_version) => {
            return ResolveDependencyTreeError::NoMatchingVersion(*no_matching_version);
        }
        Err(err) => err,
    };
    let err = match err.downcast::<RegistryResponseError>() {
        Ok(response) => return ResolveDependencyTreeError::RegistryResponse(*response),
        Err(err) => err,
    };
    match err.downcast::<GitResolveError>() {
        Ok(git) => ResolveDependencyTreeError::GitResolve(*git),
        Err(err) => ResolveDependencyTreeError::Resolve(err.to_string()),
    }
}

/// Share one resolved result across both wanted caches and the graph envelopes.
pub(super) fn cache_resolved_wanted(
    ctx: &TreeCtx,
    cache_key: WantedKey,
    workspace_final_key: Option<WorkspaceFinalWantedKey>,
    result: pnpm_resolving_resolver_base::ResolveResult,
) -> Arc<pnpm_resolving_resolver_base::ResolveResult> {
    let result = Arc::new(result);
    if let Some(key) = workspace_final_key {
        lock_recoverable(&ctx.workspace.resolved_workspace_final_by_wanted)
            .entry(key)
            .or_insert_with(|| Arc::clone(&result));
    }
    lock_recoverable(&ctx.workspace.resolved_by_wanted)
        .entry(cache_key)
        .or_insert_with(|| Arc::clone(&result));
    result
}

pub(super) fn shared_workspace_cache_key(
    ctx: &TreeCtx,
    cache_key: &WantedKey,
    wanted: &WantedDependency,
    opts: &ResolveOptions,
) -> Option<SharedWorkspaceWantedKey> {
    ctx.workspace
        .share_workspace_resolutions
        .then(|| shared_workspace_key(ctx, cache_key, wanted, opts))
        .flatten()
}

pub(super) fn workspace_result_key(
    shared_workspace_key: Option<SharedWorkspaceWantedKey>,
    canonical_workspace: Option<&pnpm_resolving_resolver_base::ResolveResult>,
    result_id: &pnpm_resolving_resolver_base::PkgResolutionId,
) -> Option<WorkspaceFinalWantedKey> {
    match (shared_workspace_key, canonical_workspace) {
        (Some(shared_wanted), Some(canonical)) => {
            Some(WorkspaceFinalWantedKey::new(shared_wanted, &canonical.id, result_id))
        }
        _ => None,
    }
}
