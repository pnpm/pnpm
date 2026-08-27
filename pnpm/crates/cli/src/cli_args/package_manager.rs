use miette::IntoDiagnostic;
use pnpm_config::{PNPM_VERSION, PmOnFail};
use pnpm_env_installer::is_package_manager_resolved;
use pnpm_lockfile::EnvLockfile;
use pnpm_package_manifest::{
    package_manager_spec::{
        dev_engines_package_managers, is_version_request, split_spec, version_without_build,
    },
    parse_manifest,
};
use pnpm_semver_include_prerelease::IncludePrereleaseRange;
use serde_json::Value;
use std::{fs, io::ErrorKind, path::Path};

pub(crate) const PACKAGE_MANAGER_SWITCH_ENV_VARS: [&str; 4] = [
    "npm_config_manage_package_manager_versions",
    "NPM_CONFIG_MANAGE_PACKAGE_MANAGER_VERSIONS",
    "pnpm_config_manage_package_manager_versions",
    "PNPM_CONFIG_MANAGE_PACKAGE_MANAGER_VERSIONS",
];

#[derive(Debug)]
pub(crate) struct WantedPackageManager {
    pub(crate) name: String,
    pub(crate) version: Option<String>,
    pub(crate) from_dev_engines: bool,
    pub(crate) on_fail: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PackageManagerToSync {
    pub(crate) specifier: String,
    pub(crate) version: String,
}

/// The pnpm version to record under the env lockfile's
/// `packageManagerDependencies`, or `None` when the project doesn't pin pnpm
/// in a way that persists (see [`should_persist_package_manager_lockfile`]).
/// `on_fail` is the effective policy — the `pmOnFail` setting when it is set,
/// which overrides the manifest's own `onFail`.
pub(crate) fn package_manager_to_sync(
    manifest: &Value,
    root_dir: &Path,
    on_fail: Option<PmOnFail>,
) -> Option<PackageManagerToSync> {
    let mut pm = wanted_package_manager(manifest)?;
    if let Some(on_fail) = on_fail {
        pm.on_fail = Some(on_fail.as_str().to_string());
    }
    let wanted_version = pm.version.as_deref()?;
    if pm.name != "pnpm" || !should_persist_package_manager_lockfile(&pm) {
        return None;
    }
    let source_version = current_source_pnpm_version().or_else(|| pnpm_version_from(root_dir));
    if let Some(version) =
        source_version.filter(|version| version_satisfies(version, wanted_version))
    {
        return Some(PackageManagerToSync { specifier: wanted_version.to_string(), version });
    }
    if let Some(version) =
        exact_version(wanted_version).filter(|version| version_satisfies(version, wanted_version))
    {
        return Some(PackageManagerToSync { specifier: wanted_version.to_string(), version });
    }
    // A range pin names no exact version, so the running pnpm's version is
    // the one the project actually uses.
    version_satisfies(PNPM_VERSION, wanted_version).then(|| PackageManagerToSync {
        specifier: wanted_version.to_string(),
        version: PNPM_VERSION.to_string(),
    })
}

/// Whether the pnpm version the manifest at `root_dir` pins still has to be
/// recorded in the env lockfile there. The install family records it from
/// its own pipeline, so an install that short-circuits before the pipeline
/// has to give up its fast path — a plain install would otherwise keep
/// reporting success while leaving every `--frozen-lockfile` run failing on
/// the unwritten entry.
///
/// `root_manifest` is that manifest when the caller already holds it, so a
/// fast path does not read the same file twice.
///
/// A manifest that cannot be read answers `false`: the full install path
/// reports that, and a fast path is not where it should surface.
pub(crate) fn package_manager_needs_recording(
    root_dir: &Path,
    on_fail: Option<PmOnFail>,
    root_manifest: Option<&Value>,
) -> bool {
    let read;
    let manifest = if let Some(manifest) = root_manifest {
        manifest
    } else {
        let Ok(Some(manifest)) = read_manifest_json(&root_dir.join("package.json")) else {
            return false;
        };
        read = manifest;
        &read
    };
    let Some(package_manager) = package_manager_to_sync(manifest, root_dir, on_fail) else {
        return false;
    };
    !EnvLockfile::read(root_dir).ok().flatten().is_some_and(|env_lockfile| {
        is_package_manager_resolved(
            &env_lockfile,
            &package_manager.specifier,
            &package_manager.version,
        )
    })
}

pub(crate) fn read_manifest_json(path: &Path) -> miette::Result<Option<Value>> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).into_diagnostic(),
    };
    parse_manifest(&content).into_diagnostic().map(Some)
}

pub(crate) fn wanted_package_manager(manifest: &Value) -> Option<WantedPackageManager> {
    if let Some(mut pm) = parse_dev_engines_package_manager(manifest) {
        if pm.version.as_deref().is_some_and(|version| node_semver::Range::parse(version).is_err())
        {
            pm.version = None;
        }
        return Some(pm);
    }
    let package_manager = manifest.get("packageManager")?.as_str()?;
    let (name, version) = parse_package_manager(package_manager);
    let version = version.and_then(|version| exact_version(&version));
    Some(WantedPackageManager { name, version, from_dev_engines: false, on_fail: None })
}

fn parse_dev_engines_package_manager(manifest: &Value) -> Option<WantedPackageManager> {
    let entries: Vec<&Value> = dev_engines_package_managers(manifest).collect();
    let declared_as_list = manifest.get("devEngines")?.get("packageManager")?.is_array();
    let (index, entry) = if declared_as_list {
        // pnpm's own entry is the one that governs this CLI; without one,
        // the first entry does.
        let index = entries
            .iter()
            .position(|entry| entry.get("name").and_then(Value::as_str) == Some("pnpm"))
            .unwrap_or(0);
        (Some(index), *entries.get(index)?)
    } else {
        (None, *entries.first()?)
    };
    let on_fail =
        entry.get("onFail").and_then(Value::as_str).map(ToString::to_string).or_else(|| {
            let index = index?;
            Some(if index == entries.len() - 1 { "error" } else { "ignore" }.to_string())
        });
    package_manager_from_engine(entry, true, on_fail)
}

fn package_manager_from_engine(
    value: &Value,
    from_dev_engines: bool,
    on_fail: Option<String>,
) -> Option<WantedPackageManager> {
    Some(WantedPackageManager {
        name: value.get("name")?.as_str()?.to_string(),
        version: value.get("version").and_then(Value::as_str).map(ToString::to_string),
        from_dev_engines,
        on_fail,
    })
}

pub(crate) fn parse_package_manager(package_manager: &str) -> (String, Option<String>) {
    let (name, reference) = split_spec(package_manager);
    let version = reference
        .filter(|reference| is_version_request(reference))
        .map(|reference| version_without_build(reference).to_string());
    (name.to_string(), version)
}

pub(crate) fn should_persist_package_manager_lockfile(pm: &WantedPackageManager) -> bool {
    if pm.on_fail.as_deref().unwrap_or("download") == "ignore" {
        return false;
    }
    if pm.from_dev_engines {
        return true;
    }
    pm.version
        .as_deref()
        .and_then(|version| node_semver::Version::parse(version).ok())
        .is_some_and(|version| version.major >= 12)
}

pub(crate) fn current_source_pnpm_version() -> Option<String> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.ancestors().find_map(pnpm_version_from)
}

fn pnpm_version_from(root_dir: &Path) -> Option<String> {
    let path = root_dir.join("pnpm11").join("pnpm").join("package.json");
    let value = read_manifest_json(&path).ok()??;
    value.get("version").and_then(Value::as_str).map(ToString::to_string)
}

/// The one version `version` pins, or `None` when it pins a range, a
/// dist-tag, or nothing at all.
///
/// The `+<algorithm>.<hash>` build corepack records the artifact it
/// downloaded with is dropped: it names corepack's download, not a pnpm
/// release, so the registry resolves the version without it and a lockfile
/// entry that kept it would never match the version recorded beside it.
pub(crate) fn exact_version(version: &str) -> Option<String> {
    let parsed = node_semver::Version::parse(version).ok()?;
    (parsed.to_string() == version).then(|| version_without_build(version).to_string())
}

/// pnpm's `semver.satisfies(version, wanted_range, { includePrerelease:
/// true })`, the call every version pin is read with — a package manager's
/// and a runtime's alike.
///
/// A version no parser accepts satisfies nothing, as upstream `satisfies`
/// answers `false` for one rather than throwing.
pub(crate) fn version_satisfies(version: &str, wanted_range: &str) -> bool {
    node_semver::Version::parse(version)
        .is_ok_and(|version| IncludePrereleaseRange::parse(wanted_range).satisfies(&version))
}

#[cfg(test)]
mod tests;
