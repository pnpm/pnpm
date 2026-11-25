use pnpm_lockfile::{
    BinaryResolution,
    PackageKey,
};
use pnpm_store_dir::{
    PackageFilesIndex,
    StoreIndex,
};
use pnpm_tarball::{
    ArchiveStoreContext,
    TarballError,
};
use sha2::{
    Digest,
    Sha512,
};

/// Upgrade a cached archive while retaining the row older pnpm versions read.
pub(super) fn upgrade(
    store: &ArchiveStoreContext<'_>,
    key: &PackageKey,
    binary: &BinaryResolution,
    manifest: &[u8],
) -> Result<Option<pnpm_fs::CasPaths>, TarballError> {
    let Some(index) = &store.index else { return Ok(None) };
    let Some(previous) = manifest_without_license(manifest) else { return Ok(None) };
    let integrity = binary.integrity.to_string();
    let package_id = key.pkg_id();
    let previous_key = super::manifest_index_key(&previous, &integrity, &package_id);
    let plain_key = pnpm_store_dir::store_index_key(&integrity, &package_id);
    for candidate in [&previous_key, &plain_key] {
        let Some(row) = StoreIndex::get_shared(index, candidate)
            .inspect_err(|error| tracing::debug!(%error, "runtime cache row unavailable"))
            .ok()
            .flatten()
        else {
            continue;
        };
        let generated = row.files
            .get("package.json")
            .is_some_and(|file| {
                file.mode == 0o644 && file.digest == format!("{:x}", Sha512::digest(&previous))
            });
        if !generated {
            continue;
        }
        if pnpm_store_dir::pkg_content_mismatch(row.manifest.as_ref(), candidate).is_some() {
            continue;
        }
        return upgrade_row(
            store,
            row,
            manifest,
            super::manifest_index_key(manifest, &integrity, &package_id),
        );
    }
    Ok(None)
}

fn manifest_without_license(manifest: &[u8]) -> Option<Vec<u8>> {
    let mut value: serde_json::Value =
        serde_json::from_slice(manifest).expect("serialized runtime manifest");
    value.as_object_mut()?.remove("license")?;
    Some(serde_json::to_vec(&value).expect("serialize runtime manifest"))
}

fn upgrade_row(
    store: &ArchiveStoreContext<'_>,
    mut row: PackageFilesIndex,
    manifest: &[u8],
    target: String,
) -> Result<Option<pnpm_fs::CasPaths>, TarballError> {
    let files = PackageFilesIndex {
        algo: row.algo.clone(),
        files: row.files.clone(),
        ..Default::default()
    };
    let mut verified = if store.verify_integrity {
        pnpm_store_dir::check_pkg_files_integrity(&store.dir, files, &store.verified_files_cache)
    } else {
        pnpm_store_dir::build_file_maps_from_index(&store.dir, files)
    };
    if !verified.passed {
        return Ok(None);
    }
    let Some(writer) = &store.index_writer else { return Ok(Some(verified.files_map)) };
    row.side_effects = None;
    row.remote_side_effects_quarantine = None;
    let parsed = serde_json::from_slice(manifest).expect("serialized runtime manifest");
    row.manifest = pnpm_package_manifest::normalize_bundled_manifest(&parsed);
    let written = store.dir.write_cas_file(manifest, false).map_err(TarballError::WriteCasFile)?;
    row.files.insert("package.json".into(), written.index_entry(0o644, None));
    verified.files_map.insert("package.json".into(), written.file);
    writer.queue(target, row);
    Ok(Some(verified.files_map))
}
