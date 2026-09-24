use crate::cli_args::pipelines::select_workspace_projects;
use clap::Args;
use indexmap::IndexMap;
use miette::Context;
use pnpm_config::Config;
use pnpm_fs::lexical_normalize;
use pnpm_package_manifest::{PackageManifest, PackageManifestError};
use pnpm_workspace::project_manifest_path;
use pnpm_workspace_manifest_writer::remove_overrides;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Remove the link created by `pnpm link` and reinstall the package as
/// declared in `package.json`.
///
/// With package names, only the matching links are removed; with no
/// arguments, every link is removed.
#[derive(Debug, Args)]
pub struct UnlinkArgs {
    pub package_names: Vec<String>,

    /// Disable pnpm hooks defined in `.pnpmfile.cjs`, including the
    /// pnpmfiles of config dependencies.
    #[clap(long = "ignore-pnpmfile")]
    pub ignore_pnpmfile: bool,
}

impl UnlinkArgs {
    pub(crate) fn apply_cli_config(&self, config: &mut Config) {
        config.ignore_pnpmfile = self.ignore_pnpmfile || config.ignore_pnpmfile;
    }

    /// Revert what `pnpm link` wrote: strip the matching `link:` overrides
    /// from `config` (in memory) and from `pnpm-workspace.yaml`, and drop the
    /// `link:` dependencies that point at the same directories from the
    /// manifests of the projects this run reinstalls. Returns whether the
    /// caller should reinstall.
    ///
    /// Mirrors pnpm: when no overrides are configured it prints "Nothing to
    /// unlink" and returns `false` so the caller stops; otherwise it removes
    /// the `link:` overrides — the ones named, or all of them — and returns
    /// `true` so the caller reinstalls, even when nothing matched.
    pub(crate) fn remove_links(
        &self,
        config: &mut Config,
        prefix: &Path,
        manifest_path: &Path,
        recursive_sort: bool,
    ) -> miette::Result<bool> {
        let Some(overrides) = config.overrides.as_mut() else {
            println!("Nothing to unlink");
            return Ok(false);
        };

        let removed = self.take_link_overrides(overrides);

        if !removed.is_empty() {
            let manifest_dir = manifest_path
                .parent()
                .ok_or_else(|| miette::miette!("manifest path has no parent directory"))?;
            let root_dir = config.workspace_dir.as_deref().unwrap_or(manifest_dir);

            let linked_dirs: Vec<(&str, PathBuf)> = removed
                .iter()
                .map(|(name, specifier)| (name.as_str(), link_target_dir(root_dir, specifier)))
                .collect();
            let unlinked_manifests: Vec<PackageManifest> =
                selected_manifest_paths(config, prefix, manifest_path, recursive_sort)?
                    .iter()
                    .map(|path| remove_linked_dependencies(path, &linked_dirs))
                    .filter_map(Result::transpose)
                    .collect::<miette::Result<_>>()?;

            let selectors: Vec<String> = removed.into_keys().collect();
            remove_overrides(root_dir, &selectors)
                .wrap_err("removing link: overrides from pnpm-workspace.yaml")?;
            for mut manifest in unlinked_manifests {
                manifest
                    .save()
                    .wrap_err("saving package.json without the unlinked dependencies")?;
            }
        }

        Ok(true)
    }

    /// Remove the `link:` overrides this run unlinks (the named ones, or all
    /// of them) from `overrides` and return them.
    fn take_link_overrides(
        &self,
        overrides: &mut IndexMap<String, String>,
    ) -> IndexMap<String, String> {
        let removed: IndexMap<String, String> = overrides
            .iter()
            .filter(|(selector, specifier)| {
                specifier.starts_with("link:")
                    && (self.package_names.is_empty()
                        || self.package_names
                            .iter()
                            .any(|name| name == *selector))
            })
            .map(|(selector, specifier)| (selector.clone(), specifier.clone()))
            .collect();
        for selector in removed.keys() {
            overrides.shift_remove(selector);
        }
        removed
    }
}

/// The manifests of the projects a `-r` / `--filter` run selects, or the
/// current project's manifest outside such a run.
fn selected_manifest_paths(
    config: &Config,
    prefix: &Path,
    manifest_path: &Path,
    recursive_sort: bool,
) -> miette::Result<Vec<PathBuf>> {
    let Some(selection) =
        select_workspace_projects(config, prefix, manifest_path, recursive_sort, false)?
    else {
        return Ok(vec![manifest_path.to_path_buf()]);
    };
    Ok(selection.selected_dirs
        .iter()
        .map(|dir| project_manifest_path(dir))
        .collect())
}

fn link_target_dir(base_dir: &Path, specifier: &str) -> PathBuf {
    let path = specifier.strip_prefix("link:").unwrap_or(specifier);
    lexical_normalize(&base_dir.join(path))
}

/// The manifest at `manifest_path` with the dependencies `pnpm link` added
/// for the removed overrides dropped, unsaved, or `None` when it has none.
/// `pnpm link` only writes `dependencies`, and only a `link:` dependency that
/// points at the same directory as its override is dropped, so a `link:`
/// dependency declared to another directory is kept.
fn remove_linked_dependencies(
    manifest_path: &Path,
    linked_dirs: &[(&str, PathBuf)],
) -> miette::Result<Option<PackageManifest>> {
    let mut manifest = match PackageManifest::from_path(manifest_path.to_path_buf()) {
        Ok(manifest) => manifest,
        Err(PackageManifestError::NoImporterManifestFound(_)) => return Ok(None),
        Err(error) => return Err(error).wrap_err("reading the project package.json"),
    };
    let manifest_dir = manifest_path
        .parent()
        .ok_or_else(|| miette::miette!("manifest path has no parent directory"))?;
    let Some(deps) = manifest
        .value_mut()
        .get_mut("dependencies")
        .and_then(Value::as_object_mut)
    else {
        return Ok(None);
    };

    let mut changed = false;
    for (name, linked_dir) in linked_dirs {
        let points_at_linked_dir = deps
            .get(*name)
            .and_then(Value::as_str)
            .is_some_and(|specifier| {
                specifier.starts_with("link:")
                    && link_target_dir(manifest_dir, specifier) == *linked_dir
            });
        if points_at_linked_dir {
            deps.remove(*name);
            changed = true;
        }
    }

    Ok(changed.then_some(manifest))
}
