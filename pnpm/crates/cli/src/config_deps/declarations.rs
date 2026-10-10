//! Add and update the config dependencies `pnpm-workspace.yaml` declares.

use super::resolve_and_install;
use miette::{Context, IntoDiagnostic, Result};
use pnpm_config::Config;
use pnpm_env_installer::ConfigDepUpdates;
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::Reporter;
use pnpm_resolving_resolver_base::UpdateBehavior;
use pnpm_workspace_state::ConfigDependency;
use std::{collections::BTreeMap, path::Path};

/// Add config dependencies: resolve + install them (merged with any
/// already-declared config deps), then write the specifiers they are saved
/// with into `pnpm-workspace.yaml`'s `configDependencies` block. Backs
/// `pacquet add --config`.
pub async fn add_config_dependencies<Reporter: self::Reporter>(
    config: &Config,
    root_dir: &Path,
    added: &BTreeMap<String, String>,
    range_spec_style: RangeSpecStyle,
) -> Result<()> {
    let declared = config.config_dependencies.clone().unwrap_or_default();
    let updates = ConfigDepUpdates {
        prev_specifiers: added
            .keys()
            .map(|name| (name.clone(), declared_specifier(&declared, name)))
            .collect(),
        behavior: UpdateBehavior::Off,
        range_spec_style,
    };
    let mut config_dependencies = declared;
    for (name, specifier) in added {
        config_dependencies.insert(
            name.clone(),
            ConfigDependency::VersionWithIntegrity(specifier.clone()),
        );
    }

    let saved =
        resolve_and_install::<Reporter>(config, &config_dependencies, root_dir, false, &updates)
            .await?;
    // An integrity-pinned specifier is recorded as written.
    let specifiers = added
        .iter()
        .map(|(name, specifier)| {
            (
                name.as_str(),
                saved
                    .get(name)
                    .unwrap_or(specifier)
                    .as_str(),
            )
        });
    record_config_dependencies(root_dir, specifiers)
}

/// Resolve the config dependencies in `updates` again and save the
/// specifiers they resolve to, in `pnpm-workspace.yaml` and in `config`.
/// Backs `pacquet update`.
pub async fn update_config_dependencies<Reporter: self::Reporter>(
    config: &mut Config,
    root_dir: &Path,
    updates: &ConfigDepUpdates,
) -> Result<()> {
    let Some(config_dependencies) = config.config_dependencies.clone() else {
        return Ok(());
    };
    if updates.prev_specifiers.is_empty() {
        return Ok(());
    }
    let saved =
        resolve_and_install::<Reporter>(config, &config_dependencies, root_dir, false, updates)
            .await?;
    // `root_dir` follows `lockfile-dir`; the declarations live beside the
    // workspace manifest.
    record_config_dependencies(
        config.workspace_dir.as_deref().unwrap_or(root_dir),
        saved
            .iter()
            .map(|(name, specifier)| (name.as_str(), specifier.as_str())),
    )?;
    let declared = config.config_dependencies.get_or_insert_default();
    for (name, specifier) in saved {
        declared.insert(name, ConfigDependency::VersionWithIntegrity(specifier));
    }
    Ok(())
}

/// The specifier `pnpm-workspace.yaml` declares for `name`, if it declares
/// one an update can start from.
pub(crate) fn declared_specifier(
    declared: &BTreeMap<String, ConfigDependency>,
    name: &str,
) -> Option<String> {
    match declared.get(name)? {
        ConfigDependency::VersionWithIntegrity(specifier) if !specifier.contains('+') => {
            Some(specifier.clone())
        }
        _ => None,
    }
}

fn record_config_dependencies<'a>(
    root_dir: &Path,
    specifiers: impl Iterator<Item = (&'a str, &'a str)>,
) -> Result<()> {
    pnpm_workspace_manifest_writer::set_config_dependencies(root_dir, specifiers)
        .into_diagnostic()
        .wrap_err("recording the config dependencies in pnpm-workspace.yaml")
}
