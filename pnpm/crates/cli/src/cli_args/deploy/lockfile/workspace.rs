use super::{
    Config, ConvertCtx, DeployFiles, DeployWorkspaceConfig, HashMap, Lockfile, Map, Path, PathBuf,
    Value, relative_path,
};
use miette::IntoDiagnostic;

/// The `pnpm-workspace.yaml` the deploy writes, and the same settings in
/// the shape the deploy install consumes. Only the settings that survive
/// a deploy are carried: patch files, rewritten to paths relative to the
/// deploy dir, and the build allow-list.
pub(super) fn deploy_workspace_settings(
    lockfile: &Lockfile,
    config: &Config,
    lockfile_dir: &Path,
    deploy_dir: &Path,
    deploy_lockfile: &mut Lockfile,
) -> miette::Result<(Map<String, Value>, DeployWorkspaceConfig)> {
    let mut workspace_manifest = Map::new();
    let mut workspace_config = DeployWorkspaceConfig {
        patched_dependencies: None,
        allow_builds: HashMap::new(),
    };
    if lockfile.patched_dependencies.is_some()
        && let Some(patched_dependencies) = config.patched_dependencies.as_ref()
    {
        deploy_lockfile.patched_dependencies.clone_from(&lockfile.patched_dependencies);
        let rewritten = patched_dependencies
            .iter()
            .map(|(name, value)| {
                let absolute = if Path::new(value).is_absolute() {
                    PathBuf::from(value)
                } else {
                    lockfile_dir.join(value)
                };
                (name.clone(), relative_path(deploy_dir, &absolute))
            })
            .collect::<indexmap::IndexMap<_, _>>();
        workspace_manifest.insert(
            "patchedDependencies".to_string(),
            serde_json::to_value(&rewritten).into_diagnostic()?,
        );
        workspace_config.patched_dependencies = Some(rewritten);
    }
    if !config.allow_builds.is_empty() {
        workspace_manifest.insert(
            "allowBuilds".to_string(),
            serde_json::to_value(&config.allow_builds).into_diagnostic()?,
        );
        workspace_config.allow_builds.clone_from(&config.allow_builds);
    }
    Ok((workspace_manifest, workspace_config))
}

pub(super) fn finish_deploy_files(
    lockfile: &Lockfile,
    config: &Config,
    ctx: &ConvertCtx<'_>,
    manifest: Value,
    mut deploy_lockfile: Lockfile,
) -> miette::Result<DeployFiles> {
    let (workspace_manifest, workspace_config) = deploy_workspace_settings(
        lockfile,
        config,
        ctx.lockfile_dir,
        ctx.deploy_dir,
        &mut deploy_lockfile,
    )?;

    Ok(DeployFiles {
        manifest,
        lockfile: deploy_lockfile,
        workspace_manifest: (!workspace_manifest.is_empty()).then_some(Value::Object(
            workspace_manifest,
        )),
        workspace_config,
    })
}
