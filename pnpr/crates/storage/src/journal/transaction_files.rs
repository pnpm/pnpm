use super::{
    AsyncWriteExt, COMMIT_MARKER, JournaledPublish, MANIFEST_FILE, Manifest, ManifestBlob,
    ManifestPackage, Ordering, Path, RegistryError, Result, SystemTime, TXN_COUNTER, UNIX_EPOCH,
    fs, io, is_canonical_revision_ref_owner, unique_tmp_path,
};

/// Write the transaction's documents and manifest and seal them with the
/// commit marker, the single atomic rename that commits the publish.
pub(super) async fn write_transaction(dir: &Path, packages: &[JournaledPublish<'_>]) -> Result<()> {
    fs::create_dir_all(dir).await?;
    let mut manifest = Manifest { packages: Vec::with_capacity(packages.len()) };
    for (index, package) in packages.iter().enumerate() {
        let document_file = format!("document-{index}.json");
        write_synced(&dir.join(&document_file), package.document).await?;
        manifest.packages.push(ManifestPackage {
            name: package.name.as_str().to_string(),
            ecosystem: package.ecosystem,
            org: package.org.map(str::to_string),
            document_file,
            blobs: package
                .slots
                .iter()
                .map(|slot| ManifestBlob {
                    filename: slot.filename().to_string(),
                    tmp_path: slot.tmp_path.clone(),
                })
                .collect(),
            revision_refs: package.revision_refs.to_vec(),
        });
    }
    write_synced(&dir.join(MANIFEST_FILE), &serde_json::to_vec_pretty(&manifest)?).await?;
    let _ = sync_dir(dir).await;
    // The seal itself: a single same-directory rename, atomic on
    // POSIX. Recovery treats a directory without this marker as an
    // aborted transaction and rolls it back.
    let marker = dir.join(COMMIT_MARKER);
    let marker_tmp = unique_tmp_path(&marker);
    write_synced(&marker_tmp, b"").await?;
    fs::rename(&marker_tmp, &marker).await?;
    let _ = sync_dir(dir).await;
    Ok(())
}

pub(super) async fn cleanup_lost_tmp_paths(tmp_paths: &[&Path], journal_removal_is_durable: bool) {
    if !journal_removal_is_durable {
        return;
    }
    for tmp_path in tmp_paths {
        let _ = fs::remove_file(tmp_path).await;
    }
}

/// Discard an unsealed transaction: nothing of it ever became visible,
/// so all there is to do is delete the staged tmp files it points at
/// and the journal entry itself. Errors are swallowed — this is
/// cleanup, and a leftover tmp file is harmless beyond a little disk.
pub(super) async fn roll_back(dir: &Path) {
    if let Ok(bytes) = fs::read(dir.join(MANIFEST_FILE)).await
        && let Ok(manifest) = serde_json::from_slice::<Manifest>(&bytes)
    {
        for package in &manifest.packages {
            for blob in &package.blobs {
                let _ = fs::remove_file(&blob.tmp_path).await;
            }
        }
    }
    let _ = fs::remove_dir_all(dir).await;
}

/// `<zero-padded unix millis>-<pid>-<counter>`: unique per process and
/// lexically ordered by seal time across restarts.
pub(super) fn txn_id() -> String {
    let millis =
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
    let counter = TXN_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{millis:016}-{}-{counter}", std::process::id())
}

pub(super) fn revision_ref_owner(dir: &Path) -> Result<&str> {
    let owner =
        dir.file_name().and_then(|name| name.to_str()).ok_or_else(|| RegistryError::Internal {
            reason: format!("publish journal path has no transaction id: {}", dir.display()),
        })?;
    if is_canonical_revision_ref_owner(owner) {
        Ok(owner)
    } else {
        Err(RegistryError::Internal {
            reason: format!("publish journal transaction id is invalid: {}", dir.display()),
        })
    }
}

pub(super) async fn write_synced(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::File::create(path).await?;
    file.write_all(bytes).await?;
    file.sync_all().await?;
    Ok(())
}

#[cfg(unix)]
pub(super) async fn sync_dir(dir: &Path) -> io::Result<()> {
    fs::File::open(dir).await?.sync_all().await
}

#[cfg(not(unix))]
pub(super) async fn sync_dir(_dir: &Path) -> io::Result<()> {
    // 표준 API로 디렉터리 엔트리의 내구성을 확인할 수 없는 플랫폼은 안전하게 미지원 처리한다.
    Err(io::Error::new(io::ErrorKind::Unsupported, "directory sync is not supported"))
}
