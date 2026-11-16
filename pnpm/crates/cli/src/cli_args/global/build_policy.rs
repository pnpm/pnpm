//! The build policy of the global packages: `globalPackages` in the global
//! `config.yaml`, laid over the decisions the global packages directory's
//! `pnpm-workspace.yaml` still holds.

use super::{
    ApprovalStore, Config, GlobalPackagesStore, Path, Reporter, WorkspaceSettings,
    load_legacy_global_settings,
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

/// Replace `cfg`'s build policy with the one of the global packages, so a
/// policy the caller's project set cannot decide which lifecycle scripts a
/// global install runs.
pub(super) fn apply_global_build_policy(
    cfg: &mut Config,
    global_pkg_dir: &Path,
) -> miette::Result<()> {
    cfg.dangerously_allow_all_builds = false;
    cfg.allow_builds.clear();
    if let Some((_, settings)) = load_legacy_global_settings(global_pkg_dir)? {
        overlay_build_policy(cfg, &settings);
    }
    if let Some(settings) = cfg.global_packages.clone() {
        overlay_build_policy(cfg, &settings);
    }
    Ok(())
}

/// Warn when the global packages directory's `pnpm-workspace.yaml` still
/// holds build settings.
pub(super) fn warn_about_legacy_build_settings<Reporter: self::Reporter>(
    config: &Config,
    global_pkg_dir: &Path,
) -> miette::Result<()> {
    let Some((path, settings)) = load_legacy_global_settings(global_pkg_dir)? else {
        return Ok(());
    };
    if settings.allow_builds.is_none()
        && settings.permissions.is_none()
        && settings.dangerously_allow_all_builds.is_none()
    {
        return Ok(());
    }
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
    emit_global_warning::<Reporter>(&format!(
        "The build settings in {path} are deprecated. Move them under globalPackages in {global_config}.",
    ));
    Ok(())
}

fn overlay_build_policy(cfg: &mut Config, settings: &WorkspaceSettings) {
    cfg.allow_builds.extend(settings.decided_build_approvals().unwrap_or_default());
    if let Some(allow_all) = settings.dangerously_allow_all_builds {
        cfg.dangerously_allow_all_builds = allow_all;
    }
}
