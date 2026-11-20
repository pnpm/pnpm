use super::{
    Arc, Config, IgnoredHookWarnings, IntoDiagnostic, Path, PnpmfileHooks, Reporter, Result,
    TrustedSettings, Value, execute_update_config_hooks, finder, resolve_pnpmfile_paths,
    seed_hook_input,
};
use miette::WrapErr;
use pnpm_catalogs_types::Catalogs;

/// The catalogs the `updateConfig` hooks for `root_dir` produce when handed
/// `seed` as the workspace catalogs, or `None` when no pnpmfile runs.
/// Nothing else the hooks change is applied to `config`.
pub async fn update_config_hook_catalogs<Reporter: self::Reporter>(
    config: &Config,
    root_dir: &Path,
    seed: &Catalogs,
) -> Result<Option<Catalogs>> {
    let pnpmfiles = resolve_pnpmfile_paths(config, root_dir)
        .map_err(|error| miette::miette!(code = "ERR_PNPM_PNPMFILE_NOT_FOUND", "{error}"))?;
    if pnpmfiles.is_empty() {
        return Ok(None);
    }
    let hooks: Vec<Arc<dyn PnpmfileHooks>> = pnpmfiles
        .iter()
        .cloned()
        .map(finder::load_pnpmfile_at)
        .collect();
    let mut input = TrustedSettings::from_resolved(config)
        .to_json()
        .into_diagnostic()
        .wrap_err("serialize the resolved settings for updateConfig hooks")?;
    let seed = serde_json::to_value(seed).into_diagnostic()?;
    seed_hook_input(&mut input, config, root_dir, &pnpmfiles, seed)?;
    let current = execute_update_config_hooks::<Reporter>(
        &pnpmfiles,
        &hooks,
        input,
        root_dir,
        IgnoredHookWarnings::Skip,
    )
    .await?;
    hook_output_catalogs(&current).map(Some)
}

pub(super) fn hook_output_catalogs(current: &Value) -> Result<Catalogs> {
    Ok(current
        .get("catalogs")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .into_diagnostic()
        .wrap_err("the updateConfig hook produced an invalid catalogs value")?
        .unwrap_or_default())
}
