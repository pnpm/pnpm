use miette::IntoDiagnostic;
use serde_json::Value;
use std::{fs, io::ErrorKind, path::Path};

#[derive(Debug)]
struct WantedPackageManager {
    name: String,
    version: Option<String>,
    from_dev_engines: bool,
    on_fail: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PackageManagerToSync {
    pub(crate) specifier: String,
    pub(crate) version: String,
}

pub(crate) fn package_manager_to_sync(
    manifest_path: &Path,
    root_dir: &Path,
) -> miette::Result<Option<PackageManagerToSync>> {
    let Some(manifest) = read_manifest_json(manifest_path)? else {
        return Ok(None);
    };
    let Some(pm) = wanted_package_manager(&manifest) else {
        return Ok(None);
    };
    let Some(wanted_version) = pm.version.as_deref() else {
        return Ok(None);
    };
    if pm.name != "pnpm" || !should_persist_package_manager_lockfile(&pm) {
        return Ok(None);
    }
    let source_version = current_source_pnpm_version().or_else(|| pnpm_version_from(root_dir));
    if let Some(version) =
        source_version.filter(|version| version_satisfies(version, wanted_version))
    {
        return Ok(Some(PackageManagerToSync { specifier: wanted_version.to_string(), version }));
    }
    Ok(exact_version(wanted_version)
        .filter(|version| version_satisfies(version, wanted_version))
        .map(|version| PackageManagerToSync { specifier: wanted_version.to_string(), version }))
}

fn read_manifest_json(path: &Path) -> miette::Result<Option<Value>> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).into_diagnostic(),
    };
    serde_json::from_str(&content).into_diagnostic().map(Some)
}

fn wanted_package_manager(manifest: &Value) -> Option<WantedPackageManager> {
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
    let value = manifest.get("devEngines")?.get("packageManager")?;
    if let Some(items) = value.as_array() {
        if items.is_empty() {
            return None;
        }
        let index = items
            .iter()
            .position(|item| item.get("name").and_then(Value::as_str) == Some("pnpm"))
            .unwrap_or(0);
        let item = &items[index];
        let on_fail =
            item.get("onFail").and_then(Value::as_str).map(ToString::to_string).or_else(|| {
                Some(if index == items.len() - 1 { "error" } else { "ignore" }.to_string())
            });
        return package_manager_from_engine(item, true, on_fail);
    }
    package_manager_from_engine(
        value,
        true,
        value.get("onFail").and_then(Value::as_str).map(ToString::to_string),
    )
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
    // Split on the `@` that separates the name from the reference. A leading
    // `@` belongs to a scoped name (e.g. `@scope/pm@1.2.3`), so skip it;
    // otherwise the first `@` is the separator. The *first* `@` (not the last)
    // is used so a reference that is a URL containing `@` (e.g. credentials)
    // stays intact. Mirrors pnpm's `parsePackageManager`
    // <https://github.com/pnpm/pnpm/blob/8eb1be4988/config/reader/src/index.ts#L895-L908>.
    let separator_index = if let Some(rest) = package_manager.strip_prefix('@') {
        rest.find('@').map(|index| index + 1)
    } else {
        package_manager.find('@')
    };
    let Some(separator_index) = separator_index else {
        return (package_manager.to_string(), None);
    };
    let name = &package_manager[..separator_index];
    let reference = &package_manager[separator_index + 1..];
    if reference.contains(':') {
        return (name.to_string(), None);
    }
    (
        name.to_string(),
        Some(reference.split_once('+').map_or(reference, |(version, _)| version).to_string()),
    )
}

fn should_persist_package_manager_lockfile(pm: &WantedPackageManager) -> bool {
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

fn exact_version(version: &str) -> Option<String> {
    let parsed = node_semver::Version::parse(version).ok()?;
    (parsed.to_string() == version).then(|| version.to_string())
}

fn version_satisfies(version: &str, wanted_range: &str) -> bool {
    let Ok(version) = node_semver::Version::parse(version) else {
        return false;
    };
    let Ok(range) = node_semver::Range::parse(wanted_range) else {
        return false;
    };
    if version.satisfies(&range) {
        return true;
    }
    if version.pre_release.is_empty() {
        return false;
    }
    let base = node_semver::Version {
        major: version.major,
        minor: version.minor,
        patch: version.patch,
        pre_release: Vec::new(),
        build: version.build,
    };
    base.satisfies(&range)
}
