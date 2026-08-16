//! `pacquet shim add|rm|ls` — context-aware shims for packages that are
//! not installed globally.
//!
//! A shim pnpm links for a globally installed package defers to the
//! project's own copy when there is one. These shims have no global
//! install behind them at all: they exist only to let a project decide
//! what runs. That is what makes `yarn` work in a Yarn project on a
//! machine that has only pnpm — the shim resolves the project's
//! `packageManager` pin and provisions it.
//!
//! Creating one is deliberate. Nothing here happens as a side effect of
//! `pnpm setup` or an install: a shim shadows whatever the user's `PATH`
//! resolved before it, so it is added only when asked for.

use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_cmd_shim::{
    Host as CmdShimHost, get_bins_from_package_manifest, is_virtual_shim_for, link_virtual_shims,
    remove_bin,
};
use pnpm_config::{Config, NamedShimPolicy, ShimPolicyValue};
use pnpm_global::bin_slot_exists;
use std::{collections::BTreeMap, fmt::Write as _, fs, path::Path};

use crate::{config_deps, engine_pm::channel::PackageManager};

/// Errors specific to `pacquet shim`. The codes carry the shared
/// `ERR_PNPM_` prefix.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum ShimError {
    #[display("Please specify the subcommand")]
    #[diagnostic(
        code(ERR_PNPM_SHIM_NO_SUBCOMMAND),
        help("Usage: pnpm shim add|rm|ls [package...]")
    )]
    NoSubcommand,

    #[display("Unknown subcommand: {subcommand}")]
    #[diagnostic(
        code(ERR_PNPM_SHIM_UNKNOWN_SUBCOMMAND),
        help("Usage: pnpm shim add|rm|ls [package...]")
    )]
    UnknownSubcommand {
        #[error(not(source))]
        subcommand: String,
    },

    #[display("Please specify at least one package")]
    #[diagnostic(code(ERR_PNPM_SHIM_NO_PACKAGE))]
    NoPackage,

    #[display("Unable to find the global bin directory")]
    #[diagnostic(
        code(ERR_PNPM_NO_GLOBAL_BIN_DIR),
        help(
            r#"Run "pnpm setup" to create it automatically, or set the global-bin-dir setting, or the PNPM_HOME env variable."#
        )
    )]
    NoGlobalDir,

    #[display("Cannot create a shim: globalShims is set to false")]
    #[diagnostic(
        code(ERR_PNPM_SHIMS_DISABLED),
        help(
            r#"That setting turns every context-aware shim off, so the shim would sit on PATH doing nothing. Remove it from the global config.yaml, or set "globalShims: true", and add the shim again."#
        )
    )]
    ShimsDisabled,

    #[display("Cannot create a shim for {package}: {bin} is already in the global bin directory")]
    #[diagnostic(
        code(ERR_PNPM_SHIM_BIN_CONFLICT),
        help(
            r#"Another package already provides that command. Remove it with "pnpm remove -g <package>", or remove its shim with "pnpm shim rm <package>"."#
        )
    )]
    BinConflict {
        #[error(not(source))]
        package: String,
        bin: String,
    },

    #[display("Cannot find the bins of {package}")]
    #[diagnostic(
        code(ERR_PNPM_SHIM_NO_BINS),
        help("A shim can only be created for a package that publishes at least one bin.")
    )]
    NoBins {
        #[error(not(source))]
        package: String,
    },
}

#[derive(Debug, Args)]
pub struct ShimArgs {
    /// The subcommand (`add`, `rm`, `ls`) and the packages it applies to.
    pub params: Vec<String>,
}

impl ShimArgs {
    /// Returns what to print; the caller writes it.
    pub async fn run(self, config: &'static Config) -> miette::Result<String> {
        let (subcommand, packages) = self.params.split_first().ok_or(ShimError::NoSubcommand)?;
        let bin_dir = config.global_bin.clone().ok_or(ShimError::NoGlobalDir)?;
        match subcommand.as_str() {
            "add" => add(config, &bin_dir, packages).await,
            "rm" | "remove" | "uninstall" => remove(config, &bin_dir, packages),
            "ls" | "list" => Ok(list(config, &bin_dir)),
            other => Err(ShimError::UnknownSubcommand { subcommand: other.to_string() }.into()),
        }
    }
}

/// Link the shims for every package in `packages` and record the opt-in.
async fn add(
    config: &'static Config,
    bin_dir: &Path,
    packages: &[String],
) -> miette::Result<String> {
    if packages.is_empty() {
        return Err(ShimError::NoPackage.into());
    }
    // Writing the opt-in would replace that global off switch with a
    // record of its own, turning the shims it disables back on.
    if shims_disabled_globally(&global_config_dir(config)?)? {
        return Err(ShimError::ShimsDisabled.into());
    }
    for package in packages {
        if !would_dispatch(config, package)? {
            return Err(ShimError::ShimsDisabled.into());
        }
    }
    // The global bin directory is `pnpm setup`'s to create, and a shim can
    // be the first thing that ever goes in it.
    fs::create_dir_all(bin_dir)
        .into_diagnostic()
        .wrap_err_with(|| format!("create {}", bin_dir.display()))?;
    let mut report = String::new();
    for package in packages {
        let bins = bins_of(config, package).await?;
        // A bin already in the global bin directory belongs to something
        // else — a globally installed package, or another shim. Replacing
        // it would take a working command away, and `pnpm shim rm` would
        // then delete it rather than give it back.
        if let Some(bin) = bins.iter().find(|bin| taken_by_another(bin_dir, bin, package)) {
            return Err(
                ShimError::BinConflict { package: package.clone(), bin: bin.clone() }.into()
            );
        }
        let bin_refs: Vec<&str> = bins.iter().map(String::as_str).collect();
        // The shims are useless without the dispatcher they call into, and
        // a user who never ran a global install has none yet.
        crate::shim_dispatch::install_dispatcher(bin_dir)
            .into_diagnostic()
            .wrap_err("install the shim dispatcher")?;
        let repairing = !installed_shims(bin_dir, package).is_empty();
        let written = link_virtual_shims::<CmdShimHost>(package, &bin_refs, bin_dir)
            .map_err(miette::Report::new)
            .wrap_err_with(|| format!("link the {package} shims"))?;
        if let Err(error) =
            set_policy(config, package, Some(ShimPolicyValue::Named(NamedShimPolicy::Auto)))
        {
            // A shim the setting does not govern shadows the command
            // without dispatching, so an add that cannot record the
            // opt-in leaves nothing behind. Repairing an existing set is
            // different: those shims were already on `PATH`, governed by
            // the entry this failed to rewrite, and removing them would
            // break what was working.
            if !repairing {
                for path in written {
                    let _ = remove_bin(&path);
                }
            }
            return Err(error);
        }
        writeln!(report, "Added {} for {package}", bins.join(", ")).unwrap();
    }
    Ok(report)
}

/// Remove the shims for every package in `packages`, and with them the
/// opt-in that created them.
fn remove(config: &Config, bin_dir: &Path, packages: &[String]) -> miette::Result<String> {
    if packages.is_empty() {
        return Err(ShimError::NoPackage.into());
    }
    let mut report = String::new();
    for package in packages {
        let bins = installed_shims(bin_dir, package);
        for bin in &bins {
            remove_bin(&bin_dir.join(bin))
                .into_diagnostic()
                .wrap_err_with(|| format!("remove the {bin} shim"))?;
        }
        set_policy(config, package, None)?;
        if bins.is_empty() {
            writeln!(report, "No shims for {package}").unwrap();
        } else {
            writeln!(report, "Removed {} for {package}", bins.join(", ")).unwrap();
        }
    }
    Ok(report)
}

/// Report every target-less shim in the global bin directory, with the
/// policy that governs it.
fn list(config: &Config, bin_dir: &Path) -> String {
    let mut packages: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (bin, package) in virtual_shims(bin_dir) {
        packages.entry(package).or_default().push(bin);
    }
    if packages.is_empty() {
        return "No shims. Add one with \"pnpm shim add <package>\".\n".to_string();
    }
    let mut report = String::new();
    for (package, mut bins) in packages {
        bins.sort();
        let policy = config.global_shims.policy(&package);
        writeln!(report, "{package} ({}): {}", policy.as_str(), bins.join(", ")).unwrap();
    }
    report
}

/// The bins a shim for `package` should carry.
///
/// The package managers pnpm provisions are known up front, down to the
/// aliases a given release may not list (`yarnpkg`, `npx`), so they need
/// no lookup. Anything else is read from the package's own published
/// manifest.
async fn bins_of(config: &'static Config, package: &str) -> miette::Result<Vec<String>> {
    if let Some(pm) = PackageManager::parse(package) {
        return Ok(pm.bins().iter().map(|bin| (*bin).to_string()).collect());
    }
    let resolved = config_deps::resolve_engine_version(config, package, "latest").await?;
    let manifest = resolved
        .and_then(|resolved| resolved.manifest)
        .ok_or_else(|| ShimError::NoBins { package: package.to_string() })?;
    // The manifest came off the registry, so there is no package directory
    // to walk: only the `bin` field can answer, which is what every
    // package that publishes a command sets.
    let bins = get_bins_from_package_manifest::<CmdShimHost>(&manifest, Path::new(""));
    if bins.is_empty() {
        return Err(ShimError::NoBins { package: package.to_string() }.into());
    }
    Ok(bins.into_iter().map(|bin| bin.name).collect())
}

/// Whether `bin` is occupied in `bin_dir` by anything other than
/// `package`'s own shim, which [`add`] rewrites freely.
fn taken_by_another(bin_dir: &Path, bin: &str, package: &str) -> bool {
    bin_slot_exists(bin_dir, bin)
        && fs::read_to_string(bin_dir.join(bin))
            .ok()
            .and_then(|content| virtual_shim_package(&content))
            .is_none_or(|owner| owner != package)
}

/// The bins in `bin_dir` whose shim stands for `package`.
fn installed_shims(bin_dir: &Path, package: &str) -> Vec<String> {
    let mut bins: Vec<String> =
        virtual_shims(bin_dir).filter(|(_, owner)| owner == package).map(|(bin, _)| bin).collect();
    bins.sort();
    bins
}

/// Every target-less shim in `bin_dir`, as `(bin name, package)`.
///
/// The shims are read rather than tracked in a side file: a shim is the
/// record of its own existence, so nothing can drift out of sync with
/// what is actually on `PATH`.
fn virtual_shims(bin_dir: &Path) -> impl Iterator<Item = (String, String)> + use<'_> {
    fs::read_dir(bin_dir).into_iter().flatten().flatten().filter_map(|entry| {
        let path = entry.path();
        // The marker is what identifies a shim, not the absence of an
        // extension: a package can publish a bin named `tool.js`. The
        // Windows siblings carry no marker, so they are skipped by it.
        let bin = path.file_name()?.to_str()?.to_string();
        let content = fs::read_to_string(&path).ok()?;
        let package = virtual_shim_package(&content)?;
        Some((bin, package))
    })
}

/// The package a shim's body declares, when it is a target-less shim.
fn virtual_shim_package(content: &str) -> Option<String> {
    content
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix("# cmd-shim-target=pkg:"))
        .map(ToString::to_string)
        .filter(|package| is_virtual_shim_for(content, package))
}

mod policy;

pub(crate) use policy::record_package_manager_shims;
use policy::{global_config_dir, set_policy, shims_disabled_globally, would_dispatch};

#[cfg(test)]
mod tests;
