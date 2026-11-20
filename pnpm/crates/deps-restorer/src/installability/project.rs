use super::platform::read_string_list;
use pnpm_package_is_installable::{
    PackageInstallabilityManifest,
    WantedEngine,
};
use serde_json::Value;

/// What a project requires of the host, per [`project_host_requirements`].
#[derive(Debug, Clone)]
pub struct ProjectHostRequirements {
    pub installability: PackageInstallabilityManifest,
    pub dependencies_node_version: Option<String>,
    pub dev_dependencies_node_version: Option<String>,
    pub dependencies_node_specifier: Option<String>,
    pub dev_dependencies_node_specifier: Option<String>,
}

/// What a project `manifest` requires of the host: its `engines.node`,
/// `engines.pnpm`, `os`, `cpu`, and `libc`, and the Node.js its own runtime
/// pin downloads under `runtime_on_fail` (the `runtimeOnFail` setting).
/// `None` when it declares none of the requirements.
#[must_use]
pub fn project_host_requirements(
    manifest: &Value,
    runtime_on_fail: Option<&str>,
) -> Option<ProjectHostRequirements> {
    let installability = project_installability_manifest(manifest);
    let (dependencies_node_version, dependencies_node_specifier) =
        downloaded_node_runtime(manifest, "dependencies", "engines", runtime_on_fail);
    let (dev_dependencies_node_version, dev_dependencies_node_specifier) =
        downloaded_node_runtime(manifest, "devDependencies", "devEngines", runtime_on_fail);
    if installability.engines.is_none()
        && installability.cpu.is_none()
        && installability.os.is_none()
        && installability.libc.is_none()
        && dependencies_node_version.is_none()
        && dev_dependencies_node_version.is_none()
    {
        return None;
    }
    Some(ProjectHostRequirements {
        installability,
        dependencies_node_version,
        dev_dependencies_node_version,
        dependencies_node_specifier,
        dev_dependencies_node_specifier,
    })
}

fn project_installability_manifest(manifest: &Value) -> PackageInstallabilityManifest {
    let engines = manifest.get("engines");
    let engine = |key| {
        engines
            .and_then(|engines| engines.get(key))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let wanted_engine = WantedEngine { node: engine("node"), pnpm: engine("pnpm") };
    PackageInstallabilityManifest {
        name: manifest
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        engines: (wanted_engine.node.is_some() || wanted_engine.pnpm.is_some()).then_some(
            wanted_engine,
        ),
        cpu: read_string_list(Some(manifest), "cpu"),
        os: read_string_list(Some(manifest), "os"),
        libc: read_string_list(Some(manifest), "libc"),
    }
}

fn downloaded_node_runtime(
    manifest: &Value,
    dependencies_field: &str,
    engines_field: &str,
    runtime_on_fail: Option<&str>,
) -> (Option<String>, Option<String>) {
    if let Some((specifier, version)) =
        pnpm_package_manifest::downloaded_node_runtime_from_dependency_field(
            manifest,
            dependencies_field,
        )
        .filter(|(specifier, _)| {
            !runtime_on_fail.is_some_and(|policy| {
                policy != "download"
                    && runtime_dependency_matches_engine_runtime(manifest, engines_field, specifier)
            })
        })
    {
        return (Some(version), Some(specifier));
    }
    (
        pnpm_package_manifest::downloaded_node_version_from_engine_field(
            manifest,
            engines_field,
            runtime_on_fail,
        ),
        pnpm_package_manifest::downloaded_node_specifier_from_engine_field(
            manifest,
            engines_field,
            runtime_on_fail,
        ),
    )
}

fn runtime_dependency_matches_engine_runtime(
    manifest: &Value,
    engines_field: &str,
    dependency_specifier: &str,
) -> bool {
    let Some(runtime_entry) = manifest
        .get(engines_field)
        .and_then(|engines| engines.get("runtime"))
    else {
        return false;
    };
    let runtimes = match runtime_entry {
        Value::Array(runtimes) => runtimes.as_slice(),
        runtime @ Value::Object(_) => std::slice::from_ref(runtime),
        _ => return false,
    };
    runtimes
        .iter()
        .any(|runtime| {
            runtime.get("name").and_then(Value::as_str) == Some("node")
                && runtime
                    .get("version")
                    .and_then(Value::as_str)
                    .is_some_and(|version| {
                        dependency_specifier.strip_prefix("runtime:") == Some(version.trim())
                    })
        })
}

#[cfg(test)]
mod tests;
