use super::planning::CandidateGroup;
use crate::{SideEffectsBySnapshot, SideEffectsMapsBySnapshot};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use pnpm_lockfile::PackageKey;
use pnpm_pnpr_client::{ArtifactCandidate, ArtifactManifest, RejectedArtifact, blob_id};
use pnpm_shared_artifact_protocol::compatibility_rank;
use pnpm_store_dir::{SideEffectsDiff, StoreIndexWriter};
use sha2::{Digest as _, Sha512};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

pub(super) fn take_persisted_remote_side_effects(
    side_effects_maps_by_snapshot: &mut SideEffectsMapsBySnapshot,
    side_effects_by_snapshot: &SideEffectsBySnapshot,
) -> HashMap<(PackageKey, String), HashMap<String, PathBuf>> {
    let mut persisted = HashMap::new();
    for (snapshot_key, diffs) in side_effects_by_snapshot {
        let remote_keys: Vec<&String> = diffs
            .iter()
            .filter_map(|(cache_key, diff)| diff.remote_origin.as_ref().map(|_| cache_key))
            .collect();
        if remote_keys.is_empty() {
            continue;
        }
        let Some(existing) = side_effects_maps_by_snapshot.get(snapshot_key) else { continue };
        let mut maps = (**existing).clone();
        take_snapshot_overlays(&mut maps, remote_keys, snapshot_key, &mut persisted);
        if maps.is_empty() {
            side_effects_maps_by_snapshot.remove(snapshot_key);
        } else {
            side_effects_maps_by_snapshot.insert(snapshot_key.clone(), Arc::new(maps));
        }
    }
    persisted
}
/// Move one snapshot's remote overlays out of its live map and into the
/// persisted set, keyed by (snapshot, cache key).
pub(super) fn take_snapshot_overlays(
    maps: &mut HashMap<String, HashMap<String, PathBuf>>,
    remote_keys: Vec<&String>,
    snapshot_key: &PackageKey,
    persisted: &mut HashMap<(PackageKey, String), HashMap<String, PathBuf>>,
) {
    for cache_key in remote_keys {
        if let Some(overlay) = maps.remove(cache_key) {
            persisted.insert((snapshot_key.clone(), cache_key.clone()), overlay);
        }
    }
}
pub(super) fn insert_side_effects_map(
    side_effects_maps_by_snapshot: &mut SideEffectsMapsBySnapshot,
    snapshot_key: PackageKey,
    cache_key: String,
    overlay: HashMap<String, PathBuf>,
) {
    let mut maps = side_effects_maps_by_snapshot
        .get(&snapshot_key)
        .map_or_else(HashMap::new, |maps| (**maps).clone());
    maps.insert(cache_key, overlay);
    side_effects_maps_by_snapshot.insert(snapshot_key, Arc::new(maps));
}
pub(super) fn stored_remote_side_effects_are_verified(
    diff: &SideEffectsDiff,
    candidate: &ArtifactCandidate,
    configured_channel: Option<&str>,
    supported_tags: &[String],
    trusted_keys: &BTreeMap<String, Vec<u8>>,
) -> bool {
    let Some(origin) = &diff.remote_origin else { return false };
    if origin.verification != "verified"
        || origin.signer_key_id != origin.envelope.key_id
        || configured_channel.is_some_and(|channel| origin.channel != channel)
    {
        return false;
    }
    let Some(public_key) = trusted_keys.get(&origin.signer_key_id) else { return false };
    let Ok(payload) = origin.envelope.verify(public_key) else { return false };
    if payload.input_key != candidate.key {
        return false;
    }
    payload.subject == candidate.subject
        && payload.owner == candidate.owner
        && payload.owner == origin.owner
        && payload.builder_profile == origin.builder_profile
        && compatibility_rank(&payload.compatibility, supported_tags).is_some()
        && manifest_matches_diff(&payload.manifest, diff)
}
pub(super) fn manifest_matches_diff(manifest: &ArtifactManifest, diff: &SideEffectsDiff) -> bool {
    let empty = HashMap::new();
    let added = diff.added.as_ref().unwrap_or(&empty);
    if added.len() != manifest.added.len() {
        return false;
    }
    for file in &manifest.added {
        let Ok(digest) = blob_id(&file.integrity) else { return false };
        if !added.get(&file.path).is_some_and(|stored| {
            stored.digest == digest && stored.mode == file.mode && stored.size == file.size
        }) {
            return false;
        }
    }
    let deleted = diff.deleted.as_deref().unwrap_or_default();
    deleted.len() == manifest.deleted.len()
        && deleted.iter().collect::<HashSet<_>>().len() == deleted.len()
        && deleted.iter().all(|path| manifest.deleted.contains(path))
}
pub(super) async fn stored_remote_side_effects_blobs_are_valid(
    diff: &SideEffectsDiff,
    overlay: &HashMap<String, PathBuf>,
) -> Result<bool, String> {
    for (file_path, info) in diff.added.iter().flatten() {
        let Some(path) = overlay.get(file_path) else { return Ok(false) };
        if !store_holds(path, &info.digest).await? {
            return Ok(false);
        }
        let metadata = tokio::fs::metadata(path)
            .await
            .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
        if metadata.len() != info.size {
            return Ok(false);
        }
    }
    Ok(true)
}
pub(super) fn quarantine_remote_side_effects(
    rejected: &RejectedArtifact,
    groups: &BTreeMap<String, CandidateGroup>,
    channel: &str,
    store_index_writer: &StoreIndexWriter,
) {
    let Some(group) = groups.get(&rejected.input_key) else { return };
    let mut rows = HashSet::new();
    for (_, _, store_index_key) in &group.snapshots {
        if rows.insert(store_index_key) {
            store_index_writer.queue_remote_side_effects_quarantine(
                store_index_key.clone(),
                channel.to_string(),
                rejected.envelope_digest.clone(),
            );
        }
    }
    tracing::warn!(
        target: "pacquet::install",
        reason = %rejected.reason,
        "remote side-effects artifact was quarantined",
    );
}
/// Decode the configured trust root, or `None` when it is absent or unusable.
///
/// A key pnpm cannot decode is a configuration mistake that would silently
/// narrow what the install trusts, so the whole lookup is abandoned rather than
/// run against a partial key set.
pub(super) fn decoded_trusted_keys(
    settings: &pnpm_config::RemoteSideEffectsCacheSettings,
) -> Option<BTreeMap<String, Vec<u8>>> {
    let encoded = settings.trusted_keys.as_ref().filter(|keys| !keys.is_empty())?;
    let mut trusted_keys = BTreeMap::new();
    for (key_id, public_key) in encoded {
        let public_key = match BASE64.decode(public_key) {
            Ok(public_key) => public_key,
            Err(error) => {
                tracing::warn!(
                    target: "pacquet::install",
                    key_id,
                    %error,
                    "remote side-effects public key is not valid base64",
                );
                return None;
            }
        };
        trusted_keys.insert(key_id.clone(), public_key);
    }
    Some(trusted_keys)
}
/// Reads the store in chunks this size while hashing, so a large CAS blob
/// is never held in memory whole.
pub(super) const STORE_READ_CHUNK: usize = 64 * 1024;
/// Whether the store already holds `digest` at `path`.
///
/// Verified unconditionally rather than answering to `verifyStoreIntegrity`:
/// the download this skips would have ended in a CAS write, and that path
/// checks content already at the destination whatever the setting says.
/// Hashing a local file is far cheaper than the transfer it avoids.
///
/// A missing file is an ordinary miss; any other failure is reported rather
/// than quietly redownloaded.
pub(super) async fn store_holds(path: &Path, digest: &str) -> Result<bool, String> {
    use tokio::io::AsyncReadExt as _;

    let mut options = tokio::fs::OpenOptions::new();
    options.read(true);
    // The store addresses its own regular files. A symlink at the digest path
    // would name bytes the store neither owns nor can keep from changing, and
    // a plain open on a FIFO would block until a writer appeared. Refusing
    // both at open binds the check to the file that is actually read, which a
    // preceding `symlink_metadata` could not.
    #[cfg(unix)]
    options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
    // The Windows spelling of the same refusal: open the reparse point itself
    // rather than what it redirects to, so the descriptor check below sees a
    // reparse point instead of the file it names.
    #[cfg(windows)]
    options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    // Whatever turned the open away — absent, a directory, a symlink
    // `O_NOFOLLOW` refused, a permission error — names something the caller
    // cannot reuse, and its fallback is a verified download that reports any
    // real fault itself. A failure once the file is open is different: that
    // one is reported below, since the store handed over a file it then could
    // not read.
    let Ok(file) = options.open(path).await else {
        return Ok(false);
    };
    if !file.metadata().await.is_ok_and(|metadata| metadata.is_file()) {
        return Ok(false);
    }
    let mut reader = tokio::io::BufReader::with_capacity(STORE_READ_CHUNK, file);
    let mut hasher = Sha512::new();
    let mut buffer = vec![0u8; STORE_READ_CHUNK];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) => break,
            Ok(read) => hasher.update(&buffer[..read]),
            Err(ref error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(format!("failed to read {}: {error}", path.display())),
        }
    }
    Ok(format!("{:x}", hasher.finalize()) == digest)
}
