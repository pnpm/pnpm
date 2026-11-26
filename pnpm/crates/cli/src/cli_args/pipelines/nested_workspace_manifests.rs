//! Warn about installed projects that carry a `pnpm-workspace.yaml` of their
//! own.
//!
//! pnpm reads settings only from the `pnpm-workspace.yaml` at the workspace
//! root, so a nested workspace's `patchedDependencies`, `overrides`, and
//! every other setting it declares do not apply when the outer workspace
//! installs it. A nested manifest whose settings the root manifest already
//! sets loses nothing, so it gets no warning. A project that is also
//! installed on its own, such as a git submodule, keeps such a manifest.

use pnpm_reporter::{
    LogEvent,
    LogLevel,
    PnpmLog,
    Reporter,
};
use pnpm_workspace::{
    RootSettings,
    WORKSPACE_MANIFEST_FILENAME,
};
use std::path::Path;

/// Emit one warning per project in `project_dirs`, other than the workspace
/// root itself, whose directory holds a `pnpm-workspace.yaml` with settings
/// the root manifest does not set. A manifest that cannot be read counts as
/// not covered.
pub(super) fn report_nested_workspace_manifests<'a, Reporter: self::Reporter>(
    workspace_dir: &Path,
    project_dirs: impl IntoIterator<Item = &'a Path>,
) {
    let workspace_dir_normalized = pnpm_fs::lexical_normalize(workspace_dir);
    let prefix = workspace_dir.to_string_lossy().into_owned();
    let nested_dirs = project_dirs
        .into_iter()
        .filter(|dir| pnpm_fs::lexical_normalize(dir) != workspace_dir_normalized)
        .filter(|dir| dir.join(WORKSPACE_MANIFEST_FILENAME).is_file())
        .collect::<Vec<_>>();
    if nested_dirs.is_empty() {
        return;
    }
    let root_settings = RootSettings::read(workspace_dir).ok();
    let mut relative_dirs = nested_dirs
        .into_iter()
        .filter(|dir| !root_settings_cover(root_settings.as_ref(), dir))
        .map(|dir| pnpm_fs::relative_path(workspace_dir, dir).to_string_lossy().replace('\\', "/"))
        .collect::<Vec<_>>();
    relative_dirs.sort();
    for relative_dir in relative_dirs {
        Reporter::emit(&LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Warn,
            message: nested_workspace_manifest_warning(&relative_dir),
            prefix: prefix.clone(),
        }));
    }
}

fn root_settings_cover(root_settings: Option<&RootSettings>, nested_dir: &Path) -> bool {
    let Some(root_settings) = root_settings else {
        return false;
    };
    root_settings.cover(nested_dir).unwrap_or(false)
}

fn nested_workspace_manifest_warning(relative_dir: &str) -> String {
    format!(
        "The settings in {relative_dir}/{WORKSPACE_MANIFEST_FILENAME} do not apply, because {relative_dir} is a project of this workspace. \
         pnpm reads settings only from the {WORKSPACE_MANIFEST_FILENAME} at the workspace root. \
         Move the settings there, or add \"!{relative_dir}\" to the root's \"packages\" to keep that project a separate workspace.",
    )
}
