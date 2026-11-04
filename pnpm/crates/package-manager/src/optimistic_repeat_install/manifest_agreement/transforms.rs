//! The project manifest the content check compares with the lockfile.

use super::{ManifestStat, OptimisticRepeatInstallCheck, PackageManifest, Path};
use std::{borrow::Cow, sync::Arc};

/// The transforms the resolver applies to a project manifest:
/// `packageExtensions`, the pnpmfile's `readPackage`, then the overrides.
pub(super) struct ManifestTransforms<'a> {
    extender: Option<crate::PackageExtender>,
    hook: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    pub(super) parsed_overrides: Option<&'a [pnpm_config_parse_overrides::VersionOverride]>,
    overrider: Option<crate::VersionsOverrider>,
}

impl<'a> ManifestTransforms<'a> {
    pub(super) fn new(
        check: &OptimisticRepeatInstallCheck<'_>,
        parsed_overrides: Option<&'a [pnpm_config_parse_overrides::VersionOverride]>,
    ) -> Result<Self, &'static str> {
        let extender = check.config.package_extensions
            .as_ref()
            .map(crate::PackageExtender::new)
            .transpose()
            .map_err(|_| "packageExtensions cannot be parsed")?;
        let overrider = parsed_overrides.map(|parsed| {
            crate::VersionsOverrider::new(
                parsed,
                check.config.root_project_manifest_dir(check.workspace_root),
            )
        });
        Ok(Self { extender, hook: read_package_hook(check)?, parsed_overrides, overrider })
    }

    /// Each of `projects`' manifests as the resolver reads it before the
    /// overrides: with the `packageExtensions` that match it, then through
    /// the pnpmfile's `readPackage`.
    pub(super) fn resolver_view<'m>(
        &self,
        projects: &[&ManifestStat<'m>],
    ) -> Result<Vec<Cow<'m, PackageManifest>>, &'static str> {
        let extended: Vec<Cow<'m, PackageManifest>> = projects
            .iter()
            .map(|project| self.extended(project.manifest))
            .collect();
        let Some(hook) = &self.hook else { return Ok(extended) };
        let hooked = projects
            .iter()
            .zip(&extended)
            .map(|(project, manifest)| {
                let ctx = pnpm_hooks::HookContext {
                    log: Arc::new(|_| {}) as pnpm_hooks::LogFn,
                    dir: None,
                };
                crate::install::hook_one_manifest(hook, ctx, project.root_dir, manifest, false)
            });
        let hooked = block_on_hooks(futures_util::future::try_join_all(hooked))
            .ok_or("the readPackage hook cannot run on this thread")?
            .map_err(|_| "the readPackage hook failed")?;
        Ok(hooked
            .into_iter()
            .map(|(_, manifest)| Cow::Owned(manifest))
            .collect())
    }

    /// `manifest` with the `packageExtensions` that match it.
    fn extended<'m>(&self, manifest: &'m PackageManifest) -> std::borrow::Cow<'m, PackageManifest> {
        let Some(extender) = self.extender
            .as_ref()
            .filter(|extender| extender.matches(manifest.value()))
        else {
            return std::borrow::Cow::Borrowed(manifest);
        };
        let mut extended = manifest.clone();
        extender.apply(extended.value_mut());
        std::borrow::Cow::Owned(extended)
    }

    /// The manifest of every directory package `projects` reach in `wanted`,
    /// as the resolver reads it.
    pub(super) fn directory_manifests(
        &self,
        check: &OptimisticRepeatInstallCheck<'_>,
        (wanted, projects): (&pnpm_lockfile::Lockfile, &[&ManifestStat<'_>]),
        workspace_packages: Option<&pnpm_resolving_resolver_base::WorkspacePackages>,
    ) -> Result<crate::install::DirectoryManifests, &'static str> {
        // Reading the manifests may run the hooks, which cannot run on a
        // current-thread runtime, so a lockfile without directory packages
        // returns before that.
        if !resolves_a_directory(wanted) {
            return Ok(crate::install::DirectoryManifests::default());
        }
        let importers: Vec<(String, &PackageManifest)> = projects
            .iter()
            .map(|project| {
                let id = pnpm_workspace::importer_id_from_root_dir(
                    check.workspace_root,
                    project.root_dir,
                );
                (id, project.manifest)
            })
            .collect();
        let inputs = crate::install::LockfileFreshnessInputs {
            lockfile_dir: check.workspace_root,
            manifests: &importers,
            workspace_packages,
            dependency_manifests: crate::install::DependencyManifestSources {
                declared: check.project_manifests.as_slice(),
                injected: check.manifests.dependency_manifests,
            },
            config: check.config,
            catalogs: check.catalogs,
            pnpmfile_hook: self.hook.as_ref(),
            scope: crate::install::FreshnessScope {
                ignore_manifest_check: false,
                allow_missing_dependency_free_importers: false,
                frozen_lockfile: false,
                prune_stale_importers: false,
                patches_only: false,
                locked_registries: None,
            },
        };
        block_on_hooks(crate::install::hooked_importer_directory_manifests(
            wanted,
            &inputs,
            self.parsed_overrides,
        ))
        .ok_or("the readPackage hook cannot run on this thread")?
        .map_err(|_| "a directory dependency's manifest cannot be read")
    }

    pub(super) fn overridden<'m>(
        &self,
        (root_dir, manifest): (&Path, &'m PackageManifest),
    ) -> std::borrow::Cow<'m, PackageManifest> {
        let Some(overrider) = &self.overrider else {
            return std::borrow::Cow::Borrowed(manifest);
        };
        let mut cloned = manifest.clone();
        overrider.apply(&mut cloned, Some(root_dir));
        std::borrow::Cow::Owned(cloned)
    }
}

fn resolves_a_directory(lockfile: &pnpm_lockfile::Lockfile) -> bool {
    lockfile.packages
        .iter()
        .flat_map(|packages| packages.values())
        .any(|metadata| {
            matches!(metadata.resolution, pnpm_lockfile::LockfileResolution::Directory(_))
        })
}

/// The hooks the install runs over the project manifests: the caller's, or
/// the pnpmfiles the configuration finds. Loading a pnpmfile starts no Node
/// worker; the first hook call does.
fn read_package_hook(
    check: &OptimisticRepeatInstallCheck<'_>,
) -> Result<Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>, &'static str> {
    match check.manifests.hooks {
        crate::ManifestHooks::InMemory(hook) => Ok(Some(Arc::clone(hook))),
        crate::ManifestHooks::Pnpmfiles if check.config.ignore_pnpmfile => Ok(None),
        crate::ManifestHooks::Pnpmfiles => {
            crate::load_install_pnpmfiles(check.config, check.workspace_root)
                .map_err(|_| "a pnpmfile cannot be loaded")
        }
    }
}

/// Run the hooks' `future` from the synchronous check. On the multi-thread
/// runtime the check runs on, `block_in_place` keeps the runtime's other
/// tasks running. A thread outside any runtime gets one of its own. `None` on
/// a current-thread runtime, which cannot block one of its own tasks.
fn block_on_hooks<Output>(future: impl Future<Output = Output>) -> Option<Output> {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if pnpm_deps_restorer::on_multi_thread_runtime() => {
            Some(tokio::task::block_in_place(|| handle.block_on(future)))
        }
        Ok(_) => None,
        Err(_) => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()
            .map(|runtime| runtime.block_on(future)),
    }
}
