use clap::Args;
use miette::Context;
use pacquet_config::Config;
use pacquet_workspace_manifest_writer::remove_overrides;
use std::path::Path;

/// Remove the link created by `pnpm link` and reinstall the package as
/// declared in `package.json`.
///
/// With package names, only the matching links are removed; with no
/// arguments, every link is removed.
#[derive(Debug, Args)]
pub struct UnlinkArgs {
    pub package_names: Vec<String>,
}

impl UnlinkArgs {
    /// Strip the matching `link:` overrides from `config` (in memory) and
    /// from `pnpm-workspace.yaml`, returning whether the caller should
    /// reinstall.
    ///
    /// Mirrors pnpm: when no overrides are configured it prints "Nothing to
    /// unlink" and returns `false` so the caller stops; otherwise it removes
    /// the `link:` overrides — the ones named, or all of them — and returns
    /// `true` so the caller reinstalls, even when nothing matched.
    pub(crate) fn strip_link_overrides(
        &self,
        config: &mut Config,
        manifest_path: &Path,
    ) -> miette::Result<bool> {
        let Some(overrides) = config.overrides.as_mut() else {
            println!("Nothing to unlink");
            return Ok(false);
        };

        let removed: Vec<String> = overrides
            .iter()
            .filter(|(selector, specifier)| {
                specifier.starts_with("link:")
                    && (self.package_names.is_empty()
                        || self.package_names.iter().any(|name| name == *selector))
            })
            .map(|(selector, _)| selector.clone())
            .collect();

        for selector in &removed {
            overrides.shift_remove(selector);
        }

        if !removed.is_empty() {
            let root_dir = config
                .workspace_dir
                .clone()
                .or_else(|| manifest_path.parent().map(Path::to_path_buf))
                .ok_or_else(|| miette::miette!("manifest path has no parent directory"))?;

            remove_overrides(&root_dir, &removed)
                .wrap_err("removing link: overrides from pnpm-workspace.yaml")?;
        }

        Ok(true)
    }
}
