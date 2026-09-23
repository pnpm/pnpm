use super::link::DEPENDENCY_FIELDS;
use clap::Args;
use miette::Context;
use pnpm_config::Config;
use pnpm_fs::lexical_normalize;
use pnpm_package_manifest::{PackageManifest, PackageManifestError};
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
    /// project's `package.json`. Returns whether the caller should reinstall.
    ///
    /// Mirrors pnpm: when no overrides are configured it prints "Nothing to
    /// unlink" and returns `false` so the caller stops; otherwise it removes
    /// the `link:` overrides — the ones named, or all of them — and returns
    /// `true` so the caller reinstalls, even when nothing matched.
    pub(crate) fn remove_links(
        &self,
        config: &mut Config,
        manifest_path: &Path,
    ) -> miette::Result<bool> {
        let Some(overrides) = config.overrides.as_mut() else {
            println!("Nothing to unlink");
            return Ok(false);
        };

        let removed: Vec<(String, String)> = overrides
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

        for (selector, _) in &removed {
            overrides.shift_remove(selector);
        }

        if !removed.is_empty() {
            let manifest_dir = manifest_path
                .parent()
                .ok_or_else(|| miette::miette!("manifest path has no parent directory"))?;
            let root_dir = config.workspace_dir.as_deref().unwrap_or(manifest_dir);

            let selectors: Vec<String> = removed
                .iter()
                .map(|(selector, _)| selector.clone())
                .collect();
            remove_overrides(root_dir, &selectors)
                .wrap_err("removing link: overrides from pnpm-workspace.yaml")?;

            let linked_dirs: Vec<(&str, PathBuf)> = removed
                .iter()
                .map(|(name, specifier)| (name.as_str(), link_target_dir(root_dir, specifier)))
                .collect();
            remove_linked_dependencies(manifest_path, &linked_dirs)?;
        }

        Ok(true)
    }
}

fn link_target_dir(base_dir: &Path, specifier: &str) -> PathBuf {
    let path = specifier.strip_prefix("link:").unwrap_or(specifier);
    lexical_normalize(&base_dir.join(path))
}

/// Drop the dependencies `pnpm link` added to `package.json` for the removed
/// overrides. Only a `link:` dependency that points at the same directory as
/// its override is dropped, so a `link:` dependency declared to another
/// directory is kept.
fn remove_linked_dependencies(
    manifest_path: &Path,
    linked_dirs: &[(&str, PathBuf)],
) -> miette::Result<()> {
    let mut manifest = match PackageManifest::from_path(manifest_path.to_path_buf()) {
        Ok(manifest) => manifest,
        Err(PackageManifestError::NoImporterManifestFound(_)) => return Ok(()),
        Err(error) => return Err(error).wrap_err("reading the project package.json"),
    };
    let manifest_dir = manifest_path
        .parent()
        .ok_or_else(|| miette::miette!("manifest path has no parent directory"))?;

    let mut changed = false;
    for field in DEPENDENCY_FIELDS {
        let Some(deps) = manifest
            .value_mut()
            .get_mut(field)
            .and_then(Value::as_object_mut)
        else {
            continue;
        };
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
    }

    if changed {
        manifest.save().wrap_err("saving package.json without the unlinked dependencies")?;
    }
    Ok(())
}
