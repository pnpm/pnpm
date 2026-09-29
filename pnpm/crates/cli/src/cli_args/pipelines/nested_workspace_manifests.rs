//! Warn about installed projects that carry a `pnpm-workspace.yaml` of their
//! own.
//!
//! pnpm reads settings only from the `pnpm-workspace.yaml` at the workspace
//! root, so a nested workspace's `patchedDependencies`, `overrides`, and
//! every other setting it declares do not apply when the outer workspace
//! installs it. The one exception is a project that keeps its own lockfile:
//! it resolves `catalog:` against the catalogs of its own manifest, see
//! [`pnpm_config::Config::adopt_project_catalogs`].

use pnpm_config::known_settings::SCHEMA_DIRECTIVE_KEY;
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_workspace::WORKSPACE_MANIFEST_FILENAME;
use std::path::Path;

/// The keys of a project's own manifest that apply when the project keeps
/// its own lockfile. `packages` names the projects of the workspace the
/// project is when installed on its own, so it has nothing to apply here.
const KEYS_A_PROJECT_WITH_ITS_OWN_LOCKFILE_READS: &[&str] =
    &["catalog", "catalogs", "extends", "packages", SCHEMA_DIRECTIVE_KEY];

/// Emit one warning per project in `project_dirs`, other than the workspace
/// root itself, whose directory holds a `pnpm-workspace.yaml` with settings
/// that do not apply. With `projects_keep_own_lockfiles`, a manifest that
/// declares nothing but catalogs gets none.
pub(super) fn report_nested_workspace_manifests<'a, Reporter: self::Reporter>(
    workspace_dir: &Path,
    project_dirs: impl IntoIterator<Item = &'a Path>,
    projects_keep_own_lockfiles: bool,
) {
    let workspace_dir_normalized = pnpm_fs::lexical_normalize(workspace_dir);
    let prefix = workspace_dir.to_string_lossy().into_owned();
    let mut warnings = project_dirs
        .into_iter()
        .filter(|dir| pnpm_fs::lexical_normalize(dir) != workspace_dir_normalized)
        .filter(|dir| dir.join(WORKSPACE_MANIFEST_FILENAME).is_file())
        .filter_map(|dir| {
            let relative_dir =
                pnpm_fs::relative_path(workspace_dir, dir).to_string_lossy().replace('\\', "/");
            if !projects_keep_own_lockfiles {
                return Some(nested_workspace_manifest_warning(&relative_dir));
            }
            let ignored = ignored_settings(dir)?;
            (!ignored.is_empty()).then(|| {
                own_lockfile_nested_manifest_warning(&relative_dir, &ignored)
            })
        })
        .collect::<Vec<_>>();
    warnings.sort();
    for message in warnings {
        Reporter::emit(&LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Warn,
            message,
            prefix: prefix.clone(),
        }));
    }
}

/// The top-level keys of the manifest in `dir` that a project keeping its
/// own lockfile does not read. `None` when the manifest does not parse,
/// which the project's install reports.
fn ignored_settings(dir: &Path) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(dir.join(WORKSPACE_MANIFEST_FILENAME)).ok()?;
    if text.trim().is_empty() {
        return Some(Vec::new());
    }
    let document: serde_json::Value = serde_saphyr::from_str(&text).ok()?;
    Some(
        document
            .as_object()?
            .keys()
            .filter(|key| !KEYS_A_PROJECT_WITH_ITS_OWN_LOCKFILE_READS.contains(&key.as_str()))
            .cloned()
            .collect(),
    )
}

fn nested_workspace_manifest_warning(relative_dir: &str) -> String {
    format!(
        "The settings in {relative_dir}/{WORKSPACE_MANIFEST_FILENAME} do not apply, because {relative_dir} is a project of this workspace. \
         pnpm reads settings only from the {WORKSPACE_MANIFEST_FILENAME} at the workspace root. \
         Move the settings there, or add \"!{relative_dir}\" to the root's \"packages\" to keep that project a separate workspace.",
    )
}

fn own_lockfile_nested_manifest_warning(relative_dir: &str, ignored: &[String]) -> String {
    let settings = ignored
        .iter()
        .map(|key| format!(r#""{key}""#))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "The settings {settings} in {relative_dir}/{WORKSPACE_MANIFEST_FILENAME} do not apply, because {relative_dir} is a project of this workspace. \
         A project that keeps its own lockfile reads only the catalogs of its own {WORKSPACE_MANIFEST_FILENAME}, and every other setting from the {WORKSPACE_MANIFEST_FILENAME} at the workspace root. \
         Move the settings there, or add \"!{relative_dir}\" to the root's \"packages\" to keep that project a separate workspace.",
    )
}
