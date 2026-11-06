use super::platform::read_string_list;
use pnpm_package_is_installable::{PackageInstallabilityManifest, WantedEngine};
use serde_json::Value;

/// What a project `manifest` requires of the host: its `engines.node`,
/// `engines.pnpm`, `os`, `cpu`, and `libc`. `None` when it declares none of
/// them.
#[must_use]
pub fn project_host_requirements(manifest: &Value) -> Option<PackageInstallabilityManifest> {
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
    declares_any.then_some(requirements)
}

#[cfg(test)]
mod tests;
