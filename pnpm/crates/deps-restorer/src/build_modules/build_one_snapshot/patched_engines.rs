use super::{
    super::{BuildModulesError, discard_failed_global_virtual_store_slot},
    BuildCandidate, BuildOneSnapshot, PackageKey,
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
    remove_linked_copies(context, snapshot_key, &candidate.name);
    discard_failed_global_virtual_store_slot(context.directories.layout, snapshot_key);
    Ok(true)
}

fn remove_linked_copies(context: &BuildOneSnapshot<'_>, snapshot_key: &PackageKey, name: &str) {
    let dirs = context.pkg_roots().all(snapshot_key);
    let modules = context.directories.lockfile_dir.join("node_modules");
    unlink_named(&modules, name, &dirs);
    unlink_named(&modules.join(".pnpm").join("node_modules"), name, &dirs);
    if let Ok(store) = std::fs::read_dir(modules.join(".pnpm")) {
        for entry in store.flatten() {
            unlink_named(&entry.path().join("node_modules"), name, &dirs);
        }
    }
    for dir in &dirs {
        let _ = std::fs::remove_dir_all(dir);
    }
}

fn unlink_named(modules: &std::path::Path, name: &str, dirs: &[std::path::PathBuf]) {
    let link = modules.join(name);
    let Ok(pointed) = std::fs::read_link(&link) else { return };
    let pointed = link
        .parent()
        .unwrap_or(modules)
        .join(pointed);
    let pointed = std::fs::canonicalize(&pointed).unwrap_or(pointed);
    let hits = dirs
        .iter()
        .any(|dir| std::fs::canonicalize(dir).unwrap_or_else(|_| dir.clone()) == pointed);
    if hits {
        let _ = std::fs::remove_file(&link).or_else(|_| std::fs::remove_dir(&link));
    }
}

/// `engineStrict` against the patched manifest. The earlier installability
/// pass skipped engines for patched packages because the published manifest
/// was still the only one on record.
/// `Some` is the skip details for an optional package. `None` means continue.
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
