use super::super::BuildModulesError;
use super::{BuildCandidate, BuildOneSnapshot, PackageKey};

/// `engineStrict` against the patched manifest. The earlier installability
/// pass skipped engines for patched packages because the published manifest
/// was still the only one on record.
pub(super) fn enforce_patched_engines(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    candidate: &BuildCandidate<'_>,
) -> Result<(), BuildModulesError> {
    if !context.scripts.patched_engines.engine_strict || candidate.patch.is_none() {
        return Ok(());
    }
    let Some(pkg_dir) = context.pkg_roots().canonical(snapshot_key) else {
        return Ok(());
    };
    let Ok(manifest) = read_package_json(&pkg_dir) else {
        return Ok(());
    };
    check_patched_manifest(context, snapshot_key, candidate, &manifest)
}

fn read_package_json(pkg_dir: &std::path::Path) -> Result<serde_json::Value, ()> {
    let raw = std::fs::read(pkg_dir.join("package.json")).map_err(|_| ())?;
    serde_json::from_slice(&raw).map_err(|_| ())
}

fn check_patched_manifest(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    candidate: &BuildCandidate<'_>,
    manifest: &serde_json::Value,
) -> Result<(), BuildModulesError> {
    let installability = pnpm_package_is_installable::PackageInstallabilityManifest {
        name: candidate.name.clone(),
        engines: wanted_engines(manifest),
        ..pnpm_package_is_installable::PackageInstallabilityManifest::default()
    };
    let host = crate::installability::InstallabilityHost::detect_with(
        true,
        context.scripts.patched_engines.node_version.map(str::to_string),
    );
    pnpm_package_is_installable::package_is_installable(
        &snapshot_key.to_string(),
        &installability,
        &installability_options(&host),
    )
    .map(|_| ())
    .map_err(BuildModulesError::PatchedEngines)
}

fn wanted_engines(
    manifest: &serde_json::Value,
) -> Option<pnpm_package_is_installable::WantedEngine> {
    let engines = manifest.get("engines")?.as_object()?;
    Some(pnpm_package_is_installable::WantedEngine {
        node: engines
            .get("node")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        pnpm: engines
            .get("pnpm")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    })
}

fn installability_options(
    host: &crate::installability::InstallabilityHost,
) -> pnpm_package_is_installable::InstallabilityOptions<'_> {
    pnpm_package_is_installable::InstallabilityOptions {
        engine_strict: true,
        optional: false,
        current_node_version: host.node_version.as_str(),
        pnpm_version: None,
        current_os: host.os,
        current_cpu: host.cpu,
        current_libc: host.libc,
        supported_architectures: None,
    }
}
