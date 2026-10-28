use super::{
    Arc, ResolveDependencyTreeError, ResolveOptions, TreeCtx, link_file_deps_inside_package,
};

pub(super) async fn original_manifest_opts<'a>(
    ctx: &TreeCtx,
    opts: &'a ResolveOptions,
) -> Result<std::borrow::Cow<'a, ResolveOptions>, ResolveDependencyTreeError> {
    if opts.requires_original_manifest || opts.refresh.current_pkg.is_none() {
        return Ok(std::borrow::Cow::Borrowed(opts));
    }
    let Some(hook) = ctx.workspace.hooks.manifests.pnpmfile_hook.as_ref() else {
        return Ok(std::borrow::Cow::Borrowed(opts));
    };
    let has_read_package = ctx.workspace.cache.has_read_package
        .get_or_try_init(|| hook.has_read_package())
        .await
        .map_err(ResolveDependencyTreeError::from)?;
    if !has_read_package {
        return Ok(std::borrow::Cow::Borrowed(opts));
    }
    Ok(std::borrow::Cow::Owned(ResolveOptions { requires_original_manifest: true, ..opts.clone() }))
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
    if !matches!(result.resolution, pnpm_lockfile::LockfileResolution::Directory(_))
        && let Some(manifest) = result.package.manifest.take()
    {
        result.package.manifest = Some(link_file_deps_inside_package(manifest));
    }

    let Some(manifest) = result.package.manifest.take() else {
        return Ok(());
    };
    // Directory resolutions carry their directory so the hook can tell a
    // workspace project's dependency instance apart from a registry
    // manifest — see `HookContext::dir`.
    let hook_ctx = || pnpm_hooks::HookContext {
        log: ctx.workspace.hooks.read_package_log
            .clone()
            .unwrap_or_else(|| Arc::new(|_| {})),
        dir: match &result.resolution {
            pnpm_lockfile::LockfileResolution::Directory(directory_resolution) => {
                Some(directory_resolution.directory.clone())
            }
            _ => None,
        },
    };
    let hooked = ctx.workspace.hooks.manifests
        .apply(manifest, hook_ctx)
        .await
        .map_err(ResolveDependencyTreeError::from)?;
    result.package.manifest = Some(hooked);
    Ok(())
}
