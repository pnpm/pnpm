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

/// Warn about a `workspaces` field in the root project manifest that pnpm
/// does not follow. Outside a pnpm workspace the field is unsupported.
/// Inside one, `pnpm-workspace.yaml` selects the projects, so the field
/// only warrants a warning when its patterns differ from that file's.
pub(crate) fn warn_about_workspaces_field(config: &Config, manifest: Option<&Value>) {
    let Some(workspace_dir) = config.workspace_dir.as_deref() else {
        if declares_yarn_workspaces(manifest) {
            emit_config_warning(
                "The \"workspaces\" field in package.json is not supported by pnpm. \
                 Create a \"pnpm-workspace.yaml\" file instead.",
            );
        }
        return;
    };
    if workspaces_field_differs(config, workspace_dir, manifest) {
        emit_config_warning(
            "The \"workspaces\" field in package.json differs from \"packages\" in \
             pnpm-workspace.yaml. pnpm uses pnpm-workspace.yaml.",
        );
    }
}

/// An empty array declares nothing, so it never differs.
fn workspaces_field_differs(
    config: &Config,
    workspace_dir: &Path,
    manifest: Option<&Value>,
) -> bool {
    // With `lockfileDir` elsewhere, the root manifest is not the workspace
    // root's, so its field says nothing about `pnpm-workspace.yaml`.
    if config.lockfile_dir
        .as_deref()
        .is_some_and(|dir| dir != workspace_dir)
    {
        return false;
    }
    let packages = config.workspace_package_patterns.as_deref().unwrap_or_default();
    declares_yarn_workspaces(manifest)
        && !same_patterns(&yarn_workspace_patterns(manifest), packages)
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
/// `cfg` then follows that file. Inside a workspace, without a usable
/// pattern, or under `--ignore-workspace`, nothing is created and
/// [`warn_about_workspaces_field`] applies instead.
pub(crate) fn create_workspace_yaml_from_yarn_workspaces(
    cfg: &mut Config,
    config_root: &Path,
    root_manifest: Option<&Value>,
) -> miette::Result<()> {
    let patterns = yarn_workspace_patterns(root_manifest);
    if cfg.workspace_dir.is_some() || cfg.ignore_workspace || patterns.is_empty() {
        warn_about_workspaces_field(cfg, root_manifest);
        return Ok(());
    }
    let path = config_root.join(WORKSPACE_MANIFEST_FILENAME);
    if existing_workspace_manifest(&path)?.is_some() {
        return adopt_existing_workspace(cfg, config_root, root_manifest);
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
            adopt_existing_workspace(cfg, config_root, root_manifest)
        }
        Err(error) => Err(error)
            .into_diagnostic()
            .wrap_err_with(|| format!("create {}", path.display())),
    }
}

enum ExistingManifest {
    File,
    Other,
}

/// Not `try_exists`: that follows a dangling symlink to report "absent",
/// and the write would then land on the link's target.
fn existing_workspace_manifest(path: &Path) -> miette::Result<Option<ExistingManifest>> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(Some(ExistingManifest::File)),
        Ok(_) => Ok(Some(ExistingManifest::Other)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
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

/// Anchor `cfg` to a `pnpm-workspace.yaml` another writer published in
/// `config_root` after the config loaded. A symlink or any entry other
/// than a regular file is left alone and never followed.
///
/// Settings other than `packages` apply only while the config loads, so
/// a manifest declaring any is an error that asks for the command again.
fn adopt_existing_workspace(
    cfg: &mut Config,
    config_root: &Path,
    root_manifest: Option<&Value>,
) -> miette::Result<()> {
    let path = config_root.join(WORKSPACE_MANIFEST_FILENAME);
    if !matches!(existing_workspace_manifest(&path)?, Some(ExistingManifest::File)) {
        return Ok(());
    }
    let manifest = pnpm_workspace::read_workspace_manifest(config_root)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", path.display()))?;
    let Some(manifest) = manifest else {
        return Ok(());
    };
    if declares_settings(&path)? {
        let path = path.display();
        return Err(miette::miette!(
            code = "ERR_PNPM_WORKSPACE_MANIFEST_APPEARED",
            help = "Run the command again.",
            "{path} was created while this command was running, and its settings could not be applied",
        ));
    }
    cfg.anchor_to_created_workspace(
        config_root.to_path_buf(),
        pnpm_workspace::workspace_package_patterns(&manifest),
    );
    warn_about_workspaces_field(cfg, root_manifest);
    Ok(())
}

/// Whether the workspace manifest at `path` has any top-level key besides
/// `packages`.
fn declares_settings(path: &Path) -> miette::Result<bool> {
    let text = std::fs::read_to_string(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(false);
    }
    let document: Value = serde_saphyr::from_str(&text)
        .into_diagnostic()
        .wrap_err_with(|| format!("parse {}", path.display()))?;
    Ok(document
        .as_object()
        .is_some_and(|keys| keys.keys().any(|key| key != "packages")))
}

/// A concurrent reader sees no file at `path` or the complete `text`.
/// Fails with `AlreadyExists` when `path` exists, which is never replaced.
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
/// Only the array spelling counts; the object form carrying `packages` is
/// not read.
fn yarn_workspace_patterns(manifest: Option<&Value>) -> Vec<String> {
    workspaces_array(manifest)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|pattern| !pattern.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Ignores order and repeats.
fn same_patterns(left: &[String], right: &[String]) -> bool {
    let left: std::collections::BTreeSet<&String> = left.iter().collect();
    let right: std::collections::BTreeSet<&String> = right.iter().collect();
    left == right
}

/// Counts a non-empty array even when none of its entries is a usable
/// pattern.
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
