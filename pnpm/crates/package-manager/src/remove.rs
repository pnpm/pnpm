use crate::{
    CommandLockfile, Install, InstallError, ProjectMutation, ResolvedPackages, SelectedProjects,
    UpdateSeedPolicy,
    catalog_cleanup::{
        WriteWorkspaceCatalogsError, post_install_prune, write_workspace_catalogs,
        write_workspace_catalogs_selected,
    },
    defer_ignored_builds, emit_initial_package_manifest, included_direct_groups,
    package_manifest_prefix, selected_project_indices,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pipe_trait::Pipe;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::{DependencyGroup, PackageManifest, PackageManifestError};
use pnpm_reporter::{LogEvent, LogLevel, PackageManifestLog, PackageManifestMessage, Reporter};
use pnpm_tarball::MemCache;
use std::{collections::HashSet, fmt::Write as _, path::PathBuf, sync::Arc};

#[must_use]
pub struct Remove<'a> {
    pub manifest: &'a mut PackageManifest,
    pub options: RemoveOptions<'a>,
    pub resources: RemoveResources,
}

/// The up-front validation failures of `pacquet remove`, raised before
/// the manifest is mutated or any install runs.
///
/// Kept separate from [`RemoveError`] so the `validate_removable` guard
/// returns a small `Result` — folding these into [`RemoveError`] (which
/// carries the large [`InstallError`]) trips `clippy::result_large_err`
/// on the non-async validator.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum RemoveValidationError {
    #[display("At least one dependency name should be specified for removal")]
    #[diagnostic(code(ERR_PNPM_MUST_REMOVE_SOMETHING))]
    MustRemoveSomething,

    #[display("{message}")]
    #[diagnostic(code(ERR_PNPM_CANNOT_REMOVE_MISSING_DEPS))]
    CannotRemoveMissingDeps {
        message: String,
        #[help]
        hint: Option<String>,
    },
}

/// Error type of [`Remove`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum RemoveError {
    #[diagnostic(transparent)]
    Validation(#[error(source)] RemoveValidationError),

    #[display("Failed to save the manifest file: {_0}")]
    SaveManifest(#[error(source)] PackageManifestError),

    /// The `catalogPrune` pass on `pnpm-workspace.yaml` failed.
    #[diagnostic(transparent)]
    WriteWorkspaceManifest(#[error(source)] WriteWorkspaceCatalogsError),

    #[diagnostic(transparent)]
    Install(#[error(source)] InstallError),
}

impl Remove<'_> {
    pub async fn run<Reporter: self::Reporter + 'static>(self) -> Result<(), RemoveError> {
        let Self {
            options: remove,
            resources: owned,
            manifest,
        } = self;
        validate_removable(manifest, remove.package_names, remove.save_type)
            .map_err(RemoveError::Validation)?;
        prepare_manifest::<Reporter>(manifest, remove.package_names, remove.save_type);

        let ignored_builds = remove_install(remove, owned, manifest)
            .run::<Reporter>()
            .await
            .pipe(defer_ignored_builds)
            .map_err(RemoveError::Install)?;

        persist_manifest::<Reporter>(manifest)?;

        write_workspace_catalogs(remove.config, None, &Catalogs::new(), manifest)
            .map_err(RemoveError::WriteWorkspaceManifest)?;

        post_install_prune(remove.config, None, manifest)
            .map_err(RemoveError::WriteWorkspaceManifest)?;

        if let Some(ignored_builds) = ignored_builds {
            return Err(RemoveError::Install(ignored_builds));
        }
        Ok(())
    }

    pub async fn run_selected<Reporter: self::Reporter + 'static>(
        self,
        selected: SelectedProjects<'_>,
    ) -> Result<(), RemoveError> {
        let Self {
            options: remove,
            resources: owned,
            manifest,
        } = self;
        let selected_indices = selected_project_indices(
            selected.projects,
            selected.ordered_dirs,
            selected.selected_dirs,
        );
        if selected_indices.is_empty() {
            return Ok(());
        }

        let edited_dirs = prepare_selected_removal::<Reporter>(
            selected.projects,
            &selected_indices,
            remove.package_names,
            remove.save_type,
        )?;
        let workspace_root = removal_workspace_root(remove.config, manifest);

        let ignored_builds = remove_install(remove, owned, manifest)
            .run_selected::<Reporter>(crate::WorkspaceInstallSelection {
                edited_dirs: Some(&edited_dirs),
                ..selected.selection()
            })
            .await
            .pipe(defer_ignored_builds)
            .map_err(RemoveError::Install)?;

        finalize_selected_remove::<Reporter>(
            selected.projects,
            &selected_indices,
            remove.config,
            &workspace_root,
            manifest,
        )?;
        if let Some(ignored_builds) = ignored_builds {
            return Err(RemoveError::Install(ignored_builds));
        }
        Ok(())
    }
}

/// The removal's borrowed and `Copy` inputs, as one value every step reads.
#[derive(Clone, Copy)]
pub struct RemoveOptions<'a> {
    pub resolved_packages: &'a ResolvedPackages,
    pub http_client: &'a ThrottledClient,
    pub config: &'static Config,
    /// The wanted lockfile, as the command reads it and as the
    /// install it runs needs it.
    pub lockfile: CommandLockfile<'a>,
    /// Names to remove.
    pub package_names: &'a [String],
    /// Dependency field to restrict removal to, or `None` to remove from
    /// any field. Derived from the `--save-prod` / `--save-dev` /
    /// `--save-optional` flags via pnpm's `getSaveType`.
    pub save_type: Option<DependencyGroup>,
    /// `--lockfile-only`: rewrite `pnpm-lock.yaml` (and the manifest) but
    /// skip materializing `node_modules`. Forwarded to the follow-up
    /// `Install` run. See [`crate::InstallExecution::lockfile_only`].
    pub lockfile_only: bool,
}

/// The removal's owned inputs, consumed by the install it runs.
pub struct RemoveResources {
    pub tarball_mem_cache: Arc<MemCache>,
    pub http_client_arc: Arc<ThrottledClient>,
    /// CLI-merged `supportedArchitectures` forwarded to the follow-up
    /// `Install` run. See [`crate::InstallProjects::supported_architectures`].
    pub supported_architectures: Option<pnpm_package_is_installable::SupportedArchitectures>,
}

/// `pnpm remove`'s `include` defaults to every dependency
/// group (`production`/`dev`/`optional` !== false), so the
/// re-resolve walks all three.
/// The manifest was just edited, but the drift is exactly the
/// deleted importer edges, which the removal handler of the
/// lockfile fast path absorbs without resolving. When it
/// declines, the freshness check fails and the install
/// re-resolves as it always did.
/// `pacquet remove` is a partial install (an
/// `uninstallSome` mutation), so the root project's install
/// stages must not run; it runs the uninstall stages instead.
/// Removing a dependency must not bump the survivors: keep
/// every remaining lockfile pin in the preferred-versions
/// seed, same as `install` / `add`.
fn remove_install<'i>(
    remove: RemoveOptions<'i>,
    owned: RemoveResources,
    manifest: &'i PackageManifest,
) -> Install<'i, impl Iterator<Item = DependencyGroup>> {
    Install {
        lockfile_policy: crate::InstallLockfilePolicy::plain(remove.config),
        execution: remove.install_execution(),
        resolution: crate::ResolutionInputs {
            update_seed_policy: UpdateSeedPolicy::KeepAll,
            preferred_versions_override: None,
            auth_override: None,
            observer: None,
            peer_issues_sink: None,
            deps_requiring_build_sink: None,
        },
        context: crate::InstallInvocation {
            http_client: remove.http_client,
            config: remove.config,
            manifest,
            emit_initial_manifest: false,
            lockfile: remove.lockfile.source,
            lockfile_path: remove.lockfile.path,
        },
        fetching: crate::InstallFetching {
            tarball_mem_cache: owned.tarball_mem_cache,
            http_client_arc: owned.http_client_arc,
            resolved_packages: remove.resolved_packages,
        },
        projects: crate::InstallProjects {
            dependency_groups: included_direct_groups(remove.config.optional),
            supported_architectures: owned.supported_architectures,
            catalogs_override: None,
            pnpmfile_hook_override: None,
            workspace_projects_override: None,
        },
    }
}

fn validate_selected_remove(
    projects: &[pnpm_workspace::Project],
    selected_indices: &[usize],
    package_names: &[String],
    save_type: Option<DependencyGroup>,
) -> Result<(), RemoveValidationError> {
    if package_names.is_empty() {
        return Err(RemoveValidationError::MustRemoveSomething);
    }
    let mut available_lookup = HashSet::new();
    let mut available_dependencies = Vec::new();
    for &index in selected_indices {
        let manifest = &projects[index].manifest;
        let peer_dependencies = manifest
            .dependencies([DependencyGroup::Peer])
            .filter(|_| save_type.is_none())
            .map(|(name, _)| name.to_string());
        for dep in manifest
            .available_dependency_names(save_type)
            .into_iter()
            .chain(peer_dependencies)
        {
            if available_lookup.insert(dep.clone()) {
                available_dependencies.push(dep);
            }
        }
    }
    available_dependencies.sort();
    let non_matched_dependencies: Vec<&String> = package_names
        .iter()
        .filter(|name| !available_lookup.contains(name.as_str()))
        .collect();
    if non_matched_dependencies.is_empty() {
        return Ok(());
    }
    Err(cannot_remove_missing_deps(&available_dependencies, &non_matched_dependencies, save_type))
}

fn edited_project_dirs(
    projects: &[pnpm_workspace::Project],
    selected_indices: &[usize],
    package_names: &[String],
    save_type: Option<DependencyGroup>,
) -> HashSet<PathBuf> {
    selected_indices
        .iter()
        .map(|&index| &projects[index])
        .filter(|project| {
            let manifest = &project.manifest;
            let peer_dependencies = manifest
                .dependencies([DependencyGroup::Peer])
                .map(|(name, _)| name.to_string());
            let listed: HashSet<String> = manifest
                .available_dependency_names(save_type)
                .into_iter()
                .chain(peer_dependencies)
                .collect();
            package_names
                .iter()
                .any(|name| listed.contains(name))
        })
        .map(|project| project.root_dir.clone())
        .collect()
}

/// Returns the projects that listed a removed package, read before the edit.
fn prepare_selected_removal<Reporter: self::Reporter>(
    projects: &mut [pnpm_workspace::Project],
    selected_indices: &[usize],
    package_names: &[String],
    save_type: Option<DependencyGroup>,
) -> Result<HashSet<PathBuf>, RemoveError> {
    validate_selected_remove(projects, selected_indices, package_names, save_type)
        .map_err(RemoveError::Validation)?;
    let edited_dirs = edited_project_dirs(projects, selected_indices, package_names, save_type);
    prepare_selected_manifests::<Reporter>(projects, selected_indices, package_names, save_type);
    Ok(edited_dirs)
}

fn prepare_selected_manifests<Reporter: self::Reporter>(
    projects: &mut [pnpm_workspace::Project],
    selected_indices: &[usize],
    package_names: &[String],
    save_type: Option<DependencyGroup>,
) {
    for &index in selected_indices {
        prepare_manifest::<Reporter>(&mut projects[index].manifest, package_names, save_type);
    }
}

fn prepare_manifest<Reporter: self::Reporter>(
    manifest: &mut PackageManifest,
    package_names: &[String],
    save_type: Option<DependencyGroup>,
) {
    emit_initial_package_manifest::<Reporter>(manifest);
    manifest.remove_dependencies(package_names, save_type);
}

fn finalize_selected_remove<Reporter: self::Reporter>(
    projects: &mut [pnpm_workspace::Project],
    selected_indices: &[usize],
    config: &'static Config,
    workspace_root: &std::path::Path,
    manifest: &PackageManifest,
) -> Result<(), RemoveError> {
    persist_selected_manifests::<Reporter>(projects, selected_indices)?;
    write_workspace_catalogs_selected(config, workspace_root, &Catalogs::new(), projects)
        .map_err(RemoveError::WriteWorkspaceManifest)?;
    post_install_prune(config, Some(workspace_root), manifest)
        .map_err(RemoveError::WriteWorkspaceManifest)?;
    Ok(())
}

fn persist_selected_manifests<Reporter: self::Reporter>(
    projects: &mut [pnpm_workspace::Project],
    selected_indices: &[usize],
) -> Result<(), RemoveError> {
    for &index in selected_indices {
        persist_manifest::<Reporter>(&mut projects[index].manifest)?;
    }
    Ok(())
}

fn persist_manifest<Reporter: self::Reporter>(
    manifest: &mut PackageManifest,
) -> Result<(), RemoveError> {
    let updated = manifest.save_and_get_written_value().map_err(RemoveError::SaveManifest)?;
    let prefix = package_manifest_prefix(manifest);
    Reporter::emit(&LogEvent::PackageManifest(PackageManifestLog {
        level: LogLevel::Debug,
        message: PackageManifestMessage::Updated { prefix, updated },
    }));
    Ok(())
}

/// The up-front guards `pacquet remove` applies before mutating the
/// manifest or running any install — both fail fast.
fn validate_removable(
    manifest: &PackageManifest,
    package_names: &[String],
    save_type: Option<DependencyGroup>,
) -> Result<(), RemoveValidationError> {
    if package_names.is_empty() {
        return Err(RemoveValidationError::MustRemoveSomething);
    }
    let available_dependencies = manifest.available_dependency_names(save_type);
    let available_lookup: HashSet<&str> = available_dependencies
        .iter()
        .map(String::as_str)
        .collect();
    let non_matched_dependencies: Vec<&String> = package_names
        .iter()
        .filter(|name| !available_lookup.contains(name.as_str()))
        .collect();
    if non_matched_dependencies.is_empty() {
        return Ok(());
    }
    Err(cannot_remove_missing_deps(&available_dependencies, &non_matched_dependencies, save_type))
}

/// Build the `ERR_PNPM_CANNOT_REMOVE_MISSING_DEPS` error, with its
/// message and hint.
fn cannot_remove_missing_deps(
    available_dependencies: &[String],
    non_matched_dependencies: &[&String],
    target_dependencies_field: Option<DependencyGroup>,
) -> RemoveValidationError {
    let quoted = non_matched_dependencies
        .iter()
        .map(|dep| format!("'{dep}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut message = format!("Cannot remove {quoted}: ");
    if available_dependencies.is_empty() {
        match target_dependencies_field {
            Some(field) => {
                write!(message, "project has no '{}'", <&str>::from(field)).unwrap();
            }
            None => message.push_str("project has no dependencies of any kind"),
        }
        return RemoveValidationError::CannotRemoveMissingDeps { message, hint: None };
    }
    let noun = if non_matched_dependencies.len() > 1 { "dependencies" } else { "dependency" };
    let in_field = target_dependencies_field
        .map(|field| format!(" in '{}'", <&str>::from(field)))
        .unwrap_or_default();
    write!(message, "no such {noun} found{in_field}").unwrap();
    let hint = format!("Available dependencies: {}", available_dependencies.join(", "));
    RemoveValidationError::CannotRemoveMissingDeps { message, hint: Some(hint) }
}

fn removal_workspace_root(config: &Config, manifest: &PackageManifest) -> std::path::PathBuf {
    config.workspace_dir
        .clone()
        .unwrap_or_else(|| {
            manifest
                .path()
                .parent()
                .expect("manifest path always has a parent dir")
                .to_path_buf()
        })
}

#[cfg(test)]
mod tests;

impl RemoveOptions<'_> {
    fn install_execution(self) -> crate::InstallExecution {
        crate::InstallExecution {
            skip_runtimes: self.config.skip_runtimes,
            mutation: ProjectMutation::UninstallSome,
            installs_only: false,
            node_linker: self.config.node_linker,
            lockfile_only: self.lockfile_only,
            dry_run: false,
        }
    }
}
