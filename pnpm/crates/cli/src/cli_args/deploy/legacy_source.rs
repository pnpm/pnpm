use super::{
    Arc, Config, ConvertCtx, DirectSpecs, HashMap, Lockfile, Path, PathBuf, PreferredVersions,
    Reporter, ResolveBases, SelectedProject, State, convert_package_key,
    convert_resolved_dependency_spec, convert_snapshot, create_file_url_key,
    get_preferred_versions_from_lockfile_and_manifests, importer_id_from_root_dir,
    lexical_normalize,
    lockfile::{convert_deploy_packages, importer_package_dirs},
    project_snapshot_to_snapshot_entry, same_path, warn,
};
use pnpm_lockfile::{ImporterDepVersion, SnapshotDepRef};
use pnpm_lockfile_verification::{VerifyLockfileResolutionsOptions, verify_lockfile_resolutions};
use pnpm_package_manager::build_resolution_verifiers;

/// The source workspace lockfile whose pins a legacy deploy prefers.
pub(super) struct LegacyDeploySource {
    pub(super) lockfile: Lockfile,
    path: PathBuf,
}

impl LegacyDeploySource {
    /// The source install's branch-lockfile selection is preserved, while a
    /// missing or malformed source lockfile falls back to fresh resolution.
    pub(super) fn load<ReporterT: Reporter>(
        config: &Config,
        source_lockfile_dir: &Path,
    ) -> Option<Self> {
        if !config.lockfile {
            return None;
        }
        match Lockfile::load_wanted(source_lockfile_dir, &config.wanted_lockfile_selection()) {
            Ok(Some(lockfile)) => Some(Self {
                lockfile,
                path: source_lockfile_dir.join(config.wanted_lockfile_name()),
            }),
            Ok(None) => None,
            Err(error) => {
                warn::<ReporterT>(
                    source_lockfile_dir,
                    format!(
                        "Ignoring broken lockfile at {}: {error}",
                        source_lockfile_dir.display(),
                    ),
                );
                None
            }
        }
    }

    pub(super) fn preferred_versions(&self) -> PreferredVersions {
        get_preferred_versions_from_lockfile_and_manifests(
            self.lockfile.snapshots.as_ref(),
            DirectSpecs::without_catalogs(&[]),
        )
    }

    /// Applies the resolution policies to the source lockfile, as an install
    /// in the source workspace does.
    pub(super) async fn verify<ReporterT: Reporter>(&self, state: &State) -> miette::Result<()> {
        let verifiers = build_resolution_verifiers(
            state.config,
            Arc::clone(&state.http_client),
            None,
            None,
            None,
            None,
            None,
        )
        .map_err(miette::Report::new)?;
        verify_lockfile_resolutions::<ReporterT>(
            &self.lockfile,
            &verifiers,
            &VerifyLockfileResolutionsOptions {
                concurrency: None,
                lockfile_path: Some(&self.path),
                cache_dir: Some(&state.config.cache_dir),
                replaced: None,
                repairs_lockfile: false,
            },
        )
        .await
        .map_err(miette::Report::new)
    }
}

impl LegacyDeploySource {
    pub(super) fn rebase(
        &mut self,
        selected: &SelectedProject,
        deploy_dir: &Path,
        source_dir: &Path,
    ) -> miette::Result<()> {
        let ctx = ConvertCtx {
            projects_by_path: &selected.projects_by_path,
            deploy_dir,
            lockfile_dir: source_dir,
            deployed_project_root: &selected.project.root_dir,
            references: crate::cli_args::deploy::lockfile::ReferenceLocation::Workspace,
        };
        self.rebase_packages(&ctx, selected)?;
        let mut importers = HashMap::new();
        for (id, snapshot) in &self.lockfile.importers {
            let root = lexical_normalize(&source_dir.join(id));
            let snapshot = rebase_importer(snapshot, &ctx, &root, self.lockfile.packages.as_ref())?;
            let id = if same_path(&root, &selected.project.root_dir) {
                Lockfile::ROOT_IMPORTER_KEY.to_string()
            } else {
                importer_id_from_root_dir(deploy_dir, &root)
            };
            importers.insert(id, snapshot);
        }
        self.lockfile.importers = importers;
        Ok(())
    }

    fn rebase_packages(
        &mut self,
        ctx: &ConvertCtx<'_>,
        selected: &SelectedProject,
    ) -> miette::Result<()> {
        let project_id = importer_id_from_root_dir(ctx.lockfile_dir, ctx.deployed_project_root);
        self.lockfile.packages = Some(
            convert_deploy_packages(
                &self.lockfile,
                &project_id,
                ctx.lockfile_dir,
                ctx.deploy_dir,
                selected,
                ctx,
            )?
            .into_iter()
            .map(|(key, metadata)| Ok((relative_file_key(&key, ctx)?, metadata)))
            .collect::<miette::Result<_>>()?,
        );
        self.lockfile.snapshots = self.lockfile.snapshots
            .as_ref()
            .map(|snapshots| {
                snapshots
                    .iter()
                    .map(|(key, snapshot)| {
                        Ok((
                            relative_file_key(&convert_package_key(key, ctx)?, ctx)?,
                            rebase_snapshot(snapshot, ctx)?,
                        ))
                    })
                    .collect::<miette::Result<_>>()
            })
            .transpose()?;
        self.seed_workspace_snapshots(ctx)?;
        Ok(())
    }

    fn seed_workspace_snapshots(&mut self, ctx: &ConvertCtx<'_>) -> miette::Result<()> {
        for (id, importer) in &self.lockfile.importers {
            let root = lexical_normalize(&ctx.lockfile_dir.join(id));
            if same_path(&root, ctx.deployed_project_root) {
                continue;
            }
            let snapshot = workspace_snapshot(importer, ctx, &root)?;
            for directory in importer_package_dirs(&root, importer) {
                let key = relative_file_key(
                    &create_file_url_key(&directory, "", ctx.projects_by_path, None)?,
                    ctx,
                )?;
                self.lockfile.snapshots.get_or_insert_default().insert(key, snapshot.clone());
            }
        }
        Ok(())
    }
}

fn rebase_snapshot(
    snapshot: &super::SnapshotEntry,
    ctx: &ConvertCtx<'_>,
) -> miette::Result<super::SnapshotEntry> {
    let mut converted = convert_snapshot(snapshot, ctx, ctx.lockfile_dir)?;
    for (source, target) in [
        (&snapshot.dependencies, &mut converted.dependencies),
        (&snapshot.optional_dependencies, &mut converted.optional_dependencies),
    ] {
        let Some(target) = target else { continue };
        for (name, reference) in source.iter().flatten() {
            if let SnapshotDepRef::Link(path) = reference {
                target.insert(
                    name.clone(),
                    SnapshotDepRef::Link(
                        lexical_normalize(&ctx.lockfile_dir.join(path))
                            .to_string_lossy()
                            .into_owned(),
                    ),
                );
            }
        }
    }
    rebase_snapshot_keys(&mut converted, ctx)?;
    Ok(converted)
}

fn rebase_importer(
    snapshot: &super::ProjectSnapshot,
    ctx: &ConvertCtx<'_>,
    root: &Path,
    packages: Option<&HashMap<super::PackageKey, super::PackageMetadata>>,
) -> miette::Result<super::ProjectSnapshot> {
    let bases = ResolveBases { file_base: ctx.lockfile_dir, link_base: root };
    let mut snapshot = snapshot.clone();
    for dependencies in [
        &mut snapshot.dependencies,
        &mut snapshot.dev_dependencies,
        &mut snapshot.optional_dependencies,
    ]
    .into_iter()
    .flatten()
    {
        for (name, spec) in dependencies {
            spec.version = rebase_importer_version(name, spec, ctx, &bases, packages)?;
        }
    }
    Ok(snapshot)
}

fn relative_file_key(
    key: &super::PkgNameVerPeer,
    ctx: &ConvertCtx<'_>,
) -> miette::Result<super::PkgNameVerPeer> {
    let super::VersionPart::File(path) = key.suffix.version() else { return Ok(key.clone()) };
    if !path.starts_with("//") {
        return Ok(key.clone());
    }
    let url = url::Url::parse(&format!("file:{path}")).map_err(|error| miette::miette!("{error}"))?;
    let path = url
        .to_file_path()
        .map_err(|()| miette::miette!("could not convert {url} to a file path"))?;
    format!(
        "{}@file:{}{}",
        key.name,
        super::relative_path(ctx.deploy_dir, &path),
        key.suffix.peer(),
    )
    .parse()
    .map_err(|error| miette::miette!("{error}"))
}

fn workspace_snapshot(
    importer: &super::ProjectSnapshot,
    ctx: &ConvertCtx<'_>,
    root: &Path,
) -> miette::Result<super::SnapshotEntry> {
    let bases = ResolveBases { file_base: ctx.lockfile_dir, link_base: root };
    let mut snapshot = project_snapshot_to_snapshot_entry(importer, ctx, &bases)?;
    for (source, target) in [
        (&importer.dependencies, &mut snapshot.dependencies),
        (&importer.optional_dependencies, &mut snapshot.optional_dependencies),
    ] {
        let Some(target) = target else { continue };
        for (name, spec) in source.iter().flatten() {
            if let ImporterDepVersion::Link(path) = &spec.version {
                target.insert(
                    name.clone(),
                    SnapshotDepRef::Link(
                        lexical_normalize(&root.join(path)).to_string_lossy().into_owned(),
                    ),
                );
            }
        }
    }
    rebase_snapshot_keys(&mut snapshot, ctx)?;
    Ok(snapshot)
}

fn rebase_snapshot_keys(
    snapshot: &mut super::SnapshotEntry,
    ctx: &ConvertCtx<'_>,
) -> miette::Result<()> {
    for reference in [&mut snapshot.dependencies, &mut snapshot.optional_dependencies]
        .into_iter()
        .flatten()
        .flat_map(|deps| deps.values_mut())
    {
        if let SnapshotDepRef::Alias(key) = reference {
            *key = relative_file_key(key, ctx)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

fn rebase_importer_version(
    name: &super::PkgName,
    spec: &super::ResolvedDependencySpec,
    ctx: &ConvertCtx<'_>,
    bases: &ResolveBases<'_>,
    packages: Option<&HashMap<super::PackageKey, super::PackageMetadata>>,
) -> miette::Result<ImporterDepVersion> {
    let mut converted = convert_resolved_dependency_spec(name, spec, ctx, bases)?.version;
    if let ImporterDepVersion::Alias(key) = &mut converted {
        *key = relative_file_key(key, ctx)?;
    }
    Ok(match &spec.version {
        ImporterDepVersion::Link(path)
            if converted
                .resolved_key(name)
                .and_then(|key| packages?.get(&key.without_peer()))
                .is_none() =>
        {
            ImporterDepVersion::Link(
                lexical_normalize(&bases.link_base.join(path)).to_string_lossy().into_owned(),
            )
        }
        _ => converted,
    })
}
