use super::{super::BuildModulesError, BuildCandidate, BuildOneSnapshot, PackageKey};
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

fn remove_linked_copies(context: &BuildOneSnapshot<'_>, snapshot_key: &PackageKey) {
    let dirs = context.pkg_roots().all(snapshot_key);
    let layout = context.directories.layout;
    let store = layout.package_store_dir();
    for modules_dir in project_modules_dirs(context) {
        unlink_children(&modules_dir, &dirs);
    }
    let virtual_store_dir = context.scripts.patched_engines.virtual_store_dir.unwrap_or(store);
    unlink_children(&virtual_store_dir.join("node_modules"), &dirs);
    // A global virtual store slot, and the links between slots, are shared
    // with every other project that resolves to them.
    if layout.enable_global_virtual_store()
        && context.directories.pkg_roots_by_key.is_none()
    {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(store) {
        for entry in entries.flatten() {
            unlink_children(&entry.path().join("node_modules"), &dirs);
        }
    }
    for dir in &dirs {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// The `node_modules` directory of every project in the lockfile. An importer
/// key that would escape the lockfile directory is left out.
fn project_modules_dirs(context: &BuildOneSnapshot<'_>) -> Vec<std::path::PathBuf> {
    let root = context.directories.modules_dir;
    let lockfile_dir = context.directories.lockfile_dir;
    let mut dirs = vec![root.to_path_buf()];
    let Ok(modules_dir_name) = root.strip_prefix(lockfile_dir) else { return dirs };
    dirs.extend(
        context.graph.importers
            .keys()
            .filter(|importer_id| importer_id.as_str() != ".")
            .filter(|importer_id| crate::validate_importer_id(importer_id).is_ok())
            .map(|importer_id| lockfile_dir.join(importer_id).join(modules_dir_name)),
    );
    dirs
}

fn unlink_children(dir: &std::path::Path, dirs: &[std::path::PathBuf]) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if unlink_if_points(&path, dirs) {
            continue;
        }
        unlink_scope(&path, dirs);
    }
}

fn unlink_scope(path: &std::path::Path, dirs: &[std::path::PathBuf]) {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else { return };
    let is_real_dir = std::fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink());
    if !name.starts_with('@') || !is_real_dir {
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else { return };
    for entry in entries.flatten() {
        let _ = unlink_if_points(&entry.path(), dirs);
    }
}

fn unlink_if_points(link: &std::path::Path, dirs: &[std::path::PathBuf]) -> bool {
    let Ok(pointed) = std::fs::read_link(link) else { return false };
    let pointed = link
        .parent()
        .unwrap_or(link)
        .join(pointed);
    let pointed = std::fs::canonicalize(&pointed).unwrap_or(pointed);
    let hits = dirs
        .iter()
        .any(|dir| std::fs::canonicalize(dir).unwrap_or_else(|_| dir.clone()) == pointed);
    if hits {
        let _ = std::fs::remove_file(link).or_else(|_| std::fs::remove_dir(link));
    }
    hits
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
