pub(super) mod persist;
pub(super) mod update;

pub(crate) use update::{EngineUpdateOptions, EngineUpdateOutcome, update_engine_deps};

use std::{collections::HashSet, path::Path};

use node_semver::{Range, Version};
use pnpm_config::{Config, PmOnFail, RuntimeOnFail};
use pnpm_engine_runtime_node_resolver::parse_node_specifier;
use pnpm_lockfile::EnvLockfile;
use pnpm_package_manager::parse_update_param;
use pnpm_package_manifest::{
    DependencyGroup, PackageManifest, is_runtime_alias,
    package_manager_spec::{
        dev_engines_package_managers, is_version_request, version_without_build,
    },
};
use pnpm_tool_protocol::{RUNTIME_PROTOCOL, strip_tool_protocol};
use serde_json::Value;

use crate::{
    cli_args::package_manager::{
        PinRoots, exact_version, parse_package_manager, version_satisfies,
    },
    engine_pm::channel::{Channel, PackageManager},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PmDeclarationLocation {
    DevEnginesOnly,
    PackageManagerFieldOnly,
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EngineDepKind {
    PackageManager { pm: PackageManager, location: PmDeclarationLocation },
    Runtime { engines_field: &'static str },
}

#[derive(Debug, Clone)]
pub(crate) struct DeclaredEngineDep {
    pub(crate) name: String,
    pub(crate) raw_specifier: String,
    pub(crate) resolver_bare_specifier: String,
    pub(crate) current: Option<Version>,
    pub(crate) group: DependencyGroup,
    pub(crate) kind: EngineDepKind,
}

/// Whether `packages` is non-empty and every selector targets a package manager
/// or runtime engine.
pub(crate) fn has_only_engine_selectors(packages: &[String]) -> bool {
    !packages.is_empty() && packages.iter().all(|spec| is_engine_selector(spec))
}

/// Whether `selector` targets a package manager or runtime engine.
pub(crate) fn is_engine_selector(selector: &str) -> bool {
    let parsed = parse_update_param(selector);
    let name = parsed.pattern.strip_prefix('!').unwrap_or(&parsed.pattern);
    if PackageManager::parse(name).is_none() && !is_runtime_alias(name) {
        return false;
    }
    parsed.version
        .as_deref()
        .is_none_or(|version| {
            let stripped = strip_tool_protocol(version).unwrap_or(version);
            is_version_request(stripped)
                || (name.eq_ignore_ascii_case("node") && parse_node_specifier(stripped).is_ok())
        })
}

/// Minimum semver version of a runtime or package-manager specifier when no
/// lockfile entry pins the resolved version.
pub(crate) fn min_version_of_spec(name: &str, spec: &str) -> Option<Version> {
    let mut spec_text = strip_tool_protocol(spec).unwrap_or(spec).trim();
    let parsed_spec;
    if name.eq_ignore_ascii_case("node")
        && let Ok(parsed) = parse_node_specifier(spec_text)
    {
        parsed_spec = parsed.version_specifier;
        spec_text = &parsed_spec;
    }
    let spec_text = version_without_build(spec_text);
    Version::parse(spec_text)
        .ok()
        .or_else(|| Range::parse(spec_text).ok().and_then(|range| range.min_version()))
}

pub(crate) fn pm_resolver_bare_specifier(pm: PackageManager, spec: &str) -> String {
    let clean = version_without_build(spec.trim());
    match pm.channel(clean) {
        Channel::Registry { package } if package == pm.name() => clean.to_string(),
        Channel::Registry { package } => format!("npm:{package}@{clean}"),
        Channel::Binary(_) => format!("{RUNTIME_PROTOCOL}{clean}"),
    }
}

/// Package managers (`devEngines.packageManager` / `packageManager`) and
/// non-reified runtimes (`devEngines.runtime` / `engines.runtime` without
/// `onFail: "download"`) declared by `manifest`.
pub(crate) fn declared_engine_deps(
    manifest: &PackageManifest,
    config: &Config,
    include_direct: &[DependencyGroup],
) -> Vec<DeclaredEngineDep> {
    let mut out = Vec::new();
    let existing_deps: HashSet<String> = manifest
        .dependencies([DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional])
        .map(|(name, _)| name.to_string())
        .collect();

    if include_direct.contains(&DependencyGroup::Dev) && config.pm_on_fail != Some(PmOnFail::Ignore)
    {
        collect_package_manager_deps(manifest, config, &existing_deps, &mut out);
    }

    if config.runtime_on_fail != Some(RuntimeOnFail::Ignore) {
        if include_direct.contains(&DependencyGroup::Dev) {
            collect_runtime_deps(
                manifest,
                "devEngines",
                DependencyGroup::Dev,
                &existing_deps,
                &mut out,
            );
        }
        if include_direct.contains(&DependencyGroup::Prod) {
            collect_runtime_deps(
                manifest,
                "engines",
                DependencyGroup::Prod,
                &existing_deps,
                &mut out,
            );
        }
    }

    out
}

fn collect_package_manager_deps(
    manifest: &PackageManifest,
    config: &Config,
    existing_deps: &HashSet<String>,
    out: &mut Vec<DeclaredEngineDep>,
) {
    let value = manifest.value();
    let legacy_pm = value
        .get("packageManager")
        .and_then(Value::as_str)
        .map(parse_package_manager);
    let mut seen_pms = HashSet::new();

    for entry in dev_engines_package_managers(value) {
        if let Some(dep) = dev_engine_pm_dep(
            entry,
            manifest,
            config,
            existing_deps,
            &mut seen_pms,
            legacy_pm.as_ref(),
        ) {
            out.push(dep);
        }
    }

    if let Some(dep) =
        legacy_pm_dep(manifest, config, existing_deps, &mut seen_pms, legacy_pm.as_ref())
    {
        out.push(dep);
    }
}

fn dev_engine_pm_dep(
    entry: &Value,
    manifest: &PackageManifest,
    config: &Config,
    existing_deps: &HashSet<String>,
    seen_pms: &mut HashSet<&'static str>,
    legacy_pm: Option<&(String, Option<String>)>,
) -> Option<DeclaredEngineDep> {
    let raw_name = entry
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)?;
    let pm = PackageManager::parse(raw_name)?;
    if !seen_pms.insert(pm.name()) || is_ignored_on_fail(entry) || existing_deps.contains(pm.name())
    {
        return None;
    }
    let raw_spec = non_empty_version_field(entry)?;
    let in_pm_field = legacy_pm.is_some_and(|(legacy_name, ver)| {
        legacy_name.eq_ignore_ascii_case(pm.name()) && ver.is_some()
    });
    let location = if in_pm_field {
        PmDeclarationLocation::Both
    } else {
        PmDeclarationLocation::DevEnginesOnly
    };
    let current = current_package_manager_version(manifest, config, pm, raw_spec, legacy_pm);
    Some(DeclaredEngineDep {
        name: pm.name().to_string(),
        raw_specifier: raw_spec.to_string(),
        resolver_bare_specifier: pm_resolver_bare_specifier(pm, raw_spec),
        current,
        group: DependencyGroup::Dev,
        kind: EngineDepKind::PackageManager { pm, location },
    })
}

fn legacy_pm_dep(
    manifest: &PackageManifest,
    config: &Config,
    existing_deps: &HashSet<String>,
    seen_pms: &mut HashSet<&'static str>,
    legacy_pm: Option<&(String, Option<String>)>,
) -> Option<DeclaredEngineDep> {
    let (legacy_name, Some(legacy_version)) = legacy_pm? else {
        return None;
    };
    let pm = PackageManager::parse(legacy_name)?;
    if !seen_pms.insert(pm.name()) || existing_deps.contains(pm.name()) || legacy_version.is_empty()
    {
        return None;
    }
    let current = current_package_manager_version(manifest, config, pm, legacy_version, legacy_pm);
    Some(DeclaredEngineDep {
        name: pm.name().to_string(),
        raw_specifier: legacy_version.clone(),
        resolver_bare_specifier: pm_resolver_bare_specifier(pm, legacy_version),
        current,
        group: DependencyGroup::Dev,
        kind: EngineDepKind::PackageManager {
            pm,
            location: PmDeclarationLocation::PackageManagerFieldOnly,
        },
    })
}

fn current_package_manager_version(
    manifest: &PackageManifest,
    config: &Config,
    pm: PackageManager,
    raw_specifier: &str,
    legacy_pm: Option<&(String, Option<String>)>,
) -> Option<Version> {
    let clean_spec = version_without_build(raw_specifier.trim());
    if pm == PackageManager::Pnpm
        && let Some(parsed) = locked_pnpm_version(manifest, config, clean_spec)
    {
        return Some(parsed);
    }
    if let Some((legacy_name, Some(legacy_ver))) = legacy_pm
        && legacy_name.eq_ignore_ascii_case(pm.name())
        && let Some(exact) = exact_version(legacy_ver)
        && version_satisfies(&exact, clean_spec)
        && let Ok(parsed) = Version::parse(&exact)
    {
        return Some(parsed);
    }
    min_version_of_spec(pm.name(), clean_spec)
}

fn locked_pnpm_version(
    manifest: &PackageManifest,
    config: &Config,
    clean_spec: &str,
) -> Option<Version> {
    let project_dir = manifest
        .path()
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let roots = PinRoots::of(config, project_dir);
    let locked_ver = EnvLockfile::read(&roots.env)
        .ok()
        .flatten()
        .and_then(|env| {
            env.importers
                .get(EnvLockfile::ROOT_IMPORTER_KEY)
                .and_then(|importer| importer.package_manager_dependencies.as_ref())
                .and_then(|deps| deps.get("pnpm"))
                .map(|dep| dep.version.clone())
        })?;
    version_satisfies(&locked_ver, clean_spec)
        .then(|| Version::parse(&locked_ver).ok())
        .flatten()
}

fn collect_runtime_deps(
    manifest: &PackageManifest,
    engines_field: &'static str,
    group: DependencyGroup,
    existing_deps: &HashSet<String>,
    out: &mut Vec<DeclaredEngineDep>,
) {
    let Some(runtime_entry) = manifest
        .value()
        .get(engines_field)
        .and_then(|engines| engines.get("runtime"))
    else {
        return;
    };
    let entries: &[Value] = match runtime_entry {
        Value::Array(arr) => arr.as_slice(),
        single @ Value::Object(_) => std::slice::from_ref(single),
        _ => return,
    };
    let mut seen_runtimes = HashSet::new();
    for entry in entries {
        if let Some(dep) =
            runtime_dep_from_entry(entry, engines_field, group, existing_deps, &mut seen_runtimes)
        {
            out.push(dep);
        }
    }
}

fn runtime_dep_from_entry(
    entry: &Value,
    engines_field: &'static str,
    group: DependencyGroup,
    existing_deps: &HashSet<String>,
    seen_runtimes: &mut HashSet<String>,
) -> Option<DeclaredEngineDep> {
    let raw_name = entry
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)?;
    let normalized_name = raw_name.to_ascii_lowercase();
    if !is_runtime_alias(&normalized_name)
        || !seen_runtimes.insert(normalized_name.clone())
        || is_ignored_or_downloaded_runtime(entry)
        || existing_deps.contains(&normalized_name)
    {
        return None;
    }
    let raw_spec = non_empty_version_field(entry)?;
    let current = min_version_of_spec(&normalized_name, raw_spec);
    Some(DeclaredEngineDep {
        name: normalized_name,
        raw_specifier: raw_spec.to_string(),
        resolver_bare_specifier: format!("{RUNTIME_PROTOCOL}{raw_spec}"),
        current,
        group,
        kind: EngineDepKind::Runtime { engines_field },
    })
}

fn is_ignored_on_fail(entry: &Value) -> bool {
    entry
        .get("onFail")
        .and_then(Value::as_str)
        .is_some_and(|on_fail| on_fail.eq_ignore_ascii_case("ignore"))
}

fn is_ignored_or_downloaded_runtime(entry: &Value) -> bool {
    entry
        .get("onFail")
        .and_then(Value::as_str)
        .is_some_and(|on_fail| {
            on_fail.eq_ignore_ascii_case("ignore") || on_fail.eq_ignore_ascii_case("download")
        })
}

fn non_empty_version_field(entry: &Value) -> Option<&str> {
    entry
        .get("version")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|ver| !ver.is_empty())
}
