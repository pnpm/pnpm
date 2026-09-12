use super::{
    Config, Context, EnvLockfile, PackageManifest, Path, Value, infer_range_spec_style,
    range_satisfies, version_lt,
};
use crate::config_deps;

/// Update the project's `packageManager` / `devEngines.packageManager`
/// pin to `target_version`.
pub(super) async fn update_project_pin(
    config: &'static Config,
    dir: &Path,
    pm: &super::super::package_manager::WantedPackageManager,
    target_version: &str,
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
        update_dev_engines_pin(config, dir, pm, &mut manifest, target_version).await?;
    } else if let Some(object) = manifest.value_mut().as_object_mut() {
        object
            .insert("packageManager".to_string(), Value::String(format!("pnpm@{target_version}")));
        manifest.save().map_err(miette::Report::new).wrap_err("write the project manifest")?;
    }

    Ok(Some(format!("The current project has been updated to use pnpm v{target_version}")))
}

/// The `pnpm` entry of `devEngines.packageManager` (which can be a single
/// object or an array), as a mutable reference.
/// Point the manifest's `devEngines.packageManager` pin at
/// `target_version`, carrying the legacy `packageManager` field along when
/// it pins pnpm too. Returns the specifier the pin now carries.
fn write_dev_engines_pin(
    manifest: &mut PackageManifest,
    target_version: &str,
) -> miette::Result<String> {
    let legacy_pin = manifest
        .value()
        .get("packageManager")
        .and_then(Value::as_str)
        .map(super::super::package_manager::parse_package_manager);
    let legacy_pins_pnpm =
        legacy_pin.is_some_and(|(name, version)| name == "pnpm" && version.is_some());

    let mut changed = false;
    // Falls back to the resolved version when devEngines has no pnpm entry
    // to update; `package_manager_pin_specifier` supplies it otherwise.
    let mut pin_specifier = target_version.to_string();
    if let Some(entry) = dev_engines_pnpm_entry_mut(manifest.value_mut()) {
        let current = entry.get("version").and_then(Value::as_str);
        let updated = package_manager_pin_specifier(legacy_pins_pnpm, current, target_version);
        changed |= insert_string_if_changed(entry, "version", &updated);
        pin_specifier = updated;
    }
    if legacy_pins_pnpm {
        let new_legacy = format!("pnpm@{target_version}");
        changed |= insert_string_if_changed(manifest.value_mut(), "packageManager", &new_legacy);
    }
    if changed {
        manifest.save().map_err(miette::Report::new).wrap_err("write the project manifest")?;
    }
    Ok(pin_specifier)
}

/// Set `key` to `value`, reporting whether that changed the object.
fn insert_string_if_changed(object: &mut Value, key: &str, value: &str) -> bool {
    let Some(object) = object.as_object_mut() else {
        return false;
    };
    if object.get(key).and_then(Value::as_str) == Some(value) {
        return false;
    }
    object.insert(key.to_string(), Value::String(value.to_string()));
    true
}

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

/// A [`super::super::package_manager::WantedPackageManager`] flagged as `fromDevEngines` so
/// [`super::super::package_manager::should_persist_package_manager_lockfile`] decides persistence
/// the way it does for a devEngines pin.
fn pm_for_persist(
    pm: &super::super::package_manager::WantedPackageManager,
) -> super::super::package_manager::WantedPackageManager {
    super::super::package_manager::WantedPackageManager {
        name: pm.name.clone(),
        version: pm.version.clone(),
        from_dev_engines: true,
        on_fail: pm.on_fail.clone(),
    }
}

/// The specifier to record in `packageManagerDependencies` after
/// `self-update` rewrites the `devEngines.packageManager` pin. It must equal
/// the constraint a later install reads back from the manifest (see
/// [`super::super::package_manager::package_manager_to_sync`]) — the updated
/// devEngines constraint, never the CLI dist-tag or range passed to
/// `self-update` — so a subsequent `--frozen-lockfile` install does not
/// reject the lockfile as outdated. A legacy `packageManager` pin is always
/// exact, so it takes the resolved version directly.
pub(super) fn package_manager_pin_specifier(
    legacy_pins_pnpm: bool,
    current_dev_engine_version: Option<&str>,
    target_version: &str,
) -> String {
    if legacy_pins_pnpm {
        target_version.to_string()
    } else {
        update_version_constraint(current_dev_engine_version, target_version)
    }
}

/// Returns the updated `devEngines.packageManager` version constraint.
/// Exact pins and simple ranges (`^`, `~`) are rewritten to the new version
/// while keeping the operator, matching `pnpm update` / `pnpm runtime set`.
/// Complex ranges that still satisfy the new version are left as-is (the
/// lockfile pins the exact version); otherwise the new version is written
/// as a caret range.
pub(super) fn update_version_constraint(current: Option<&str>, new_version: &str) -> String {
    let Some(current) = current else {
        return new_version.to_string();
    };
    match infer_range_spec_style(current) {
        Some(pinned) => format!("{}{new_version}", pinned.range_prefix()),
        None if range_satisfies(current, new_version) => current.to_string(),
        None => format!("^{new_version}"),
    }
}

/// The project's currently-pinned pnpm version, used to guard implicit
/// `latest` against downgrading. Prefers the env lockfile's resolved
/// version (accurate for range pins); falls back to the spec's exact
/// version.
pub(super) fn read_project_pinned_pnpm_version(
    lockfile_dir: &Path,
    spec: Option<&str>,
) -> Option<String> {
    let lockfile_pinned = EnvLockfile::read(lockfile_dir).ok().flatten().and_then(|env| {
        env.importers
            .get(EnvLockfile::ROOT_IMPORTER_KEY)
            .and_then(|importer| importer.package_manager_dependencies.as_ref())
            .and_then(|deps| deps.get("pnpm"))
            .map(|dep| dep.version.clone())
    });
    let spec_min = spec.and_then(super::super::package_manager::exact_version);
    match (lockfile_pinned, spec_min) {
        (Some(lockfile), Some(spec)) => {
            Some(if version_lt(&spec, &lockfile) { lockfile } else { spec })
        }
        (lockfile, spec) => lockfile.or(spec),
    }
}

async fn update_dev_engines_pin(
    config: &'static Config,
    dir: &Path,
    pm: &super::super::package_manager::WantedPackageManager,
    manifest: &mut PackageManifest,
    target_version: &str,
) -> miette::Result<()> {
    let pin_specifier = write_dev_engines_pin(manifest, target_version)?;
    if super::super::package_manager::should_persist_package_manager_lockfile(&pm_for_persist(pm)) {
        let root_dir = config.workspace_dir.clone().unwrap_or_else(|| dir.to_path_buf());
        Box::pin(config_deps::sync_package_manager_dependencies(
            config,
            &root_dir,
            &pin_specifier,
            target_version,
            false,
            false,
        ))
        .await?;
    }
    Ok(())
}
