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
use pnpm_cmd_shim::{Host as CmdShimHost, get_bins_from_package_manifest, is_safe_bin_name};
use pnpm_config::{Config, NamedShimPolicy, ShimPolicyValue};
use pnpm_crypto_hash::create_short_hash;
use pnpm_global::bin_slot_exists;
use pnpm_resolving_parse_wanted_dependency::is_valid_old_npm_package_name;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fmt::Write as _,
    fs,
    io::{self, Read as _},
    path::{Path, PathBuf},
};

use crate::{
    cli_args::global_bin_lock::acquire_global_bin_lock,
    config_deps,
    engine_pm::channel::PackageManager,
    shim_dispatch::{
        ShimTarget, install_native_shim, migrate_legacy_shims, native_shim_target, native_shims,
        remove_native_shim,
    },
};

const MAX_VIRTUAL_SHIM_METADATA_BYTES: u64 = 64 * 1024;
const VIRTUAL_SHIM_STATE_PREFIX: &str = ".pnpm-shim-v1-virtual-";

/// A global install replaces the target-less shim body, so this record
/// preserves whether `pnpm shim add` should be restored after uninstall.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VirtualShimState {
    package: String,
    bins: Vec<String>,
}

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

struct VirtualShimPublication<'a> {
    config: &'a Config,
    bin_dir: &'a Path,
    package: &'a str,
    bins: &'a [String],
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
    // Registry lookups happen before the lock, so no other global-bin
    // writer waits on the network.
    let mut bins_by_package = Vec::with_capacity(packages.len());
    for package in packages {
        bins_by_package.push((package, bins_of(config, package).await?));
    }
    let _global_bin_lock = acquire_global_bin_lock(bin_dir)?;
    // A legacy shim for the same package is only recognized as its own
    // once migrated, so migrate before the slot check.
    migrate_legacy_shims(bin_dir).into_diagnostic().wrap_err("migrate the global shims")?;
    let mut report = String::new();
    for (package, bins) in bins_by_package {
        // A bin already in the global bin directory belongs to something
        // else — a globally installed package, or another shim. Replacing
        // it would take a working command away, and `pnpm shim rm` would
        // then delete it rather than give it back.
        if let Some(bin) = bins.iter().find(|bin| taken_by_another(bin_dir, bin, package)) {
            return Err(
                ShimError::BinConflict { package: package.clone(), bin: bin.clone() }.into()
            );
        }
        publish_virtual_shims(&VirtualShimPublication { config, bin_dir, package, bins: &bins })?;
        writeln!(report, "Added {} for {package}", bins.join(", ")).unwrap();
    }
    Ok(report)
}

fn publish_virtual_shims(publication: &VirtualShimPublication<'_>) -> miette::Result<()> {
    let VirtualShimPublication { config, bin_dir, package, bins } = *publication;
    // Commit the governing records first. If publishing a shim later
    // fails, a retry can repair it without rollback racing a process
    // that replaced the public bin slot.
    set_policy(config, package, Some(ShimPolicyValue::Named(NamedShimPolicy::Auto)))?;
    record_virtual_shim_state(bin_dir, package, bins)?;
    for bin in bins {
        install_native_shim(bin_dir, bin, &ShimTarget::Virtual(package.to_string()))
            .into_diagnostic()
            .wrap_err_with(|| format!("link the {package} shims"))?;
    }
    Ok(())
}

/// Remove the shims for every package in `packages`, and with them the
/// opt-in that created them.
fn remove(config: &Config, bin_dir: &Path, packages: &[String]) -> miette::Result<String> {
    if packages.is_empty() {
        return Err(ShimError::NoPackage.into());
    }
    let _global_bin_lock = acquire_global_bin_lock(bin_dir)?;
    // A legacy shim is only listed once migrated.
    migrate_legacy_shims(bin_dir).into_diagnostic().wrap_err("migrate the global shims")?;
    let mut report = String::new();
    for package in packages {
        let bins = installed_shims(bin_dir, package);
        for bin in &bins {
            remove_native_shim(bin_dir, bin)
                .into_diagnostic()
                .wrap_err_with(|| format!("remove the {bin} shim"))?;
        }
        remove_virtual_shim_state(bin_dir, package)?;
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
        && virtual_shim_owner(&bin_dir.join(bin))
            .ok()
            .flatten()
            .is_none_or(|owner| owner != package)
}

/// The bins in `bin_dir` whose shim stands for `package`.
fn installed_shims(bin_dir: &Path, package: &str) -> Vec<String> {
    virtual_shims(bin_dir).filter(|(_, owner)| owner == package).map(|(bin, _)| bin).collect()
}

/// Every target-less shim in `bin_dir`, as `(bin name, package)`, in bin
/// name order.
///
/// Active shims are read from their recorded targets rather than
/// inferred from restoration state. The state only remembers an explicit
/// opt-in while a global package occupies the public bin slot.
fn virtual_shims(bin_dir: &Path) -> impl Iterator<Item = (String, String)> + use<'_> {
    native_shims(bin_dir).unwrap_or_default().into_iter().filter_map(|bin| {
        let package = virtual_shim_owner(&bin_dir.join(&bin)).ok().flatten()?;
        Some((bin, package))
    })
}

/// The package the target-less shim at `path` stands for; `None` for an
/// installed shim, a direct shim, or an empty slot.
pub(crate) fn virtual_shim_owner(path: &Path) -> io::Result<Option<String>> {
    let (Some(bin_dir), Some(name)) = (path.parent(), path.file_name().and_then(OsStr::to_str))
    else {
        return Ok(None);
    };
    Ok(native_shim_target(bin_dir, name)?
        .and_then(|target| target.virtual_package().map(str::to_string)))
}

pub(crate) fn virtual_shim_bins_to_restore(
    bin_dir: &Path,
    package: &str,
) -> miette::Result<Vec<String>> {
    let path = virtual_shim_state_path(bin_dir, package);
    let Some(state) = read_virtual_shim_state(&path)? else { return Ok(Vec::new()) };
    if state.package != package {
        let path_display = path.display();
        return Err(miette::miette!(
            "Virtual shim state at {} belongs to {}, not {package}",
            path_display,
            state.package,
        ));
    }
    Ok(state.bins)
}

pub(crate) fn virtual_shim_restoration_owners(
    bin_dir: &Path,
) -> miette::Result<BTreeMap<String, String>> {
    let entries = match fs::read_dir(bin_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("scan virtual shim state in {}", bin_dir.display()));
        }
    };
    let mut owners = BTreeMap::new();
    for entry in entries {
        let entry = entry
            .into_diagnostic()
            .wrap_err_with(|| format!("scan virtual shim state in {}", bin_dir.display()))?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if !file_name.starts_with(VIRTUAL_SHIM_STATE_PREFIX) || !file_name.ends_with(".json") {
            continue;
        }
        let path = entry.path();
        let Some(state) = read_virtual_shim_state(&path)? else { continue };
        if virtual_shim_state_path(bin_dir, &state.package) != path {
            let path_display = path.display();
            return Err(miette::miette!(
                "Virtual shim state at {} has an invalid package owner",
                path_display,
            ));
        }
        for bin in state.bins {
            if let Some(owner) = owners.get(&bin)
                && owner != &state.package
            {
                return Err(miette::miette!(
                    "Virtual shim state for {bin} is claimed by both {owner} and {}",
                    state.package,
                ));
            }
            owners.insert(bin, state.package.clone());
        }
    }
    Ok(owners)
}

fn read_virtual_shim_state(path: &Path) -> miette::Result<Option<VirtualShimState>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("read virtual shim state from {}", path.display()));
        }
    };
    let mut bytes = Vec::new();
    file.take(MAX_VIRTUAL_SHIM_METADATA_BYTES + 1)
        .read_to_end(&mut bytes)
        .into_diagnostic()
        .wrap_err_with(|| format!("read virtual shim state from {}", path.display()))?;
    if bytes.len() as u64 > MAX_VIRTUAL_SHIM_METADATA_BYTES {
        let path_display = path.display();
        return Err(miette::miette!("Virtual shim state at {path_display} is too large"));
    }
    let state: VirtualShimState = serde_json::from_slice(&bytes)
        .into_diagnostic()
        .wrap_err_with(|| format!("parse virtual shim state from {}", path.display()))?;
    if !is_valid_old_npm_package_name(&state.package) {
        let path_display = path.display();
        return Err(miette::miette!(
            "Virtual shim state at {path_display} has an invalid package owner",
        ));
    }
    if let Some(bin) = state.bins.iter().find(|bin| !is_safe_bin_name(bin)) {
        let path_display = path.display();
        return Err(miette::miette!(
            "Virtual shim state at {path_display} contains invalid bin name {bin:?}",
        ));
    }
    Ok(Some(state))
}

pub(crate) fn record_virtual_shim_state(
    bin_dir: &Path,
    package: &str,
    bins: &[String],
) -> miette::Result<()> {
    let path = virtual_shim_state_path(bin_dir, package);
    let state = VirtualShimState { package: package.to_string(), bins: bins.to_vec() };
    let bytes = serde_json::to_vec(&state).into_diagnostic().wrap_err("serialize virtual shims")?;
    pnpm_fs::write_atomic(&path, &bytes)
        .into_diagnostic()
        .wrap_err_with(|| format!("record virtual shims at {}", path.display()))
}

fn remove_virtual_shim_state(bin_dir: &Path, package: &str) -> miette::Result<()> {
    let path = virtual_shim_state_path(bin_dir, package);
    match fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("remove virtual shim state at {}", path.display()));
        }
    }
    Ok(())
}

fn virtual_shim_state_path(bin_dir: &Path, package: &str) -> PathBuf {
    let file_name = format!("{VIRTUAL_SHIM_STATE_PREFIX}{}.json", create_short_hash(package));
    bin_dir.join(file_name)
}

mod policy;

pub(crate) use policy::record_package_manager_shims;
use policy::{global_config_dir, set_policy, shims_disabled_globally, would_dispatch};

#[cfg(test)]
mod tests;
