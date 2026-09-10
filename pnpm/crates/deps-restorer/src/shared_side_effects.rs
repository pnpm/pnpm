mod platform;
use platform::{
    ArtifactPlatform, artifact_platform, digest_integrity, package_version, patch_hash,
};

mod persisted;
use persisted::{
    decoded_trusted_keys, insert_side_effects_map, quarantine_remote_side_effects, store_holds,
    stored_remote_side_effects_are_verified, stored_remote_side_effects_blobs_are_valid,
    take_persisted_remote_side_effects,
};

mod remote;
use remote::{RemoteCacheSetup, fetch_remote_artifacts, remote_cache_setup};

mod planning;
use planning::{CandidatePlan, plan_candidate_groups, plan_eligible_roots};

use crate::{
    AllowBuildPolicy, RemoteSideEffectsQuarantineBySnapshot, RequiresBuildBySnapshot,
    SideEffectsBySnapshot, SideEffectsMapsBySnapshot, StoreIndexKeysBySnapshot,
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use pnpm_config::Config;
use pnpm_lockfile::{PackageKey, PackageMetadata, SnapshotEntry};
use pnpm_pnpr_client::{
    ARTIFACT_KIND, ArtifactBlobUpload, ArtifactCandidate, ArtifactFile, ArtifactManifest,
    ArtifactPayload, ArtifactSubject, BuilderProfile, CompatibilityConstraints, OwnerScope,
    PackageIdentity, PnprClient, PublishArtifactRequest, SignedArtifactEnvelope,
};
use pnpm_store_dir::{CafsFileInfo, StoreIndexWriter};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

pub(crate) type BaseCasPaths = HashMap<PackageKey, HashMap<String, PathBuf>>;

pub struct SharedSideEffectsPublisher {
    authorization: Option<String>,
    builder_id: String,
    builder_profile: BuilderProfile,
    client: PnprClient,
    key_id: String,
    organization: String,
    packages: HashSet<String>,
    platform: ArtifactPlatform<'static>,
    private_key: Vec<u8>,
    runtime: tokio::runtime::Handle,
}

pub(crate) struct ApplySharedSideEffectsOptions<'a> {
    pub config: &'a Config,
    pub snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
    pub packages: &'a HashMap<PackageKey, PackageMetadata>,
    pub requires_build_by_snapshot: &'a RequiresBuildBySnapshot,
    pub allow_build_policy: &'a AllowBuildPolicy,
    pub base_cas_paths: &'a BaseCasPaths,
    pub side_effects_maps_by_snapshot: &'a mut SideEffectsMapsBySnapshot,
    pub side_effects_by_snapshot: &'a SideEffectsBySnapshot,
    pub remote_side_effects_quarantine_by_snapshot: &'a RemoteSideEffectsQuarantineBySnapshot,
    pub store_index_keys_by_snapshot: &'a StoreIndexKeysBySnapshot,
    pub store_index_writer: &'a Arc<StoreIndexWriter>,
}

pub(crate) async fn apply_shared_side_effects(mut options: ApplySharedSideEffectsOptions<'_>) {
    let persisted_remote = take_persisted_remote_side_effects(
        options.side_effects_maps_by_snapshot,
        options.side_effects_by_snapshot,
    );
    if !options.config.side_effects_cache_read() {
        options.side_effects_maps_by_snapshot.clear();
    }
    let Some(setup) = remote_cache_setup(options.config, options.snapshots) else { return };

    let roots = plan_eligible_roots(&options, &setup);
    if roots.is_empty() {
        return;
    }

    let groups = plan_candidate_groups(
        &CandidatePlan {
            config: options.config,
            snapshots: options.snapshots,
            packages: options.packages,
            setup: &setup,
            side_effects_by_snapshot: options.side_effects_by_snapshot,
            store_index_keys_by_snapshot: options.store_index_keys_by_snapshot,
        },
        roots,
        persisted_remote,
        options.side_effects_maps_by_snapshot,
    )
    .await;
    if groups.is_empty() || options.config.frozen_store {
        return;
    }
    fetch_remote_artifacts(&mut options, &setup, &groups).await;
}

fn non_empty(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}

pub(crate) fn shared_side_effects_publisher(
    config: &Config,
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
) -> Option<SharedSideEffectsPublisher> {
    let server = config.pnpr_server.as_deref()?;
    let settings = config.remote_side_effects_cache.as_ref()?;
    if settings.publish != Some(true) {
        return None;
    }
    let snapshots = snapshots?;
    let platform = artifact_platform(snapshots)?;
    let private_key = BASE64.decode(settings.private_key.as_ref()?).ok()?;
    let key_id = settings.key_id.clone()?;
    let builder_id = settings.builder_id.clone()?;
    let organization = non_empty(&settings.org)?.to_string();
    let environment = settings.build_env.clone().unwrap_or_default();
    Some(SharedSideEffectsPublisher {
        authorization: config.auth_headers.for_url(server),
        builder_id,
        builder_profile: BuilderProfile {
            image_digest: settings.image_digest.clone(),
            architecture_baseline: settings
                .architecture_baseline
                .clone()
                .unwrap_or_else(|| pnpm_graph_hasher::host_arch().to_string()),
            environment,
        },
        client: PnprClient::new(server),
        key_id,
        organization,
        packages: settings.packages.iter().cloned().collect(),
        platform,
        private_key,
        runtime: tokio::runtime::Handle::current(),
    })
}

impl SharedSideEffectsPublisher {
    pub(crate) fn can_publish(
        &self,
        metadata_key: &PackageKey,
        metadata: &PackageMetadata,
    ) -> bool {
        self.packages.contains(&metadata_key.name.to_string())
            && metadata.resolution.checkable_integrity().is_some()
    }

    pub(crate) fn publish(
        &self,
        snapshot_key: &PackageKey,
        metadata: &PackageMetadata,
        graph: &HashMap<PackageKey, pnpm_graph_hasher::DepsGraphNode<PackageKey>>,
        patch_file_hash: Option<&str>,
        diff: pnpm_store_dir::SideEffectsDiff,
        store: &pnpm_store_dir::StoreDir,
    ) -> Result<(), String> {
        if diff.is_empty() {
            return Ok(());
        }
        let Some(subject) = self.subject(&snapshot_key.without_peer(), metadata) else {
            return Ok(());
        };
        let input_key =
            pnpm_graph_hasher::calc_dep_state_input_key(graph, snapshot_key, patch_file_hash);
        let upload = artifact_upload(diff.added.unwrap_or_default(), store)?;
        let payload = ArtifactPayload {
            kind: ARTIFACT_KIND.to_string(),
            subject,
            input_key: input_key.clone(),
            owner: OwnerScope::organization(self.organization.clone()),
            builder_id: self.builder_id.clone(),
            builder_profile: self.builder_profile.clone(),
            compatibility: CompatibilityConstraints::Tagged {
                tags: vec![self.platform.tag().map_err(|error| error.to_string())?],
            },
            manifest: ArtifactManifest {
                added: upload.files,
                deleted: diff.deleted.unwrap_or_default(),
            },
        };
        self.publish_payload(input_key, &payload, upload.blobs.into_values().collect())
    }

    fn publish_payload(
        &self,
        input_key: String,
        payload: &ArtifactPayload,
        blobs: Vec<ArtifactBlobUpload>,
    ) -> Result<(), String> {
        self.runtime
            .block_on(
                self.client.publish_artifact(
                    &PublishArtifactRequest {
                        key: input_key,
                        envelope: SignedArtifactEnvelope::sign(
                            payload,
                            self.key_id.clone(),
                            &self.private_key,
                        )
                        .map_err(|error| error.to_string())?,
                        blobs,
                    },
                    self.authorization.as_deref(),
                ),
            )
            .map_err(|error| error.to_string())
    }

    /// The artifact's subject, when this publisher covers the package
    /// and its source can be pinned.
    fn subject(
        &self,
        metadata_key: &PackageKey,
        metadata: &PackageMetadata,
    ) -> Option<ArtifactSubject> {
        let package_name = metadata_key.name.to_string();
        if !self.packages.contains(&package_name) {
            return None;
        }
        let source_integrity =
            metadata.resolution.checkable_integrity().map(ToString::to_string)?;
        Some(ArtifactSubject::dependency_side_effects(
            PackageIdentity {
                name: package_name,
                version: package_version(metadata_key, metadata.version.as_deref()),
            },
            source_integrity,
        ))
    }
}

/// The built files as the artifact lists them, each one's bytes read
/// from the CAFS for upload.
struct ArtifactUpload {
    files: Vec<ArtifactFile>,
    blobs: BTreeMap<String, ArtifactBlobUpload>,
}

fn artifact_upload(
    added: HashMap<String, CafsFileInfo>,
    store: &pnpm_store_dir::StoreDir,
) -> Result<ArtifactUpload, String> {
    let mut files = Vec::new();
    let mut blobs = BTreeMap::new();
    for (path, info) in added {
        let integrity = digest_integrity(&info.digest)?;
        let stored_path = store
            .cas_file_path_by_mode(&info.digest, info.mode)
            .ok_or_else(|| format!("invalid CAFS digest for built file {path:?}"))?;
        let bytes = std::fs::read(&stored_path)
            .map_err(|error| format!("failed to read {}: {error}", stored_path.display()))?;
        files.push(ArtifactFile {
            path,
            integrity: integrity.clone(),
            mode: info.mode,
            size: info.size,
        });
        blobs
            .entry(integrity.clone())
            .or_insert_with(|| ArtifactBlobUpload { integrity, data: BASE64.encode(bytes) });
    }
    files.sort_unstable_by(|left, right| left.path.cmp(&right.path));
    Ok(ArtifactUpload { files, blobs })
}

fn dependency_package(candidate: &ArtifactCandidate) -> &PackageIdentity {
    let ArtifactSubject::DependencySideEffects { package, .. } = &candidate.subject else {
        unreachable!("dependency side-effects candidates have dependency subjects")
    };
    package
}

#[cfg(test)]
mod tests;
