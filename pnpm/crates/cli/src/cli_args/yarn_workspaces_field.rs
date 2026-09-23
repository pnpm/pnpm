//! The `workspaces` field of `package.json` is Yarn's and npm's way to
//! declare a monorepo's projects. pnpm declares them in
//! `pnpm-workspace.yaml` instead and never reads the manifest field, so a
//! repository converted from Yarn without that file would install as a
//! single project with no hint about why. The install family creates the
//! file from the field's patterns; where it cannot, a warning names the
//! field and the file that replaces it.

use super::config_warnings::emit_config_warning;
use miette::{Context, IntoDiagnostic};
use pnpm_config::{Config, WORKSPACE_MANIFEST_FILENAME};
use serde_json::Value;
use std::path::Path;

/// Warn when the root project manifest declares Yarn's `workspaces` field
/// outside a pnpm workspace. Inside one the field is redundant rather than
/// misleading: `pnpm-workspace.yaml` already selects the projects, so
/// `workspace_dir` being set silences the warning.
pub(crate) fn warn_unsupported_workspaces_field(
    manifest: Option<&Value>,
    workspace_dir: Option<&Path>,
) {
    if workspace_dir.is_some() || !declares_yarn_workspaces(manifest) {
        return;
    }
    emit_config_warning(
        "The \"workspaces\" field in package.json is not supported by pnpm. \
         Create a \"pnpm-workspace.yaml\" file instead.",
    );
}

/// Whether [`create_workspace_yaml_from_yarn_workspaces`] would convert
/// this root manifest, so an install must not skip it.
pub(crate) fn converts_yarn_workspaces(config: &Config, root_manifest: Option<&Value>) -> bool {
    config.workspace_dir.is_none()
        && !config.ignore_workspace
        && !yarn_workspace_patterns(root_manifest).is_empty()
}

/// Create `pnpm-workspace.yaml` in `config_root` from the root manifest's
/// `workspaces` patterns and anchor `cfg` to it, so the projects link on
/// this install.
///
/// An existing `pnpm-workspace.yaml` is never replaced, whether it was
/// there before the check or appeared while this one was being written;
/// `cfg` then follows that file. Without a usable pattern, or under
/// `--ignore-workspace`, the unsupported-field warning applies instead.
pub(crate) fn create_workspace_yaml_from_yarn_workspaces(
    cfg: &mut Config,
    config_root: &Path,
    root_manifest: Option<&Value>,
) -> miette::Result<()> {
    if cfg.workspace_dir.is_some() {
        return Ok(());
    }
    let patterns = yarn_workspace_patterns(root_manifest);
    if cfg.ignore_workspace || patterns.is_empty() {
        warn_unsupported_workspaces_field(root_manifest, None);
        return Ok(());
    }
    let path = config_root.join(WORKSPACE_MANIFEST_FILENAME);
    if workspace_manifest_exists(&path)? {
        return Ok(());
    }
    let text = render_workspace_manifest(&patterns)
        .into_diagnostic()
        .wrap_err_with(|| format!("render {}", path.display()))?;
    match publish_new_workspace_manifest(&path, &text) {
        Ok(()) => {
            emit_config_warning(
                r#"Created "pnpm-workspace.yaml" from the "workspaces" field in package.json."#,
            );
            cfg.anchor_to_created_workspace(config_root.to_path_buf(), patterns);
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            anchor_to_existing_workspace(cfg, config_root)
        }
        Err(error) => Err(error)
            .into_diagnostic()
            .wrap_err_with(|| format!("create {}", path.display())),
    }
}

/// Not `try_exists`: that follows a dangling symlink to report "absent",
/// and the write would then land on the link's target.
fn workspace_manifest_exists(path: &Path) -> miette::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error)
            .into_diagnostic()
            .wrap_err_with(|| format!("check for an existing {}", path.display())),
    }
}

fn render_workspace_manifest(
    patterns: &[String],
) -> Result<String, pnpm_workspace_manifest_writer::EditManifestFieldError> {
    let packages = Value::Array(
        patterns
            .iter()
            .cloned()
            .map(Value::String)
            .collect(),
    );
    match pnpm_workspace_manifest_writer::edit_manifest_field(None, "packages", &packages)? {
        pnpm_workspace_manifest_writer::ManifestEdit::Write(text) => Ok(text),
        edit => unreachable!("a new manifest with packages must be written, got {edit:?}"),
    }
}

/// Anchor `cfg` to the `pnpm-workspace.yaml` another writer published in
/// `config_root` while this one was being written.
fn anchor_to_existing_workspace(cfg: &mut Config, config_root: &Path) -> miette::Result<()> {
    let manifest = pnpm_workspace::read_workspace_manifest(config_root)
        .into_diagnostic()
        .wrap_err_with(|| {
            format!("read {}", config_root.join(WORKSPACE_MANIFEST_FILENAME).display())
        })?;
    if let Some(manifest) = manifest {
        cfg.anchor_to_created_workspace(
            config_root.to_path_buf(),
            pnpm_workspace::workspace_package_patterns(&manifest),
        );
    }
    Ok(())
}

/// Write `text` to `path` only if nothing is there, publishing it whole:
/// the text goes to a sibling temp file that a no-replace rename then
/// moves into place, so a concurrent reader sees no file or a complete
/// one. Fails with `AlreadyExists` when `path` exists, and a failure
/// removes only the temp file.
fn publish_new_workspace_manifest(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut builder = tempfile::Builder::new();
    builder.prefix(".pnpm-workspace-").suffix(".yaml-tmp");
    // The creation mode `File::create` uses, so the umask decides the
    // final mode as it does for any other new project file.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        builder.permissions(std::fs::Permissions::from_mode(0o666));
    }
    let mut tmp = builder.tempfile_in(dir)?;
    tmp.write_all(text.as_bytes())?;
    tmp.flush()?;
    tmp.persist_noclobber(path)
        .map(drop)
        .map_err(|persist| persist.error)
}

/// The usable patterns of an array-form `workspaces` field.
///
/// Only the array spelling counts, which is the spelling pnpm 11 warns
/// about. Yarn also accepts an object carrying `packages`, and neither
/// version handles it yet; the two are kept aligned here.
fn yarn_workspace_patterns(manifest: Option<&Value>) -> Vec<String> {
    workspaces_array(manifest)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|pattern| !pattern.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Whether the manifest declares a non-empty array-form `workspaces`
/// field, usable patterns or not.
fn declares_yarn_workspaces(manifest: Option<&Value>) -> bool {
    workspaces_array(manifest).is_some_and(|entries| !entries.is_empty())
}

fn workspaces_array(manifest: Option<&Value>) -> Option<&Vec<Value>> {
    manifest
        .and_then(|manifest| manifest.get("workspaces"))
        .and_then(Value::as_array)
}

#[cfg(test)]
mod tests;
