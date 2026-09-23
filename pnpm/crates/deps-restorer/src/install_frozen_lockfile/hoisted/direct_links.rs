use super::{
    Config, HoistedLinkerError, NodeLinker, OsStr, Path, PathBuf, SymlinkDirectDependenciesError,
    SymlinkPackageError,
};

pub(crate) fn link_selected_hoisted_direct_dependencies(
    config: &Config,
    lockfile_dir: &Path,
    project_manifests: &[(PathBuf, &pnpm_package_manifest::PackageManifest)],
    direct_dependencies_by_importer_id: &crate::DirectDependenciesByImporterId,
) -> Result<(), HoistedLinkerError> {
    let modules_dir_name =
        config.modules_dir.file_name().unwrap_or_else(|| OsStr::new("node_modules"));
    let root_modules_dir = pnpm_fs::lexical_normalize(&config.modules_dir);
    let link_options = crate::shim_link_options(config, NodeLinker::Hoisted);
    for (project_dir, _) in project_manifests {
        // The workspace root owns the hoisted slot itself, so its own
        // entries are the real directories rather than links to them.
        let is_workspace_root =
            pnpm_fs::lexical_normalize(project_dir) == pnpm_fs::lexical_normalize(lockfile_dir);
        let scope = HoistedLinkScope {
            importer_id: pnpm_workspace::importer_id_from_root_dir(lockfile_dir, project_dir),
            root_modules_dir: &root_modules_dir,
            modules_dir: if is_workspace_root {
                root_modules_dir.clone()
            } else {
                project_dir.join(modules_dir_name)
            },
            is_workspace_root,
        };
        scope.link_direct_dependencies(direct_dependencies_by_importer_id, &link_options)?;
    }
    Ok(())
}

/// One importer's share of the hoisted direct-dependency linking.
struct HoistedLinkScope<'a> {
    importer_id: String,
    /// The workspace root's `node_modules`, where the hoister put the
    /// dependency copy every project reaches by walking up.
    root_modules_dir: &'a Path,
    modules_dir: PathBuf,
    is_workspace_root: bool,
}

impl HoistedLinkScope<'_> {
    fn link_direct_dependencies(
        &self,
        direct_dependencies_by_importer_id: &crate::DirectDependenciesByImporterId,
        link_options: &pnpm_cmd_shim::LinkBinsOptions,
    ) -> Result<(), HoistedLinkerError> {
        let Some(direct_dependencies) = direct_dependencies_by_importer_id.get(&self.importer_id)
        else {
            return Ok(());
        };
        let mut linked_names = Vec::new();
        for (alias, target) in direct_dependencies {
            if self.link_one(alias, target)? {
                linked_names.push(alias.clone());
            }
        }
        crate::link_direct_dep_bins(&self.modules_dir, &linked_names, link_options)
            .map_err(|source| {
                HoistedLinkerError::SymlinkDirectDependencies(
                    SymlinkDirectDependenciesError::LinkBins(source),
                )
            })
    }

    /// `Ok(true)` when the alias now resolves inside the project's own
    /// `node_modules`, so its bins are this importer's to link.
    fn link_one(&self, alias: &str, target: &Path) -> Result<bool, HoistedLinkerError> {
        let link_path =
            crate::safe_join_modules_dir::safe_join_modules_dir(&self.modules_dir, alias)
                .map_err(|source| {
                    self.symlink_failure(alias, SymlinkPackageError::InvalidAlias(source))
                })?;
        // A dependency that won the workspace-root slot is reached by
        // walking up from the project, exactly as it is under pnpm.
        // Repeating it inside the project would give a build a second
        // copy to run lifecycle scripts in. Checked after `link_path` so
        // an unusable alias still reports itself.
        if !self.is_workspace_root
            && pnpm_fs::lexical_normalize(target)
                == pnpm_fs::lexical_normalize(&self.root_modules_dir.join(alias))
        {
            self.remove_root_shadow(alias, target, &link_path)?;
            return Ok(false);
        }
        if pnpm_fs::lexical_normalize(&link_path) == pnpm_fs::lexical_normalize(target) {
            return Ok(true);
        }
        crate::symlink_package(target, &link_path)
            .map_err(|source| self.symlink_failure(alias, source))?;
        Ok(true)
    }

    /// An install that predates the walk-up rule, or one where the
    /// version had lost the root slot, leaves a link shadowing the root
    /// copy — the duplicate the rule exists to avoid. A real directory
    /// is the pruner's to remove, and only ever belongs to a version
    /// that lost the slot.
    fn remove_root_shadow(
        &self,
        alias: &str,
        target: &Path,
        link_path: &Path,
    ) -> Result<(), HoistedLinkerError> {
        // `is_symlink_or_junction`, not `Path::is_symlink`: on Windows
        // `symlink_dir` falls back to a junction when it cannot create a
        // true symlink, and a junction is not a symlink to the stdlib.
        let stale_link = match pnpm_fs::is_symlink_or_junction(link_path) {
            Ok(is_link) => is_link,
            // Nothing to clean up — the common case, and the one
            // `junction::exists` reports as an error rather than
            // `Ok(false)`.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(self.symlink_dir_failure(alias, target, link_path, error)),
        };
        if !stale_link {
            return Ok(());
        }
        pnpm_fs::remove_symlink_dir(link_path)
            .map_err(|error| self.symlink_dir_failure(alias, target, link_path, error))
    }

    fn symlink_failure(&self, alias: &str, source: SymlinkPackageError) -> HoistedLinkerError {
        HoistedLinkerError::SymlinkDirectDependencies(
            SymlinkDirectDependenciesError::SymlinkPackage {
                importer_id: self.importer_id.clone(),
                name: alias.to_owned(),
                source,
            },
        )
    }

    fn symlink_dir_failure(
        &self,
        alias: &str,
        target: &Path,
        link_path: &Path,
        error: std::io::Error,
    ) -> HoistedLinkerError {
        self.symlink_failure(
            alias,
            SymlinkPackageError::SymlinkDir {
                symlink_target: target.to_path_buf(),
                symlink_path: link_path.to_path_buf(),
                error,
            },
        )
    }
}
