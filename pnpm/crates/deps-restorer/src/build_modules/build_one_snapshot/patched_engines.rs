use super::{
    super::BuildModulesError,
    BuildCandidate, BuildOneSnapshot, PackageKey,
    linked_copies::{LinkTargets, unlink_children, unlink_project_links},
};
use pnpm_reporter::{
    LogEvent, LogLevel, Reporter, SkippedOptionalDependencyLog, SkippedOptionalPackage,
    SkippedOptionalReason,
};

pub(super) fn skip_incompatible_optional<EventReporter: Reporter>(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    candidate: &BuildCandidate<'_>,
) -> Result<bool, BuildModulesError> {
    let optional = context.graph.snapshots.get(snapshot_key).is_some_and(|entry| entry.optional);
    let Some(details) = enforce_patched_engines(context, snapshot_key, candidate, optional)? else {
        return Ok(false);
    };
    EventReporter::emit(&LogEvent::SkippedOptionalDependency(SkippedOptionalDependencyLog {
        level: LogLevel::Debug,
        details: Some(details),
        package: SkippedOptionalPackage::Installed {
            id: snapshot_key.to_string(),
            name: candidate.name.clone(),
            version: candidate.version.clone(),
        },
        parents: None,
        prefix: context.directories.lockfile_dir.to_string_lossy().into_owned(),
        reason: SkippedOptionalReason::UnsupportedEngine,
    }));
    remove_linked_copies(context, snapshot_key);
    Ok(true)
}

/// Remove an engine-incompatible optional dependency and every link to it.
/// Cleanup failures leave a stale entry for the next install to repair, so
/// they are not reported.
fn remove_linked_copies(context: &BuildOneSnapshot<'_>, snapshot_key: &PackageKey) {
    let dirs = context.pkg_roots().all(snapshot_key);
    let targets = LinkTargets::resolve(&dirs);
    let layout = context.directories.layout;
    let _ = unlink_project_links(context, &targets);
    // A global virtual store slot, and the links between slots, are shared
    // with every other project that resolves to them.
    if layout.enable_global_virtual_store()
        && context.directories.pkg_roots_by_key.is_none()
    {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(layout.package_store_dir()) {
        for entry in entries.flatten() {
            let _ = unlink_children(&entry.path().join("node_modules"), &targets);
        }
    }
    for dir in &dirs {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// Evaluates `engineStrict` against the patched manifest.
///
/// Returns `Some(details)` when an optional package should be skipped, or `None` if compatible.
pub(super) fn enforce_patched_engines(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    candidate: &BuildCandidate<'_>,
    optional: bool,
) -> Result<Option<String>, BuildModulesError> {
    if !context.scripts.patched_engines.engine_strict || candidate.patch.is_none() {
        return Ok(None);
    }
    let Some(pkg_dir) = context.pkg_roots().canonical(snapshot_key) else {
        return Ok(None);
    };
    let manifest = read_package_json(&pkg_dir)
        .map_err(|()| BuildModulesError::PatchedManifestUnreadable {
            dep_path: snapshot_key.to_string(),
        })?;
    check_patched_manifest(context, snapshot_key, candidate, &manifest, optional)
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
    optional: bool,
) -> Result<Option<String>, BuildModulesError> {
    let installability = pnpm_package_is_installable::PackageInstallabilityManifest {
        name: candidate.name.clone(),
        engines: wanted_engines(manifest),
        ..pnpm_package_is_installable::PackageInstallabilityManifest::default()
    };
    let host = crate::installability::InstallabilityHost::detect_with(
        true,
        context.scripts.patched_engines.node_version.map(str::to_string),
    );
    match pnpm_package_is_installable::package_is_installable(
        &snapshot_key.to_string(),
        &installability,
        &installability_options(&host, optional),
    ) {
        Ok(pnpm_package_is_installable::InstallabilityVerdict::SkipOptional {
            details, ..
        }) => Ok(Some(details)),
        Ok(_) => Ok(None),
        Err(source) => Err(BuildModulesError::PatchedEngines(source)),
    }
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
    optional: bool,
) -> pnpm_package_is_installable::InstallabilityOptions<'_> {
    pnpm_package_is_installable::InstallabilityOptions {
        engine_strict: true,
        optional,
        current_node_version: host.node_version.as_str(),
        pnpm_version: None,
        current_os: host.os,
        current_cpu: host.cpu,
        current_libc: host.libc,
        supported_architectures: None,
    }
}
