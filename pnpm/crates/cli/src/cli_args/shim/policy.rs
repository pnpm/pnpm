//! The `globalShims` record: what it says, and what writing to it must
//! never do.
//!
//! The record in the global `config.yaml` is one of three layers the
//! dispatcher reads — the pnpm home's `pnpm-workspace.yaml` and the
//! environment both outrank it — so a command that edits it has to ask
//! what the *resolved* setting would be, not what its own file says. Two
//! rules follow, and both are about not deciding something the user
//! didn't:
//!
//! - a shim is not recorded when a layer above would leave it switched
//!   off, because the record would outlive that layer;
//! - a removal never switches a shim *on*, which clearing the entry that
//!   holds back a built-in shim would do.

use miette::{Context, IntoDiagnostic};
use pnpm_config::{
    Config, GLOBAL_CONFIG_YAML_FILENAME, GlobalShims, GlobalShimsSetting, Host, NamedShimPolicy,
    ShimPolicy, ShimPolicyValue, WorkspaceSettings, default_config_dir,
};
use pnpm_workspace_manifest_writer::update_manifest_field;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use crate::{cli_args::shim::ShimError, engine_pm::channel::PackageManager};

/// Record (or clear) `package`'s entry in the global `config.yaml`'s
/// `globalShims` record, which is what the dispatcher consults at run
/// time. The shim files and the setting are written together: a shim the
/// setting disables would sit on `PATH` doing nothing but shadowing.
pub(super) fn set_policy(
    config: &Config,
    package: &str,
    policy: Option<ShimPolicyValue>,
) -> miette::Result<()> {
    let config_dir = global_config_dir(config)?;
    // A global off switch holds no entries to clear, and replacing it
    // with an empty record would turn the built-in shims back on.
    if policy.is_none() && shims_disabled_globally(&config_dir)? {
        return Ok(());
    }
    let mut entries = recorded_entries(&config_dir)?;
    match policy {
        Some(policy) => {
            entries.insert(package.to_string(), policy);
        }
        None => match entries.get(package) {
            // Clearing an entry that switches a *built-in* shim off would
            // switch it on: that entry is the only thing holding it back,
            // and `pnpm shim rm` does not enable things. For anything
            // else, an off entry is the user's to take back.
            Some(policy) if !policy.dispatches() && dispatches_by_default(package) => {
                return Ok(());
            }
            // Clearing an entry the record does not hold changes nothing,
            // and writing anyway would spell the built-in defaults out
            // into the user's configuration as though they had chosen
            // them.
            None => return Ok(()),
            Some(_) => {
                entries.remove(package);
            }
        },
    }
    let value = serde_json::to_value(GlobalShimsSetting::Entries(entries))
        .into_diagnostic()
        .wrap_err("serialize the globalShims setting")?;
    fs::create_dir_all(&config_dir)
        .into_diagnostic()
        .wrap_err_with(|| format!("create {}", config_dir.display()))?;
    update_manifest_field(&config_dir.join(GLOBAL_CONFIG_YAML_FILENAME), "globalShims", &value)
        .map_err(miette::Report::new)
        .wrap_err("record the globalShims setting")
}

/// Opt every package manager among `packages` into project-aware
/// dispatch, and report which ones that added an entry for.
///
/// A globally installed package manager should defer to the version a
/// project pins, the way a globally installed runtime already defers to
/// `devEngines.runtime` — the global install stays the fallback for
/// projects that pin nothing. Only a package the user has not decided
/// about is recorded: an entry of its own, or a `globalShims: false` that
/// turns every shim off, is a decision and stands.
pub(crate) fn record_package_manager_shims<'a>(
    config: &Config,
    packages: impl IntoIterator<Item = &'a str>,
) -> miette::Result<BTreeSet<String>> {
    let config_dir = global_config_dir(config)?;
    if shims_disabled_globally(&config_dir)? {
        return Ok(BTreeSet::new());
    }
    let recorded = recorded_entries(&config_dir)?;
    let mut added = BTreeSet::new();
    for package in packages {
        // A disable that outranks this record leaves the shim doing
        // nothing, and the entry would outlive the disable.
        if PackageManager::parse(package).is_none()
            || recorded.contains_key(package)
            || !would_dispatch(config, package)?
        {
            continue;
        }
        set_policy(config, package, Some(ShimPolicyValue::Named(NamedShimPolicy::Auto)))?;
        added.insert(package.to_string());
    }
    Ok(added)
}

pub(super) fn global_config_dir(config: &Config) -> miette::Result<PathBuf> {
    config
        .config_dir
        .clone()
        .or_else(default_config_dir::<Host>)
        .ok_or_else(|| ShimError::NoGlobalDir.into())
}

/// Whether a shim this command writes for `package` would ever dispatch.
///
/// The setting `pnpm shim` records is one layer of it: the pnpm home's
/// `pnpm-workspace.yaml` and `PNPM_CONFIG_GLOBAL_SHIMS` both outrank the
/// global `config.yaml` this writes into, and a disable in either would
/// leave the shim on `PATH` doing nothing but shadowing. The answer comes
/// from the dispatcher's own layering, applied over the record as it
/// would read after this command.
pub(super) fn would_dispatch(config: &Config, package: &str) -> miette::Result<bool> {
    let mut recorded = recorded_entries(&global_config_dir(config)?)?;
    recorded.insert(package.to_string(), ShimPolicyValue::Named(NamedShimPolicy::Auto));
    let mut shims = GlobalShims::default();
    shims.apply(&GlobalShimsSetting::Entries(recorded));
    crate::shim_dispatch::apply_settings_above_global_config(&mut shims)
        .map_err(|error| miette::miette!("{error}"))
        .wrap_err("read the globalShims setting")?;
    Ok(shims.policy(package) != ShimPolicy::Off)
}

/// Whether `package` would dispatch with nothing recorded for it — the
/// built-in defaults are the managed runtimes.
pub(super) fn dispatches_by_default(package: &str) -> bool {
    GlobalShims::default().policy(package) != ShimPolicy::Off
}

/// Whether the global `config.yaml` turns every context-aware shim off.
/// Recording an opt-in on the user's behalf would quietly undo that;
/// asking for a shim outright still narrows it to what was asked for.
pub(super) fn shims_disabled_globally(config_dir: &Path) -> miette::Result<bool> {
    let settings = WorkspaceSettings::load_global(config_dir)
        .map_err(miette::Report::new)
        .wrap_err("read the global config.yaml")?;
    Ok(matches!(
        settings.and_then(|settings| settings.global_shims),
        Some(GlobalShimsSetting::Toggle(false)),
    ))
}

/// The `globalShims` record as the global `config.yaml` holds it today.
/// A scalar shorthand (`globalShims: true`) carries no per-package
/// entries, so an edit starts from the built-in defaults instead.
pub(super) fn recorded_entries(
    config_dir: &Path,
) -> miette::Result<std::collections::HashMap<String, ShimPolicyValue>> {
    let settings = WorkspaceSettings::load_global(config_dir)
        .map_err(miette::Report::new)
        .wrap_err("read the global config.yaml")?;
    match settings.and_then(|settings| settings.global_shims) {
        Some(GlobalShimsSetting::Entries(entries)) => Ok(entries),
        Some(GlobalShimsSetting::Toggle(false)) => Ok(std::collections::HashMap::new()),
        Some(GlobalShimsSetting::Toggle(true)) | None => Ok(default_entries()),
    }
}

/// The built-in defaults as an explicit record, so writing one entry does
/// not silently drop the rest.
pub(super) fn default_entries() -> std::collections::HashMap<String, ShimPolicyValue> {
    GlobalShims::default()
        .entries()
        .map(|(name, _)| (name.to_string(), ShimPolicyValue::Named(NamedShimPolicy::Auto)))
        .collect()
}
