use super::super::{
    Arc, HashSet, InstallError, PackageManifest, Path, PathBuf, Reporter,
    dev_preinstall_already_ran, run_dev_preinstall, selected_manifest_freshness_inputs,
};
use pnpm_config::Config;
use pnpm_executor::DEV_PREINSTALL_STAGE;

/// The project manifests as `packageExtensions` and the pnpmfile's
/// `readPackage` rewrote them. An empty layer means the layer below it
/// stands.
#[derive(Default)]
pub(super) struct HookedManifests {
    extended: Vec<(PathBuf, PackageManifest)>,
    hooked: Vec<(PathBuf, PackageManifest)>,
}
impl HookedManifests {
    // pnpm's `getContext` runs `readPackage` over every project
    // manifest before anything reads it, so a hook that rewrites a
    // project's own specifier steers the resolution, the freshness
    // gates, and the importer entries the lockfile records alike.
    // The optimistic repeat-install check above stays on the on-disk
    // manifests on purpose: it is the one gate that must not spawn
    // the Node worker.
    // `packageExtensions` runs ahead of the pnpmfile's `readPackage`,
    // the order the resolver applies them in. The freshness gates below
    // compare against these, because the lockfile they check was written
    // from the extended manifests too — a peer an extension injects into
    // a workspace project is auto-installed and recorded, so a check that
    // read the file on disk would see it as a dependency that vanished.
    pub(super) async fn hook<Reporter: self::Reporter>(
        config: &Config,
        workspace_root: &Path,
        declared: &[(PathBuf, &PackageManifest)],
        pnpmfile_hook: Option<&Arc<dyn pnpm_hooks::PnpmfileHooks>>,
        pre_hooked_paths: &HashSet<PathBuf>,
    ) -> Result<Self, InstallError> {
        let extended = extend_project_manifests(config, declared)?;
        let extended_view = manifests_view(declared, &extended);
        let read_package_log =
            pnpmfile_hook.map(|hook| read_package_log::<Reporter>(hook, workspace_root));
        let every_project_manifest_is_pre_hooked =
            extended_view.iter().all(|(_, manifest)| pre_hooked_paths.contains(manifest.path()));
        let hooked = hook_project_manifests(
            (pnpmfile_hook, read_package_log.as_ref()),
            &extended_view,
            pre_hooked_paths,
            every_project_manifest_is_pre_hooked,
        )
        .await?;
        Ok(Self { extended, hooked })
    }

    pub(super) fn view<'v>(
        &'v self,
        declared: &'v [(PathBuf, &'v PackageManifest)],
    ) -> std::borrow::Cow<'v, [(PathBuf, &'v PackageManifest)]> {
        if self.hooked.is_empty() {
            manifests_view(declared, &self.extended)
        } else {
            manifests_view(declared, &self.hooked)
        }
    }
}
pub(super) fn read_package_log<Reporter: self::Reporter>(
    hook: &Arc<dyn pnpm_hooks::PnpmfileHooks>,
    workspace_root: &Path,
) -> pnpm_hooks::LogFn {
    hook.source_path().map_or_else(
        || Arc::new(|_| {}) as pnpm_hooks::LogFn,
        |from| {
            crate::install_with_fresh_lockfile::hook_log_fn::<Reporter>(
                workspace_root,
                from,
                "readPackage",
            )
        },
    )
}
/// The pnpmfile whose checksum the freshness gates compare against a
/// lockfile's `pnpmfileChecksum`, resolved the way the install that records
/// one resolves it. Building the handle costs a `stat`; the Node worker only
/// starts if a gate has to ask whether the pnpmfile exports hooks.
pub(super) fn resolve_pnpmfile_hook(
    config: &Config,
    workspace_root: &Path,
    override_hook: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
) -> Result<Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>, InstallError> {
    if let Some(hook) = override_hook {
        return Ok(Some(hook));
    }
    if config.ignore_pnpmfile {
        return Ok(None);
    }
    pnpm_hooks::finder::load_pnpmfiles(workspace_root, crate::pnpmfile_selection(config))
        .map_err(InstallError::MissingPnpmfile)
}
/// Borrow the rewritten manifests when a pass produced any, keeping the
/// caller's own list otherwise.
pub(super) fn manifests_view<'v>(
    declared: &'v [(PathBuf, &'v PackageManifest)],
    rewritten: &'v [(PathBuf, PackageManifest)],
) -> std::borrow::Cow<'v, [(PathBuf, &'v PackageManifest)]> {
    if rewritten.is_empty() {
        return std::borrow::Cow::Borrowed(declared);
    }
    std::borrow::Cow::Owned(
        rewritten.iter().map(|(project_dir, manifest)| (project_dir.clone(), manifest)).collect(),
    )
}
/// pnpm's `getContext` runs `readPackage` over every project manifest before
/// anything reads it, so a hook that rewrites a project's own specifier steers
/// the resolution, the freshness gates, and the importer entries the lockfile
/// records alike. Empty when no hook applies or every manifest was hooked
/// already.
pub(super) async fn hook_project_manifests(
    hook: (Option<&Arc<dyn pnpm_hooks::PnpmfileHooks>>, Option<&pnpm_hooks::LogFn>),
    project_manifests: &[(PathBuf, &PackageManifest)],
    pre_hooked_paths: &HashSet<PathBuf>,
    every_manifest_is_pre_hooked: bool,
) -> Result<Vec<(PathBuf, PackageManifest)>, InstallError> {
    let (Some(hook), Some(log)) = hook else { return Ok(Vec::new()) };
    if every_manifest_is_pre_hooked {
        return Ok(Vec::new());
    }
    futures_util::future::try_join_all(project_manifests.iter().map(|(project_dir, manifest)| {
        let ctx = pnpm_hooks::HookContext { log: Arc::clone(log), dir: None };
        let pre_hooked = pre_hooked_paths.contains(manifest.path());
        async move { hook_one_manifest(hook, ctx, project_dir, manifest, pre_hooked).await }
    }))
    .await
}
pub(super) async fn hook_one_manifest(
    hook: &Arc<dyn pnpm_hooks::PnpmfileHooks>,
    ctx: pnpm_hooks::HookContext,
    project_dir: &Path,
    manifest: &PackageManifest,
    pre_hooked: bool,
) -> Result<(PathBuf, PackageManifest), InstallError> {
    if pre_hooked {
        return Ok((project_dir.to_path_buf(), manifest.clone()));
    }
    let value = hook
        .read_package(manifest.value().clone(), ctx)
        .await
        .map_err(InstallError::ReadPackageHook)?;
    let mut hooked = manifest.clone();
    *hooked.value_mut() = (*value).clone();
    Ok((project_dir.to_path_buf(), hooked))
}
/// The `(importer_id, manifest)` pairs the freshness gates compare the
/// lockfile against.
pub(super) fn manifest_freshness_inputs<'a>(
    workspace_root: &Path,
    project_manifests: &[(PathBuf, &'a PackageManifest)],
    selection: Option<&crate::WorkspaceInstallSelection<'_>>,
) -> Vec<(String, &'a PackageManifest)> {
    let Some(selection) = selection else {
        return project_manifests
            .iter()
            .map(|(project_dir, manifest)| {
                (pnpm_workspace::importer_id_from_root_dir(workspace_root, project_dir), *manifest)
            })
            .collect();
    };
    selected_manifest_freshness_inputs(workspace_root, project_manifests, selection.install_dirs)
}
/// What decides whether the `devPreinstall` hook runs.
pub(super) struct DevPreinstallScope<'a> {
    pub(super) config: &'a Config,
    pub(super) workspace_root: &'a Path,
    pub(super) project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub(super) resolve_only: bool,
    pub(super) ignore_manifest_check: bool,
    pub(super) rebuild: Option<&'a crate::RebuildOptions>,
}
/// pnpm reads the hook off the root project's in-memory manifest and only
/// shells out when it is defined. Falling back to the executor's own read
/// covers a root that isn't among the importers, as a filtered install's is
/// not — pnpm's `safeReadProjectManifestOnly` fallback.
pub(super) fn run_dev_preinstall_hook<Reporter: self::Reporter>(
    scope: &DevPreinstallScope<'_>,
) -> Result<(), InstallError> {
    if scope.config.ignore_scripts
        || scope.resolve_only
        || scope.ignore_manifest_check
        || scope.rebuild.is_some()
        || dev_preinstall_already_ran()
    {
        return Ok(());
    }
    let normalized_root = pnpm_fs::lexical_normalize(scope.workspace_root);
    let root_defines_hook = scope
        .project_manifests
        .iter()
        .find(|(project_dir, _)| pnpm_fs::lexical_normalize(project_dir) == normalized_root)
        .is_none_or(|(_, manifest)| {
            matches!(manifest.script(DEV_PREINSTALL_STAGE, true), Ok(Some(_)))
        });
    if !root_defines_hook {
        return Ok(());
    }
    run_dev_preinstall::<Reporter>(scope.config, scope.workspace_root)
}
/// `project_manifests` with `packageExtensions` applied — pnpm's built-in
/// compatibility set and the user's, in that order, matching what the
/// resolver hands the rest of the install.
///
/// Empty when no extension applies, so the caller keeps using the manifests
/// it read from disk rather than a set of identical clones.
pub(super) fn extend_project_manifests(
    config: &Config,
    project_manifests: &[(PathBuf, &PackageManifest)],
) -> Result<Vec<(PathBuf, PackageManifest)>, InstallError> {
    let compat_extender = (!config.ignore_compatibility_db)
        .then(crate::compat_package_extensions::compat_package_extender);
    let extender = match config.package_extensions.as_ref() {
        Some(extensions) => crate::PackageExtender::new(extensions)
            .map(|extender| (!extender.is_empty()).then_some(extender))
            .map_err(InstallError::InvalidPackageExtensionSelector)?,
        None => None,
    };
    let selects = |manifest: &PackageManifest| {
        compat_extender.is_some_and(|extender| extender.matches(manifest.value()))
            || extender.as_ref().is_some_and(|extender| extender.matches(manifest.value()))
    };
    // A workspace project is rarely named by an extension — pnpm's
    // compatibility set names published packages — so this usually finds
    // nothing and the caller keeps the manifests it read from disk.
    if !project_manifests.iter().any(|(_, manifest)| selects(manifest)) {
        return Ok(Vec::new());
    }
    Ok(project_manifests
        .iter()
        .map(|(project_dir, manifest)| {
            let mut extended = (*manifest).clone();
            if let Some(compat_extender) = compat_extender {
                compat_extender.apply(extended.value_mut());
            }
            if let Some(extender) = extender.as_ref() {
                extender.apply(extended.value_mut());
            }
            (project_dir.clone(), extended)
        })
        .collect())
}
