use super::{ResolvedArtifactContext, store_holds};
use pnpm_config::Config;
use pnpm_pnpr_client::{ArtifactBlobRequest, ArtifactFile, PnprClientError, blob_id};
use pnpm_store_dir::CafsFileInfo;
use std::{collections::HashMap, path::PathBuf};

/// The store path and CAFS record for one of the artifact's added
/// files: a blob this artifact already staged, one the store already
/// holds, or one downloaded now. The error's flag says whether the
/// failure is the artifact's fault, and so quarantines it.
pub(in super::super) async fn stage_artifact_blob(
    context: &ResolvedArtifactContext<'_>,
    artifact: &pnpm_pnpr_client::VerifiedArtifact,
    file: &ArtifactFile,
    stored: &mut HashMap<(String, u32), PathBuf>,
    downloaded: &mut HashMap<String, Vec<u8>>,
) -> Result<(PathBuf, CafsFileInfo), (String, bool)> {
    let storage_key = (file.integrity.clone(), file.mode);
    let digest = blob_id(&file.integrity).map_err(|error| (error.to_string(), true))?;
    let info = |digest: String| CafsFileInfo {
        digest,
        mode: file.mode,
        size: file.size,
        checked_at: None,
    };
    if let Some(path) = stored.get(&storage_key) {
        return Ok((path.clone(), info(digest)));
    }
    if !downloaded.contains_key(&file.integrity) {
        if let Some(path) = stored_blob_path(context.config, file, &digest)
            .await?
        {
            stored.insert(storage_key, path.clone());
            return Ok((path, info(digest)));
        }
        let bytes = download_artifact_file(context, artifact, file).await?;
        downloaded.insert(file.integrity.clone(), bytes);
    }
    let (path, _) = context.config.store_dir
        .write_cas_file(
            &downloaded[&file.integrity],
            pnpm_fs::file_mode::is_executable(file.mode),
        )
        .map_err(|error| (error.to_string(), false))?;
    stored.insert(storage_key, path.clone());
    Ok((path, info(digest)))
}

pub(in super::super) async fn download_artifact_file(
    context: &ResolvedArtifactContext<'_>,
    artifact: &pnpm_pnpr_client::VerifiedArtifact,
    file: &ArtifactFile,
) -> Result<Vec<u8>, (String, bool)> {
    let bytes = context.client
        .download_artifact_blob(
            &ArtifactBlobRequest {
                owner: artifact.payload.owner.clone(),
                integrity: file.integrity.clone(),
            },
            context.authorization,
        )
        .await
        .map_err(|error| {
            let quarantine = matches!(error, PnprClientError::Protocol(_));
            (error.to_string(), quarantine)
        })?;
    if bytes.len() as u64 != file.size {
        return Err((
            "shared artifact blob does not match its declared size".to_string(),
            true,
        ));
    }
    Ok(bytes)
}

/// The store's own copy of a blob, when it holds one.
///
/// A built package's files are mostly its own, and artifacts share
/// files with each other. The store addresses content by the digest the
/// manifest entry already carries, so anything it holds is the same
/// bytes and needs no transfer.
///
/// Both this lookup and the write in [`stage_artifact_blob`] address the
/// store by `is_executable`, so they cannot disagree about where a mode
/// belongs. The manifest only carries 0o644 and 0o755 today, but the
/// agreement must not rest on that.
pub(in super::super) async fn stored_blob_path(
    config: &Config,
    file: &ArtifactFile,
    digest: &str,
) -> Result<Option<PathBuf>, (String, bool)> {
    let Some(path) = config.store_dir.cas_file_path_by_mode(digest, file.mode) else {
        return Ok(None);
    };
    if !store_holds(&path, digest).await.map_err(|error| (error, false))? {
        return Ok(None);
    }
    if !tokio::fs::metadata(&path).await
        .is_ok_and(|metadata| metadata.len() == file.size)
    {
        return Err((
            "stored shared artifact blob does not match its declared size".to_string(),
            true,
        ));
    }
    Ok(Some(path))
}
