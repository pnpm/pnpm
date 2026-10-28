use std::path::{Path, PathBuf};

use miette::Context;
use node_semver::Version;
use pnpm_config::Config;
use pnpm_engine_runtime_node_resolver::normalize_node_runtime_version_specifier;
use pnpm_env_installer::pnpm_engine_packages;
use pnpm_lockfile::EnvLockfile;
use pnpm_lockfile_preferred_versions::get_version_selector_type;
use pnpm_package_manifest::PackageManifest;
use pnpm_registry::RangeSpecStyle;
use pnpm_resolving_npm_resolver::calc_version_range;
use pnpm_resolving_resolver_base::VersionSelectorType;
use pnpm_tool_protocol::RUNTIME_PROTOCOL;
use serde_json::Value;

use super::{DeclaredEngineDep, EngineDepKind, PmDeclarationLocation};
use crate::{
    cli_args::package_manager::{
        PinRoots, should_persist_package_manager_lockfile, version_satisfies,
        wanted_package_manager,
    },
    config_deps,
    engine_pm::channel::PackageManager,
};

pub(super) fn record_resolved_engine_update(
    dep: DeclaredEngineDep,
    resolved_version: Version,
    new_spec: String,
    no_save: bool,
    pnpm_sync_target: &mut Option<(String, String)>,
    manifest_updates: &mut Vec<(DeclaredEngineDep, Version, String)>,
) {
    if let EngineDepKind::PackageManager { pm: PackageManager::Pnpm, .. } = dep.kind {
        let effective_spec = if no_save { dep.raw_specifier.clone() } else { new_spec.clone() };
        *pnpm_sync_target = Some((effective_spec, resolved_version.to_string()));
    }
    if !no_save {
        manifest_updates.push((dep, resolved_version, new_spec));
    }
}

pub(super) async fn commit_manifest_engine_updates(
    config: &Config,
    manifest: &mut PackageManifest,
    manifest_updates: &[(DeclaredEngineDep, Version, String)],
    pnpm_sync_target: Option<(String, String)>,
) -> miette::Result<()> {
    if !manifest_updates.is_empty() {
        persist_manifest_engine_updates(manifest, manifest_updates)?;
    }
    if let Some((wanted_specifier, target_version)) = pnpm_sync_target {
        sync_updated_pnpm_lockfile(config, manifest, &wanted_specifier, &target_version).await?;
    }
    Ok(())
}

pub(super) fn compute_updated_engine_spec(
    dep: &DeclaredEngineDep,
    requested: Option<&str>,
    resolved_version: &Version,
    save_exact: bool,
    range_spec_style: RangeSpecStyle,
) -> String {
    if dep.name == "node" && matches!(dep.kind, EngineDepKind::Runtime { .. }) {
        return compute_node_runtime_spec(dep, requested, resolved_version, save_exact);
    }
    if let Some(req) = requested
        && Version::parse(req).is_err()
    {
        if get_version_selector_type(req) != Some(VersionSelectorType::Tag) {
            return req.to_string();
        }
        if save_exact {
            return resolved_version.to_string();
        }
        return calc_version_range(
            resolved_version,
            Some(&dep.raw_specifier),
            Some(req),
            range_spec_style,
        );
    }
    if save_exact {
        return resolved_version.to_string();
    }
    calc_version_range(resolved_version, Some(&dep.raw_specifier), None, range_spec_style)
}

fn compute_node_runtime_spec(
    dep: &DeclaredEngineDep,
    requested: Option<&str>,
    resolved_version: &Version,
    save_exact: bool,
) -> String {
    if save_exact {
        return resolved_version.to_string();
    }
    let selector = match requested {
        Some(req)
            if Version::parse(req).is_err()
                && get_version_selector_type(req) != Some(VersionSelectorType::Tag) =>
        {
            req
        }
        _ => dep.raw_specifier.as_str(),
    };
    let prev_runtime = format!("{RUNTIME_PROTOCOL}{selector}");
    normalize_node_runtime_version_specifier(
        selector,
        &resolved_version.to_string(),
        Some(&prev_runtime),
    )
}

pub(super) fn persist_manifest_engine_updates(
    manifest: &mut PackageManifest,
    updates: &[(DeclaredEngineDep, Version, String)],
) -> miette::Result<()> {
    let mut changed_in_memory = false;
    for (dep, resolved_version, new_spec) in updates {
        changed_in_memory |=
            apply_engine_dep_update_to_value(manifest.value_mut(), dep, resolved_version, new_spec);
    }
    if !changed_in_memory {
        return Ok(());
    }
    if manifest.path().exists() {
        let mut disk_manifest = PackageManifest::from_path(manifest.path().to_path_buf())
            .map_err(miette::Report::new)
            .wrap_err("reading project manifest to update engine versions")?;
        let mut changed_on_disk = false;
        for (dep, resolved_version, new_spec) in updates {
            changed_on_disk |= apply_engine_dep_update_to_value(
                disk_manifest.value_mut(),
                dep,
                resolved_version,
                new_spec,
            );
        }
        if changed_on_disk {
            disk_manifest
                .save()
                .map_err(miette::Report::new)
                .wrap_err("writing updated engine versions to project manifest")?;
        }
        return Ok(());
    }
    manifest
        .save()
        .map_err(miette::Report::new)
        .wrap_err("writing updated engine versions to project manifest")
}

fn apply_engine_dep_update_to_value(
    manifest: &mut Value,
    dep: &DeclaredEngineDep,
    resolved_version: &Version,
    new_spec: &str,
) -> bool {
    let mut changed = false;
    match dep.kind {
        EngineDepKind::PackageManager { pm, location } => {
            if matches!(
                location,
                PmDeclarationLocation::DevEnginesOnly | PmDeclarationLocation::Both,
            ) && let Some(entry) =
                find_engine_entry_mut(manifest, "devEngines", "packageManager", pm.name())
            {
                changed |= set_string_field(entry, "version", new_spec);
            }
            if matches!(
                location,
                PmDeclarationLocation::PackageManagerFieldOnly | PmDeclarationLocation::Both,
            ) {
                let exact_pm = format!("{}@{resolved_version}", pm.name());
                changed |= set_string_field(manifest, "packageManager", &exact_pm);
            }
        }
        EngineDepKind::Runtime { engines_field } => {
            if let Some(entry) =
                find_engine_entry_mut(manifest, engines_field, "runtime", &dep.name)
            {
                changed |= set_string_field(entry, "version", new_spec);
            }
        }
    }
    changed
}

fn find_engine_entry_mut<'a>(
    manifest: &'a mut Value,
    container_field: &str,
    sub_field: &str,
    target_name: &str,
) -> Option<&'a mut Value> {
    let field = manifest.get_mut(container_field)?.get_mut(sub_field)?;
    if field.is_array() {
        return field
            .as_array_mut()?
            .iter_mut()
            .find(|item| {
                item.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| name.trim().eq_ignore_ascii_case(target_name))
            });
    }
    field
        .get("name")
        .and_then(Value::as_str)
        .is_some_and(|name| name.trim().eq_ignore_ascii_case(target_name))
        .then_some(field)
}

fn set_string_field(object: &mut Value, key: &str, value: &str) -> bool {
    let Some(object) = object.as_object_mut() else {
        return false;
    };
    if object.get(key).and_then(Value::as_str) == Some(value) {
        return false;
    }
    object.insert(key.to_string(), Value::String(value.to_string()));
    true
}

pub(super) async fn sync_updated_pnpm_lockfile(
    config: &Config,
    manifest: &PackageManifest,
    wanted_specifier: &str,
    target_version: &str,
) -> miette::Result<()> {
    let Some(env_root) =
        should_sync_pnpm_lockfile(config, manifest, wanted_specifier, target_version)
    else {
        return Ok(());
    };
    let packages = pnpm_engine_packages(target_version);
    let boot_config = config.for_boot();
    let initial = Box::pin(config_deps::sync_engine_dependencies(
        &boot_config,
        &env_root,
        packages,
        wanted_specifier,
        target_version,
        false,
        false,
    ))
    .await;
    let result = match initial {
        Ok(_) => Ok(()),
        Err(_) if packages.len() > 1 => Box::pin(config_deps::sync_engine_dependencies(
            &boot_config,
            &env_root,
            &["pnpm"],
            wanted_specifier,
            target_version,
            false,
            false,
        ))
        .await
        .map(|_| ()),
        Err(err) => Err(err),
    };
    result.map_err(|err| {
        let reason = pnpm_network::redact_url_credentials(&err.to_string());
        miette::miette!("{reason}").wrap_err("updating pnpm pin in pnpm-lock.yaml")
    })
}

fn should_sync_pnpm_lockfile(
    config: &Config,
    manifest: &PackageManifest,
    wanted_specifier: &str,
    target_version: &str,
) -> Option<PathBuf> {
    if !config.lockfile
        || !version_satisfies(target_version, wanted_specifier)
        || config.package_manager_bootstrap.registry != config.registry
    {
        return None;
    }
    let project_dir = manifest
        .path()
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let roots = PinRoots::of(config, project_dir);
    let mut wanted = wanted_package_manager(manifest.value())?;
    if let Some(on_fail) = config.pm_on_fail {
        wanted.on_fail = Some(on_fail.as_str().to_string());
    }
    let has_existing_pnpm_pin = EnvLockfile::read(&roots.env)
        .ok()
        .flatten()
        .is_some_and(|env| {
            env.importers
                .get(EnvLockfile::ROOT_IMPORTER_KEY)
                .and_then(|imp| imp.package_manager_dependencies.as_ref())
                .is_some_and(|deps| deps.contains_key("pnpm"))
        });
    if !should_persist_package_manager_lockfile(&wanted) && !has_existing_pnpm_pin {
        return None;
    }
    Some(roots.env)
}
