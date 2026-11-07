use super::platform::read_string_list;
use pnpm_package_is_installable::{PackageInstallabilityManifest, WantedEngine};
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct ProjectHostRequirements {
    pub installability: PackageInstallabilityManifest,
    pub node_version: Option<String>,
}

/// What a project `manifest` requires of the host: its `engines.node`,
/// `engines.pnpm`, `os`, `cpu`, and `libc`. `None` when it declares none of
/// them.
#[must_use]
pub fn project_host_requirements(
    manifest: &Value,
    runtime_on_fail: Option<&str>,
) -> Option<ProjectHostRequirements> {
    let engines = manifest.get("engines");
    let engine = |key| {
        engines
            .and_then(|engines| engines.get(key))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let wanted_engine = WantedEngine { node: engine("node"), pnpm: engine("pnpm") };
    let requirements = PackageInstallabilityManifest {
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
    };
    let declares_any = requirements.engines.is_some()
        || requirements.cpu.is_some()
        || requirements.os.is_some()
        || requirements.libc.is_some();
    declares_any.then(|| {
        let mut effective_manifest = manifest.clone();
        if let Some(runtime_on_fail) = runtime_on_fail {
            pnpm_package_manifest::apply_runtime_on_fail_override(
                &mut effective_manifest,
                runtime_on_fail,
            );
        }
        ProjectHostRequirements {
            installability: requirements,
            node_version: pnpm_package_manifest::node_version_from_engines_runtime(
                &effective_manifest,
            ),
        }
    })
}

#[cfg(test)]
mod tests;
