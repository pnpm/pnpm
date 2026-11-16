//! The build policy of the global packages: `globalPackagesConfig` in the
//! global `config.yaml`, laid over the decisions the global packages
//! directory's `pnpm-workspace.yaml` still holds.

use super::{
    ApprovalStore, Config, GlobalPackagesStore, Path, Reporter, WorkspaceSettings,
    load_global_packages_config, load_legacy_global_settings,
};
use pnpm_config::GLOBAL_CONFIG_YAML_FILENAME;
use pnpm_reporter::emit_global_warning;

/// Where the build decisions of the global packages are recorded.
pub(super) fn global_approval_store<'a>(
    config: &'a Config,
    global_pkg_dir: &'a Path,
) -> miette::Result<ApprovalStore<'a>> {
    Ok(ApprovalStore::GlobalPackages(GlobalPackagesStore::of(config, global_pkg_dir)?))
}

/// Complete `cfg`'s build policy for a global group. The loader already
/// applied `globalPackagesConfig`, the environment, and the command line, in
/// that order, and dropped the policy of the layers below them. Beneath those
/// come the decisions this command recorded since `cfg` was loaded, which an
/// earlier group may have moved there, then the legacy decisions of the
/// global packages directory's `pnpm-workspace.yaml`.
pub(super) fn apply_global_build_policy(
    cfg: &mut Config,
    global_pkg_dir: &Path,
) -> miette::Result<()> {
    if let Some(config_dir) = cfg.config_dir.clone()
        && let Some(settings) = load_global_packages_config(&config_dir)?
    {
        fill_build_policy(cfg, &settings);
    }
    if let Some((_, settings)) = load_legacy_global_settings(global_pkg_dir)? {
        fill_build_policy(cfg, &settings);
    }
    Ok(())
}

/// The settings of the global packages directory's `pnpm-workspace.yaml`
/// that the global packages still apply, deprecated in favor of
/// `globalPackagesConfig`.
const LEGACY_BUILD_KEYS: &[&str] = &["allowBuilds", "dangerouslyAllowAllBuilds", "permissions"];

/// Warn about the settings the global packages directory's
/// `pnpm-workspace.yaml` holds: the build settings still apply and are
/// deprecated, and every other setting there is ignored.
pub(super) fn warn_about_legacy_global_settings<Reporter: self::Reporter>(
    config: &Config,
    global_pkg_dir: &Path,
) -> miette::Result<()> {
    let Some((path, settings)) = load_legacy_global_settings(global_pkg_dir)? else {
        return Ok(());
    };
    let Ok(serde_json::Value::Object(set)) = serde_json::to_value(&settings) else {
        return Ok(());
    };
    let (build, ignored): (Vec<&str>, Vec<&str>) = set
        .iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(key, _)| key.as_str())
        .partition(|key| LEGACY_BUILD_KEYS.contains(key));
    let global_config = config.config_dir
        .as_deref()
        .map_or_else(
            || GLOBAL_CONFIG_YAML_FILENAME.to_string(),
            |dir| {
                dir.join(GLOBAL_CONFIG_YAML_FILENAME)
                    .display()
                    .to_string()
            },
        );
    let path = path.display();
    if !build.is_empty() {
        emit_global_warning::<Reporter>(&format!(
            "The build settings in {path} are deprecated. Move them under globalPackagesConfig in {global_config}.",
        ));
    }
    if !ignored.is_empty() {
        let ignored = ignored.join(", ");
        emit_global_warning::<Reporter>(&format!(
            "These settings in {path} do not apply to global packages: {ignored}. Set them under globalPackagesConfig in {global_config}.",
        ));
    }
    Ok(())
}

/// Add the decisions of `settings` that no higher layer of `cfg` made.
fn fill_build_policy(cfg: &mut Config, settings: &WorkspaceSettings) {
    for (pkg, allowed) in settings.decided_build_approvals().unwrap_or_default() {
        cfg.allow_builds.entry(pkg).or_insert(allowed);
    }
    if let Some(allow_all) = settings.dangerously_allow_all_builds
        && !cfg.explicit_settings.contains_key("dangerouslyAllowAllBuilds")
    {
        cfg.dangerously_allow_all_builds = allow_all;
    }
}
