use crate::{State, cli_args::recursive::discover_workspace_projects};
use indexmap::{IndexMap, map::Entry};
use miette::{Context, IntoDiagnostic};
use node_semver::Range;
use percent_encoding::percent_decode_str;
use pnpm_config::Config;
use pnpm_fs::lexical_normalize;
use pnpm_package_manifest::PackageManifest;
use pnpm_reporter::Reporter;
use serde_json::Value;
use std::path::{Path, PathBuf};

const DEPENDENCY_FIELDS: [&str; 3] = ["dependencies", "devDependencies", "optionalDependencies"];

/// A dependency specifier written by Yarn's `patch:` protocol, such as
/// `patch:foo@npm%3A1.2.3#~/.yarn/patches/foo.patch`.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct YarnPatchSpecifier {
    /// The specifier of the package the patch applies to, in the form pnpm
    /// resolves.
    pub specifier: String,
    /// The `patchedDependencies` key selecting the patched package.
    pub patch_key: String,
    /// The patch file paths as Yarn recorded them, without Yarn's builtin
    /// compatibility patches. A `~/` prefix is relative to the Yarn project
    /// root, any other path to the declaring project.
    pub patch_paths: Vec<String>,
}

impl YarnPatchSpecifier {
    /// `None` when `specifier` does not use the `patch:` protocol.
    pub(super) fn parse(specifier: &str) -> Option<Self> {
        let (source, selector) = specifier.strip_prefix("patch:")?.split_once('#')?;
        let selector = selector.split_once("::").map_or(selector, |(selector, _)| selector);
        let patch_paths = selector
            .split('&')
            .filter(|path| !is_builtin_patch(path))
            .map(str::to_string)
            .collect();
        let source = percent_decode_str(source).decode_utf8_lossy();
        let (name, range) = split_descriptor(&source)?;
        let (specifier, patch_key) = match range.strip_prefix("npm:") {
            Some(npm_range) => match split_descriptor(npm_range) {
                Some((target, target_range)) => {
                    (range.to_string(), patch_key(target, target_range))
                }
                None => (npm_range.to_string(), patch_key(name, npm_range)),
            },
            None => (range.to_string(), patch_key(name, range)),
        };
        Some(YarnPatchSpecifier { specifier, patch_key, patch_paths })
    }
}

/// Yarn's builtin patches, such as `optional!builtin<compat/typescript>`,
/// adapt packages to Plug'n'Play and have no file.
fn is_builtin_patch(path: &str) -> bool {
    path.strip_prefix("optional!")
        .unwrap_or(path)
        .starts_with("builtin<")
}

fn patch_file_path(patch_path: &str, yarn_root: &Path, project_dir: &Path) -> PathBuf {
    match patch_path.strip_prefix("~/") {
        Some(path) => yarn_root.join(path),
        None => project_dir.join(patch_path),
    }
}

/// Split a `name@range` descriptor whose name may be scoped.
fn split_descriptor(descriptor: &str) -> Option<(&str, &str)> {
    let at = descriptor.get(1..)?.find('@')? + 1;
    Some((&descriptor[..at], &descriptor[at + 1..]))
}

fn patch_key(name: &str, range: &str) -> String {
    if Range::parse(range).is_ok() { format!("{name}@{range}") } else { name.to_string() }
}

/// A Yarn patch that import did not apply.
#[derive(Debug)]
pub(super) enum DroppedPatch {
    Missing { alias: String, patch_file: PathBuf },
    Several { alias: String },
    Conflicting { alias: String, patch_file: String, patch_key: String, kept: String },
}

impl DroppedPatch {
    fn warning(&self) -> String {
        match self {
            DroppedPatch::Missing { alias, patch_file } => format!(
                r#"The patch file {} of "{alias}" does not exist. "{alias}" was imported without the patch."#,
                patch_file.display(),
            ),
            DroppedPatch::Several { alias } => format!(
                r#""{alias}" has several Yarn patches, and pnpm applies one patch per dependency. "{alias}" was imported without the patches."#,
            ),
            DroppedPatch::Conflicting { alias, patch_file, patch_key, kept } => format!(
                r#"The Yarn patch {patch_file} of "{alias}" was not applied, because "{patch_key}" already uses the patch {kept}."#,
            ),
        }
    }
}

/// The result of [`convert_yarn_patches`].
#[derive(Debug, Default)]
pub(super) struct ConvertedYarnPatches {
    /// Whether any manifest was rewritten.
    pub manifests_changed: bool,
    pub dropped: Vec<DroppedPatch>,
}

/// An entry already in `patched_dependencies`, or recorded earlier, wins
/// over a converted patch with the same key.
pub(super) fn convert_yarn_patches(
    manifests: &mut [PackageManifest],
    yarn_root: &Path,
    workspace_dir: &Path,
    patched_dependencies: &mut IndexMap<String, String>,
) -> miette::Result<ConvertedYarnPatches> {
    let mut converted = ConvertedYarnPatches::default();
    for manifest in manifests {
        let project_dir = manifest
            .path()
            .parent()
            .unwrap_or(yarn_root)
            .to_path_buf();
        let patches = replace_patch_specifiers(manifest.value_mut());
        if patches.is_empty() {
            continue;
        }
        manifest
            .save()
            .into_diagnostic()
            .wrap_err_with(|| format!("saving {}", manifest.path().display()))?;
        converted.manifests_changed = true;
        for (alias, patch) in patches {
            let patch_file = single_patch_file(&alias, &patch.patch_paths, yarn_root, &project_dir);
            let patch_file = match patch_file {
                Ok(patch_file) => patch_file,
                Err(DroppedPatch::Missing { .. })
                    if patched_dependencies.contains_key(&patch.patch_key) =>
                {
                    continue;
                }
                Err(dropped) => {
                    converted.dropped.push(dropped);
                    continue;
                }
            };
            let Some(patch_file) = patch_file else { continue };
            let recorded = RecordedPatch { key: patch.patch_key, file: &patch_file, alias };
            converted.dropped.extend(recorded.record(patched_dependencies, workspace_dir));
        }
    }
    Ok(converted)
}

/// The one patch file pnpm can apply, or `None` when Yarn listed only
/// builtin patches.
fn single_patch_file(
    alias: &str,
    patch_paths: &[String],
    yarn_root: &Path,
    project_dir: &Path,
) -> Result<Option<PathBuf>, DroppedPatch> {
    let patch_path = match patch_paths {
        [] => return Ok(None),
        [patch_path] => patch_path,
        _ => return Err(DroppedPatch::Several { alias: alias.to_string() }),
    };
    let patch_file = patch_file_path(patch_path, yarn_root, project_dir);
    if !patch_file.is_file() {
        return Err(DroppedPatch::Missing { alias: alias.to_string(), patch_file });
    }
    Ok(Some(patch_file))
}

/// A converted patch about to be recorded in `patchedDependencies`.
pub(super) struct RecordedPatch<'a> {
    pub key: String,
    pub file: &'a Path,
    pub alias: String,
}

impl RecordedPatch<'_> {
    /// Record the patch unless its key already has one. A kept entry that
    /// names another file is reported as a conflict.
    pub(super) fn record(
        self,
        patched_dependencies: &mut IndexMap<String, String>,
        workspace_dir: &Path,
    ) -> Option<DroppedPatch> {
        let relative_file = workspace_relative_path(self.file, workspace_dir);
        match patched_dependencies.entry(self.key) {
            Entry::Vacant(entry) => {
                entry.insert(relative_file);
                None
            }
            Entry::Occupied(entry)
                if lexical_normalize(&workspace_dir.join(entry.get()))
                    == lexical_normalize(self.file) =>
            {
                None
            }
            Entry::Occupied(entry) => Some(DroppedPatch::Conflicting {
                alias: self.alias,
                patch_file: relative_file,
                patch_key: entry.key().clone(),
                kept: entry.get().clone(),
            }),
        }
    }
}

fn replace_patch_specifiers(manifest: &mut Value) -> Vec<(String, YarnPatchSpecifier)> {
    let mut patches = Vec::new();
    for field in DEPENDENCY_FIELDS {
        let Some(dependencies) = manifest.get_mut(field).and_then(Value::as_object_mut) else {
            continue;
        };
        for (alias, specifier) in dependencies.iter_mut() {
            let Some(patch) = specifier.as_str().and_then(YarnPatchSpecifier::parse) else {
                continue;
            };
            *specifier = Value::String(patch.specifier.clone());
            patches.push((alias.clone(), patch));
        }
    }
    patches
}

fn workspace_relative_path(path: &Path, workspace_dir: &Path) -> String {
    let relative = pathdiff::diff_paths(path, workspace_dir).unwrap_or_else(|| path.to_path_buf());
    relative.to_string_lossy().replace('\\', "/")
}

/// The state to import with. A manifest rewritten on disk needs a state
/// that reads it again.
pub(super) fn import_yarn_patches<Reporter: self::Reporter>(
    state: State,
    yarn_root: &Path,
) -> miette::Result<State> {
    let config = state.config;
    let workspace_dir = config.workspace_dir.clone().unwrap_or_else(|| yarn_root.to_path_buf());
    let mut manifests = match &config.workspace_dir {
        Some(workspace_dir) => discover_workspace_projects(workspace_dir, config)?
            .0
            .into_iter()
            .map(|project| project.manifest)
            .collect(),
        None => vec![state.manifest.clone()],
    };
    let mut patched_dependencies = config.patched_dependencies.clone().unwrap_or_default();
    let converted =
        convert_yarn_patches(&mut manifests, yarn_root, &workspace_dir, &mut patched_dependencies)?;
    for dropped in &converted.dropped {
        pnpm_reporter::emit_global_warning::<Reporter>(&dropped.warning());
    }
    if !converted.manifests_changed {
        return Ok(state);
    }
    reload_state(&state, config, workspace_dir, patched_dependencies)
}

fn reload_state(
    state: &State,
    config: &Config,
    workspace_dir: PathBuf,
    patched_dependencies: IndexMap<String, String>,
) -> miette::Result<State> {
    let mut config = config.clone();
    let recorded_count = config.patched_dependencies.as_ref().map_or(0, IndexMap::len);
    if patched_dependencies.len() > recorded_count {
        pnpm_workspace_manifest_writer::set_patched_dependencies(
            &workspace_dir,
            &patched_dependencies,
        )
        .wrap_err("recording the imported patches")?;
        config.workspace_dir.get_or_insert(workspace_dir);
        config.patched_dependencies = Some(patched_dependencies);
    }
    State::init(state.manifest.path().to_path_buf(), Config::leak(config), false)
        .wrap_err("initialize the state")
}

#[cfg(test)]
mod tests;
