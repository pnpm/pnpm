use super::{
    ApplySharedSideEffectsOptions, BaseCasPaths, artifact_platform, decoded_trusted_keys,
    dependency_package, insert_side_effects_map, non_empty, planning::CandidateGroup,
    quarantine_remote_side_effects, store_holds,
};
use crate::{RemoteSideEffectsQuarantineBySnapshot, SideEffectsMapsBySnapshot};
use pnpm_config::Config;
use pnpm_lockfile::{PackageKey, SnapshotEntry};
use pnpm_pnpr_client::{
    ArtifactBlobRequest, ArtifactFile, OwnerScope, PnprClient, PnprClientError, RejectedArtifact,
    ResolveArtifactsOptions, blob_id,
};
use pnpm_store_dir::{CafsFileInfo, RemoteSideEffectsOrigin, SideEffectsDiff, StoreIndexWriter};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

/// Resolve the groups' artifacts on the configured pnpr server,
/// quarantine the rejected ones and overlay the rest.
pub(super) async fn fetch_remote_artifacts(
    options: &mut ApplySharedSideEffectsOptions<'_>,
    setup: &RemoteCacheSetup,
    groups: &BTreeMap<String, CandidateGroup>,
) {
    let config = options.config;
    let Some(server) = config.pnpr_server.as_deref() else { return };
    let client = PnprClient::new(server);
    let authorization = config.auth_headers.for_url(server);
    let Some((resolved, rejected_artifacts)) = resolve_remote_artifacts(
        &client,
        setup,
        groups,
        options.remote_side_effects_quarantine_by_snapshot,
        server,
        authorization.as_deref(),
    )
    .await
    else {
        return;
    };
    for rejected in rejected_artifacts {
        quarantine_remote_side_effects(&rejected, groups, server, options.store_index_writer);
    }

    for (input_key, artifact) in resolved {
        apply_resolved_artifact(
            &ResolvedArtifactContext {
                config,
                client: &client,
                server,
                authorization: authorization.as_deref(),
                groups,
                base_cas_paths: options.base_cas_paths,
                store_index_writer: options.store_index_writer,
            },
            &input_key,
            &artifact,
            options.side_effects_maps_by_snapshot,
        )
        .await;
    }
}
/// The remote side-effects configuration this install can use, or
/// `None` when the cache is off, misconfigured, or the host platform is
/// not one the shared-artifact protocol describes.
pub(super) struct RemoteCacheSetup {
    pub(super) supported_tags: Vec<String>,
    pub(super) trusted_keys: BTreeMap<String, Vec<u8>>,
    pub(super) owner: OwnerScope,
    pub(super) eligible_packages: HashSet<String>,
    pub(super) node_major: u32,
}
pub(super) fn remote_cache_setup(
    config: &Config,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
) -> Option<RemoteCacheSetup> {
    if config.ignore_scripts {
        return None;
    }
    let settings = config.remote_side_effects_cache.as_ref()?;
    let platform = artifact_platform(snapshots)?;
    let supported_tags = match platform.supported_tags() {
        Ok(tags) => tags,
        Err(error) => {
            tracing::warn!(target: "pacquet::install", %error, "remote side-effects platform is unsupported");
            return None;
        }
    };
    let trusted_keys = decoded_trusted_keys(settings)?;
    let organization = non_empty(&settings.org)?;
    Some(RemoteCacheSetup {
        supported_tags,
        trusted_keys,
        owner: OwnerScope::organization(organization.to_string()),
        eligible_packages: settings.packages.iter().cloned().collect(),
        node_major: platform.node_major(),
    })
}
/// Ask the remote cache which of the planned groups it can supply.
/// `None` when the lookup could not run: a failed handshake or query is
/// a cache miss, never an install failure.
pub(super) async fn resolve_remote_artifacts(
    client: &PnprClient,
    setup: &RemoteCacheSetup,
    groups: &BTreeMap<String, CandidateGroup>,
    remote_side_effects_quarantine_by_snapshot: &RemoteSideEffectsQuarantineBySnapshot,
    server: &str,
    authorization: Option<&str>,
) -> Option<(BTreeMap<String, pnpm_pnpr_client::VerifiedArtifact>, Vec<RejectedArtifact>)> {
    tracing::debug!(
        target: "pacquet::install",
        candidates = groups.len(),
        "querying remote side-effects cache",
    );
    if let Err(error) = client.handshake_artifacts().await {
        tracing::warn!(target: "pacquet::install", %error, "remote side-effects cache handshake failed");
        return None;
    }
    let allowed_builds =
        groups.values().map(|group| dependency_package(&group.candidate).name.clone()).collect();
    let quarantined_envelope_digests =
        quarantined_digests(groups, remote_side_effects_quarantine_by_snapshot, server);
    let rejected_artifacts = Arc::new(std::sync::Mutex::new(Vec::new()));
    let rejected_artifacts_for_callback = Arc::clone(&rejected_artifacts);
    let resolved = match client
        .resolve_artifacts(ResolveArtifactsOptions {
            candidates: groups.values().map(|group| group.candidate.clone()).collect(),
            supported_tags: setup.supported_tags.clone(),
            eligible_packages: setup.eligible_packages.clone(),
            allowed_builds,
            ignore_scripts: false,
            trusted_keys: setup.trusted_keys.clone(),
            quarantined_envelope_digests,
            on_rejected_artifact: Some(Arc::new(move |rejected| {
                rejected_artifacts_for_callback.lock().unwrap().push(rejected);
            })),
            authorization: authorization.map(str::to_owned),
        })
        .await
    {
        Ok(resolved) => resolved,
        Err(error) => {
            tracing::warn!(target: "pacquet::install", %error, "remote side-effects cache lookup failed");
            return None;
        }
    };
    let rejected_artifacts = std::mem::take(&mut *rejected_artifacts.lock().unwrap());
    Some((resolved, rejected_artifacts))
}
pub(super) fn quarantined_digests(
    groups: &BTreeMap<String, CandidateGroup>,
    remote_side_effects_quarantine_by_snapshot: &RemoteSideEffectsQuarantineBySnapshot,
    server: &str,
) -> BTreeMap<String, HashSet<String>> {
    groups
        .iter()
        .map(|(input_key, group)| {
            let quarantined = group.snapshots.iter().filter_map(|(snapshot_key, _, _)| {
                remote_side_effects_quarantine_by_snapshot
                    .get(snapshot_key)
                    .and_then(|channels| channels.get(server))
            });
            let digests = quarantined.flatten().cloned().collect();
            (input_key.clone(), digests)
        })
        .collect()
}
/// What one resolved artifact needs to be staged into the store.
pub(super) struct ResolvedArtifactContext<'a> {
    config: &'a Config,
    client: &'a PnprClient,
    server: &'a str,
    authorization: Option<&'a str>,
    groups: &'a BTreeMap<String, CandidateGroup>,
    base_cas_paths: &'a BaseCasPaths,
    store_index_writer: &'a Arc<StoreIndexWriter>,
}
/// Overlay one resolved artifact's file map onto the group's base file
/// map and record the result for every snapshot in the group. A
/// rejected artifact leaves the group unbuilt, so the local build runs
/// as it would without the cache.
pub(super) async fn apply_resolved_artifact(
    context: &ResolvedArtifactContext<'_>,
    input_key: &str,
    artifact: &pnpm_pnpr_client::VerifiedArtifact,
    side_effects_maps_by_snapshot: &mut SideEffectsMapsBySnapshot,
) {
    let Some(group) = context.groups.get(input_key) else { return };
    // See `SideEffectsDiff::is_empty`: an artifact with nothing to restore
    // must not stand in for the build.
    if artifact.payload.manifest.is_empty() {
        tracing::debug!(
            target: "pacquet::install",
            input_key,
            "remote side-effects artifact restores nothing; building locally instead",
        );
        return;
    }
    let Some((first_snapshot, _, _)) = group.snapshots.first() else { return };
    let Some(base) = context.base_cas_paths.get(first_snapshot) else { return };
    let staged = match stage_artifact(context, artifact, base).await {
        Ok(staged) => staged,
        Err((error, quarantine)) => {
            report_rejected_artifact(context, input_key, artifact, group, &error, quarantine);
            return;
        }
    };
    let diff = remote_diff(context, artifact, staged.added);
    record_group(context, group, &staged.overlay, &diff, side_effects_maps_by_snapshot);
}
/// The artifact's file map over the group's base, with every added
/// file staged in the CAFS.
pub(super) struct StagedArtifact {
    overlay: HashMap<String, PathBuf>,
    added: HashMap<String, CafsFileInfo>,
}
pub(super) async fn stage_artifact(
    context: &ResolvedArtifactContext<'_>,
    artifact: &pnpm_pnpr_client::VerifiedArtifact,
    base: &HashMap<String, PathBuf>,
) -> Result<StagedArtifact, (String, bool)> {
    let mut overlay = base.clone();
    let mut downloaded = HashMap::<String, Vec<u8>>::new();
    let mut stored = HashMap::<(String, u32), PathBuf>::new();
    let mut added = HashMap::<String, CafsFileInfo>::new();
    for deleted in &artifact.payload.manifest.deleted {
        overlay.remove(deleted);
    }
    for file in &artifact.payload.manifest.added {
        let (path, info) =
            stage_artifact_blob(context, artifact, file, &mut stored, &mut downloaded).await?;
        overlay.insert(file.path.clone(), path);
        added.insert(file.path.clone(), info);
    }
    Ok(StagedArtifact { overlay, added })
}
pub(super) fn remote_diff(
    context: &ResolvedArtifactContext<'_>,
    artifact: &pnpm_pnpr_client::VerifiedArtifact,
    added: HashMap<String, CafsFileInfo>,
) -> SideEffectsDiff {
    SideEffectsDiff {
        added: Some(added),
        deleted: Some(artifact.payload.manifest.deleted.clone()),
        remote_origin: Some(RemoteSideEffectsOrigin {
            channel: context.server.to_string(),
            owner: artifact.payload.owner.clone(),
            signer_key_id: artifact.envelope.key_id.clone(),
            builder_profile: artifact.payload.builder_profile.clone(),
            envelope: artifact.envelope.clone(),
            verification: "verified".to_string(),
        }),
    }
}
/// Record the overlay for every snapshot in the group, locally and in
/// the store index.
pub(super) fn record_group(
    context: &ResolvedArtifactContext<'_>,
    group: &CandidateGroup,
    overlay: &HashMap<String, PathBuf>,
    diff: &SideEffectsDiff,
    side_effects_maps_by_snapshot: &mut SideEffectsMapsBySnapshot,
) {
    for (snapshot_key, local_cache_key, store_index_key) in &group.snapshots {
        insert_side_effects_map(
            side_effects_maps_by_snapshot,
            snapshot_key.clone(),
            local_cache_key.clone(),
            overlay.clone(),
        );
        context.store_index_writer.queue_remote_side_effects(
            store_index_key.clone(),
            local_cache_key.clone(),
            diff.clone(),
        );
    }
}
pub(super) fn report_rejected_artifact(
    context: &ResolvedArtifactContext<'_>,
    input_key: &str,
    artifact: &pnpm_pnpr_client::VerifiedArtifact,
    group: &CandidateGroup,
    error: &str,
    quarantine: bool,
) {
    if quarantine {
        quarantine_remote_side_effects(
            &RejectedArtifact {
                input_key: input_key.to_owned(),
                envelope_digest: artifact.envelope_digest.clone(),
                reason: error.to_owned(),
            },
            context.groups,
            context.server,
            context.store_index_writer,
        );
    }
    tracing::warn!(
        target: "pacquet::install",
        package = %dependency_package(&group.candidate).name,
        %error,
        "remote side-effects artifact was rejected",
    );
}
/// The store path and CAFS record for one of the artifact's added
/// files: a blob this artifact already staged, one the store already
/// holds, or one downloaded now. The error's flag says whether the
/// failure is the artifact's fault, and so quarantines it.
pub(super) async fn stage_artifact_blob(
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
        if let Some(path) = stored_blob_path(context.config, file, &digest).await? {
            stored.insert(storage_key, path.clone());
            return Ok((path, info(digest)));
        }
        let bytes = download_artifact_file(context, artifact, file).await?;
        downloaded.insert(file.integrity.clone(), bytes);
    }
    let (path, _) = context
        .config
        .store_dir
        .write_cas_file(&downloaded[&file.integrity], pnpm_fs::file_mode::is_executable(file.mode))
        .map_err(|error| (error.to_string(), false))?;
    stored.insert(storage_key, path.clone());
    Ok((path, info(digest)))
}
pub(super) async fn download_artifact_file(
    context: &ResolvedArtifactContext<'_>,
    artifact: &pnpm_pnpr_client::VerifiedArtifact,
    file: &ArtifactFile,
) -> Result<Vec<u8>, (String, bool)> {
    let bytes = context
        .client
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
        return Err(("shared artifact blob does not match its declared size".to_string(), true));
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
pub(super) async fn stored_blob_path(
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
    if !tokio::fs::metadata(&path).await.is_ok_and(|metadata| metadata.len() == file.size) {
        return Err((
            "stored shared artifact blob does not match its declared size".to_string(),
            true,
        ));
    }
    Ok(Some(path))
}
