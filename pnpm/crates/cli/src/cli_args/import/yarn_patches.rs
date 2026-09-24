use crate::{State, cli_args::recursive::discover_workspace_projects};
use indexmap::IndexMap;
use miette::{Context, IntoDiagnostic};
use node_semver::Range;
use percent_encoding::percent_decode_str;
use pnpm_config::Config;
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
    /// The patch file path as Yarn recorded it. A `~/` prefix is relative
    /// to the Yarn project root, any other path to the declaring project.
    pub patch_path: String,
}

impl YarnPatchSpecifier {
    /// `None` when `specifier` does not use the `patch:` protocol.
    pub(super) fn parse(specifier: &str) -> Option<Self> {
        let (source, patch_path) = specifier.strip_prefix("patch:")?.split_once('#')?;
        let patch_path = patch_path.split_once("::").map_or(patch_path, |(path, _)| path);
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
        Some(YarnPatchSpecifier { specifier, patch_key, patch_path: patch_path.to_string() })
    }

    fn patch_file_path(&self, yarn_root: &Path, project_dir: &Path) -> PathBuf {
        match self.patch_path.strip_prefix("~/") {
            Some(path) => yarn_root.join(path),
            None => project_dir.join(&self.patch_path),
        }
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

/// The result of [`convert_yarn_patches`].
#[derive(Debug, Default)]
pub(super) struct ConvertedYarnPatches {
    /// Whether any manifest was rewritten.
    pub manifests_changed: bool,
    /// Patch files that do not exist, keyed by the dependency declaring them.
    pub missing_patch_files: Vec<(String, PathBuf)>,
}

/// Replace every `patch:` specifier in `manifests` with the specifier of
/// the package it patches, saving each changed manifest, and add each
/// patch file that exists to `patched_dependencies` as a path relative to
/// `workspace_dir`. An entry already in `patched_dependencies` is kept.
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
            let patch_file = patch.patch_file_path(yarn_root, &project_dir);
            if !patch_file.is_file() {
                converted.missing_patch_files.push((alias, patch_file));
                continue;
            }
            patched_dependencies
                .entry(patch.patch_key)
                .or_insert_with(|| workspace_relative_path(&patch_file, workspace_dir));
        }
    }
    Ok(converted)
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

/// Convert the `patch:` specifiers of the imported projects and return the
/// state to import with: `state` itself when no manifest used the
/// protocol, otherwise one reloaded from the rewritten manifests with the
/// converted patches in its `patchedDependencies`.
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
    for (alias, patch_file) in &converted.missing_patch_files {
        pnpm_reporter::emit_global_warning::<Reporter>(&format!(
            r#"The patch file {} of "{alias}" does not exist. "{alias}" was imported without the patch."#,
            patch_file.display(),
        ));
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
