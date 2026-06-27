//! `pacquet self-update` — update pnpm to the latest version (or a given one).
//!
//! Ports pnpm's
//! [`self-update` command](https://github.com/pnpm/pnpm/blob/a33eeec9cd/pnpm11/engine/pm/commands/src/self-updater/selfUpdate.ts).
//!
//! The target version is resolved from the trusted package-manager
//! bootstrap registry. When the project pins pnpm via
//! `packageManager` / `devEngines.packageManager`, that pin is updated in
//! place; otherwise the engine is installed into the global packages
//! directory, its native binary linked, its registry signature verified,
//! and its bins linked into the global bin directory.

// `pub(crate)` so `pnpm with` can reuse the engine installer and the
// engine-identity verifier; both commands install the same pnpm engine.
pub(crate) mod install_pnpm;
pub(crate) mod verify_engine;

use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pacquet_cmd_shim::{Host as CmdShimHost, link_bins_of_packages_with_excludes};
use pacquet_config::{Config, PACQUET_VERSION};
use pacquet_fs::force_symlink_dir;
use pacquet_global::{create_global_cache_key, get_hash_link, read_installed_packages};
use pacquet_lockfile::EnvLockfile;
use pacquet_package_manifest::PackageManifest;
use pacquet_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pacquet_resolving_npm_resolver::which_version_is_pinned;
use serde_json::Value;
use std::{collections::HashSet, path::Path};

use crate::config_deps;

/// Migration guidance printed once when `self-update` crosses a major
/// boundary. Mirrors pnpm's `MAJOR_UPGRADE_HINTS`; add an entry per
/// future major that ships breaking changes users need to act on.
fn major_upgrade_hint(target_major: u64) -> Option<&'static str> {
    match target_major {
        11 => Some(
            "pnpm v11 removed or renamed several v10 settings. \
             See https://pnpm.io/11.x/migration for migration instructions.",
        ),
        _ => None,
    }
}

/// Errors specific to `self-update`. Codes mirror pnpm's `PnpmError`
/// codes (pnpm prefixes `ERR_PNPM_`, so a code already starting with
/// `PNPM_` becomes `ERR_PNPM_PNPM_...`).
#[derive(Debug, Display, Error, Diagnostic)]
pub(crate) enum SelfUpdateError {
    #[display("You should update pnpm with corepack")]
    #[diagnostic(code(ERR_PNPM_CANT_SELF_UPDATE_IN_COREPACK))]
    CantSelfUpdateInCorepack,

    #[display(r#"Cannot find "{specifier}" version of pnpm"#)]
    #[diagnostic(code(ERR_PNPM_CANNOT_RESOLVE_PNPM))]
    CannotResolvePnpm { specifier: String },

    #[display(
        "Refusing to switch to pnpm v{version}: it violates the configured minimumReleaseAge / trustPolicy"
    )]
    #[diagnostic(code(ERR_PNPM_PNPM_RELEASE_POLICY_VIOLATION))]
    ReleasePolicyViolation { version: String },

    #[display("{message}")]
    #[diagnostic(code(ERR_PNPM_PNPM_ENGINE_IDENTITY_UNVERIFIABLE))]
    EngineIdentityUnverifiable { message: String },

    #[display("{message}")]
    #[diagnostic(code(ERR_PNPM_PNPM_ENGINE_IDENTITY_MISMATCH))]
    EngineIdentityMismatch { message: String },

    #[display("Unable to find the global bin directory")]
    #[diagnostic(
        code(ERR_PNPM_NO_GLOBAL_BIN_DIR),
        help(
            r#"Run "pnpm setup" to create it automatically, or set the global-bin-dir setting, or the PNPM_HOME env variable. The global bin directory should be in the PATH."#
        )
    )]
    NoGlobalDir,
}

#[derive(Debug, Args)]
pub struct SelfUpdateArgs {
    /// The version, range, or dist-tag to update to. Defaults to the
    /// `latest` dist-tag (which refuses to downgrade).
    pub version: Option<String>,
}

/// Refuse to self-update under corepack (which manages its own updates).
/// Checked in the dispatcher *before* project config is loaded, so a broken
/// `.npmrc` / workspace config can't mask the corepack refusal. Mirrors
/// pnpm's `isExecutedByCorepack` guard.
pub(crate) fn reject_if_corepack() -> miette::Result<()> {
    if is_executed_by_corepack() {
        return Err(SelfUpdateError::CantSelfUpdateInCorepack.into());
    }
    Ok(())
}

impl SelfUpdateArgs {
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        config: &'static Config,
        dir: &Path,
    ) -> miette::Result<()> {
        if let Some(message) =
            Box::pin(handler::<Reporter>(self.version.as_deref(), config, dir)).await?
        {
            println!("{message}");
        }
        Ok(())
    }
}

/// The `self-update` flow. Returns the final user-facing message (printed
/// to stdout), or `None` when nothing needs printing.
async fn handler<Reporter: self::Reporter + 'static>(
    params: Option<&str>,
    config: &'static Config,
    dir: &Path,
) -> miette::Result<Option<String>> {
    let prefix = dir.to_string_lossy().into_owned();
    info::<Reporter>(&prefix, "Checking for updates...");

    // `self-update` (no args) defaults to the `latest` dist-tag but
    // refuses to downgrade; `self-update latest` (explicit) bypasses the
    // guard so a downgrade can still be forced.
    let is_implicit_latest = params.is_none();
    let bare_specifier = params.unwrap_or("latest");

    let resolved =
        Box::pin(config_deps::resolve_pnpm_version(config, bare_specifier)).await?.ok_or_else(
            || SelfUpdateError::CannotResolvePnpm { specifier: bare_specifier.to_string() },
        )?;
    let target_version = resolved.version;

    // Under strict resolution (`minimumReleaseAge` strict, or
    // `trustPolicy='no-downgrade'`), a policy violation must fail closed
    // rather than silently switch — mirrors pnpm's `makeResolutionStrict`.
    let strict_resolution = (config.resolved_minimum_release_age().is_some()
        && config.resolved_minimum_release_age_strict())
        || config.trust_policy == pacquet_config::TrustPolicy::NoDowngrade;
    if strict_resolution && resolved.policy_violation {
        return Err(SelfUpdateError::ReleasePolicyViolation { version: target_version }.into());
    }

    let manifest_value = super::package_manager::read_manifest_json(&dir.join("package.json"))?;
    let wanted = manifest_value.as_ref().and_then(super::package_manager::wanted_package_manager);

    // Migration hint when crossing a major boundary. The pinned version
    // (when the project pins pnpm) is the source of truth for "from";
    // otherwise the running binary is.
    let previous_version = match &wanted {
        // A range pin (`^10.0.0`) has no recoverable major on its own, so
        // read the resolved version from the env lockfile to drive the
        // hint — otherwise crossing a major would silently skip it.
        Some(pm) if pm.name == "pnpm" => {
            let lockfile_dir = config.workspace_dir.as_deref().unwrap_or(dir);
            read_project_pinned_pnpm_version(lockfile_dir, pm.version.as_deref())
                .filter(|version| version != &target_version)
        }
        _ if PACQUET_VERSION != target_version => Some(PACQUET_VERSION.to_string()),
        _ => None,
    };
    if let Some(previous) = &previous_version
        && let (Some(previous_major), Ok(target)) =
            (coerce_major(previous), node_semver::Version::parse(&target_version))
        && target.major > previous_major
        && let Some(hint) = major_upgrade_hint(target.major)
    {
        warn::<Reporter>(&prefix, hint);
    }

    // Project-pin branch: the project pins pnpm, so update the pin in
    // place instead of touching the global install.
    if let Some(pm) = &wanted
        && pm.name == "pnpm"
    {
        return Box::pin(update_project_pin(
            config,
            dir,
            pm,
            &target_version,
            bare_specifier,
            is_implicit_latest,
        ))
        .await;
    }

    // Global switch.
    if target_version == PACQUET_VERSION {
        return Ok(Some(format!(
            r#"The currently active pnpm v{PACQUET_VERSION} is already "{bare_specifier}" and doesn't need an update"#,
        )));
    }
    if is_implicit_latest && version_lt(&target_version, PACQUET_VERSION) {
        return Ok(Some(format!(
            r#"The currently active pnpm v{PACQUET_VERSION} is newer than the "latest" version on the registry (v{target_version}). No update performed. Run "pnpm self-update latest" to downgrade."#,
        )));
    }

    info::<Reporter>(
        &prefix,
        &format!("Switching pnpm from v{PACQUET_VERSION} to v{target_version}..."),
    );

    let env_root = config.global_pkg_dir.clone().ok_or(SelfUpdateError::NoGlobalDir)?;
    // Resolve integrities (pnpm + @pnpm/exe + platform binary) into the
    // env lockfile so the engine identity can be verified before install.
    Box::pin(config_deps::sync_package_manager_dependencies(
        config,
        &env_root,
        bare_specifier,
        &target_version,
        false,
    ))
    .await?;
    let env = EnvLockfile::read(&env_root)
        .map_err(miette::Report::new)
        .wrap_err("read the env lockfile")?
        .ok_or_else(|| SelfUpdateError::EngineIdentityUnverifiable {
            message: format!(
                "Cannot verify the identity of pnpm@{target_version}: its integrity metadata is missing from pnpm-lock.yaml.",
            ),
        })?;
    Box::pin(verify_engine::verify_pnpm_engine_identity(&env, &target_version, config)).await?;

    let result = Box::pin(install_pnpm::install_pnpm::<Reporter>(
        config,
        &target_version,
        config.supported_architectures.clone(),
    ))
    .await?;

    link_into_global_bin(config, &result.install_dir)?;

    if result.already_existed {
        return Ok(Some(format!(
            "The {bare_specifier} version, v{target_version}, is already present on the system. It was activated by linking it from {}.",
            result.install_dir.display(),
        )));
    }
    Ok(Some(format!("Successfully updated pnpm to v{target_version}")))
}

/// Update the project's `packageManager` / `devEngines.packageManager`
/// pin to `target_version`. Mirrors the project-pin branch of pnpm's
/// `self-update` handler.
async fn update_project_pin(
    config: &'static Config,
    dir: &Path,
    pm: &super::package_manager::WantedPackageManager,
    target_version: &str,
    bare_specifier: &str,
    is_implicit_latest: bool,
) -> miette::Result<Option<String>> {
    if pm.version.as_deref() == Some(target_version) {
        return Ok(Some(format!(
            "The current project is already set to use pnpm v{target_version}",
        )));
    }

    // Implicit `latest` must not downgrade a project pinned to a newer
    // version than the registry's `latest`. The env lockfile lives at the
    // workspace root, not necessarily the command's `--dir`.
    let lockfile_dir = config.workspace_dir.as_deref().unwrap_or(dir);
    if is_implicit_latest
        && let Some(current) = read_project_pinned_pnpm_version(lockfile_dir, pm.version.as_deref())
        && version_lt(target_version, &current)
    {
        return Ok(Some(format!(
            r#"The current project is set to use pnpm v{current}, which is newer than the "latest" version on the registry (v{target_version}). No update performed. Run "pnpm self-update latest" to downgrade."#,
        )));
    }

    let manifest_path = dir.join("package.json");
    let mut manifest = PackageManifest::from_path(manifest_path)
        .map_err(miette::Report::new)
        .wrap_err("read the project manifest")?;

    let has_dev_engines = manifest
        .value()
        .get("devEngines")
        .and_then(|dev_engines| dev_engines.get("packageManager"))
        .is_some();

    if has_dev_engines {
        let legacy_pins_pnpm = manifest
            .value()
            .get("packageManager")
            .and_then(Value::as_str)
            .map(super::package_manager::parse_package_manager)
            .is_some_and(|(name, version)| name == "pnpm" && version.is_some());

        let mut changed = false;
        if let Some(entry) = dev_engines_pnpm_entry_mut(manifest.value_mut()) {
            let current = entry.get("version").and_then(Value::as_str).map(ToString::to_string);
            let updated = if legacy_pins_pnpm {
                target_version.to_string()
            } else {
                update_version_constraint(current.as_deref(), target_version)
            };
            if current.as_deref() != Some(updated.as_str())
                && let Some(object) = entry.as_object_mut()
            {
                object.insert("version".to_string(), Value::String(updated));
                changed = true;
            }
        }
        if legacy_pins_pnpm {
            let new_legacy = format!("pnpm@{target_version}");
            if manifest.value().get("packageManager").and_then(Value::as_str) != Some(&new_legacy)
                && let Some(object) = manifest.value_mut().as_object_mut()
            {
                object.insert("packageManager".to_string(), Value::String(new_legacy));
                changed = true;
            }
        }
        if changed {
            manifest.save().map_err(miette::Report::new).wrap_err("write the project manifest")?;
        }
        if super::package_manager::should_persist_package_manager_lockfile(&pm_for_persist(pm)) {
            let root_dir = config.workspace_dir.clone().unwrap_or_else(|| dir.to_path_buf());
            Box::pin(config_deps::sync_package_manager_dependencies(
                config,
                &root_dir,
                bare_specifier,
                target_version,
                false,
            ))
            .await?;
        }
    } else if let Some(object) = manifest.value_mut().as_object_mut() {
        object
            .insert("packageManager".to_string(), Value::String(format!("pnpm@{target_version}")));
        manifest.save().map_err(miette::Report::new).wrap_err("write the project manifest")?;
    }

    Ok(Some(format!("The current project has been updated to use pnpm v{target_version}")))
}

/// The `pnpm` entry of `devEngines.packageManager` (which can be a single
/// object or an array), as a mutable reference.
fn dev_engines_pnpm_entry_mut(manifest: &mut Value) -> Option<&mut Value> {
    let package_manager = manifest.get_mut("devEngines")?.get_mut("packageManager")?;
    if package_manager.is_array() {
        return package_manager
            .as_array_mut()?
            .iter_mut()
            .find(|item| item.get("name").and_then(Value::as_str) == Some("pnpm"));
    }
    if package_manager.get("name").and_then(Value::as_str) == Some("pnpm") {
        return Some(package_manager);
    }
    None
}

/// A [`super::package_manager::WantedPackageManager`] flagged as `fromDevEngines` so
/// [`super::package_manager::should_persist_package_manager_lockfile`] decides persistence
/// the same way pnpm's `shouldPersistLockfile` does for a devEngines pin.
fn pm_for_persist(
    pm: &super::package_manager::WantedPackageManager,
) -> super::package_manager::WantedPackageManager {
    super::package_manager::WantedPackageManager {
        name: pm.name.clone(),
        version: pm.version.clone(),
        from_dev_engines: true,
        on_fail: pm.on_fail.clone(),
    }
}

/// Returns the updated `devEngines.packageManager` version constraint.
/// Mirrors pnpm's `updateVersionConstraint`: a constraint that still
/// satisfies the new version is left as-is (the lockfile pins the exact
/// version); otherwise the new version is written with the constraint's
/// pinning style, falling back to a caret range.
fn update_version_constraint(current: Option<&str>, new_version: &str) -> String {
    let Some(current) = current else {
        return new_version.to_string();
    };
    if range_satisfies(current, new_version) {
        return current.to_string();
    }
    match which_version_is_pinned(current) {
        Some(pinned) => format!("{}{new_version}", pinned.range_prefix()),
        None => format!("^{new_version}"),
    }
}

/// The project's currently-pinned pnpm version, used to guard implicit
/// `latest` against downgrading. Prefers the env lockfile's resolved
/// version (accurate for range pins); falls back to the spec's exact
/// version. Mirrors pnpm's `readProjectPinnedPnpmVersion`.
fn read_project_pinned_pnpm_version(lockfile_dir: &Path, spec: Option<&str>) -> Option<String> {
    let lockfile_pinned = EnvLockfile::read(lockfile_dir).ok().flatten().and_then(|env| {
        env.importers
            .get(EnvLockfile::ROOT_IMPORTER_KEY)
            .and_then(|importer| importer.package_manager_dependencies.as_ref())
            .and_then(|deps| deps.get("pnpm"))
            .map(|dep| dep.version.clone())
    });
    let spec_min = spec.and_then(super::package_manager::exact_version);
    match (lockfile_pinned, spec_min) {
        (Some(lockfile), Some(spec)) => {
            Some(if version_lt(&spec, &lockfile) { lockfile } else { spec })
        }
        (lockfile, spec) => lockfile.or(spec),
    }
}

/// Link the installed engine's bins into the global bin directory and
/// record its cache-keyed hash symlink (so `pnpm ls -g` and `store prune`
/// see it). Mirrors the linking pnpm's `self-update` does after install.
fn link_into_global_bin(config: &Config, install_dir: &Path) -> miette::Result<()> {
    let global_bin = config.global_bin.clone().ok_or(SelfUpdateError::NoGlobalDir)?;
    let global_pkg_dir = config.global_pkg_dir.clone().ok_or(SelfUpdateError::NoGlobalDir)?;

    let pkgs = read_installed_packages(install_dir);
    link_bins_of_packages_with_excludes::<CmdShimHost>(&pkgs, &global_bin, &HashSet::new())
        .map_err(miette::Report::new)
        .wrap_err("link the updated pnpm bins")?;

    let aliases = vec![install_pnpm::PNPM_PACKAGE_NAME.to_string()];
    let cache_hash = create_global_cache_key(&aliases, &registries_for_cache_key(config));
    let hash_link = get_hash_link(&global_pkg_dir, &cache_hash);
    force_symlink_dir(install_dir, &hash_link)
        .into_diagnostic()
        .wrap_err("link the global pnpm install directory")?;
    Ok(())
}

/// Build the registry map (`{ default, ...scoped }`) hashed into the
/// global cache key, from the trusted package-manager bootstrap registries
/// — never the repo-controlled project registries, so a project `.npmrc`
/// can't change the hash-symlink name (which would create duplicate global
/// `pnpm` groups that `find_global_package` resolves non-deterministically).
fn registries_for_cache_key(config: &Config) -> Vec<(String, String)> {
    let bootstrap = &config.package_manager_bootstrap;
    let mut registries = vec![("default".to_string(), bootstrap.registry.clone())];
    registries.extend(bootstrap.registries.iter().map(|(key, value)| (key.clone(), value.clone())));
    registries
}

/// `true` when pnpm is running under corepack, which manages its own
/// updates. Mirrors pnpm's `isExecutedByCorepack` (corepack sets
/// `COREPACK_ROOT`).
fn is_executed_by_corepack() -> bool {
    std::env::var_os("COREPACK_ROOT").is_some()
}

fn coerce_major(version: &str) -> Option<u64> {
    node_semver::Version::parse(version).ok().map(|version| version.major)
}

fn version_lt(left: &str, right: &str) -> bool {
    match (node_semver::Version::parse(left), node_semver::Version::parse(right)) {
        (Ok(left), Ok(right)) => left < right,
        _ => false,
    }
}

fn range_satisfies(range: &str, version: &str) -> bool {
    let (Ok(range), Ok(parsed)) =
        (node_semver::Range::parse(range), node_semver::Version::parse(version))
    else {
        return false;
    };
    if range.satisfies(&parsed) {
        return true;
    }
    // node-semver rejects a prerelease (`1.2.3-beta.1`) even when its base
    // version is in range; retry with the base so prerelease self-update
    // targets aren't spuriously treated as out of range.
    if parsed.pre_release.is_empty() {
        return false;
    }
    let base = format!("{}.{}.{}", parsed.major, parsed.minor, parsed.patch);
    matches!(node_semver::Version::parse(&base), Ok(base) if range.satisfies(&base))
}

#[cfg(test)]
mod tests;

fn info<Reporter: self::Reporter>(prefix: &str, message: &str) {
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Info,
        message: message.to_string(),
        prefix: prefix.to_string(),
    }));
}

fn warn<Reporter: self::Reporter>(prefix: &str, message: &str) {
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Warn,
        message: message.to_string(),
        prefix: prefix.to_string(),
    }));
}
