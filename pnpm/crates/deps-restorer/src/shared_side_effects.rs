use crate::{
    AllowBuildPolicy, RequiresBuildBySnapshot, SideEffectsMapsBySnapshot, build_deps_subgraph,
    deps_graph::in_lockfile_order, install_frozen_lockfile::find_runtime_node_major,
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use pnpm_config::Config;
use pnpm_lockfile::{PackageKey, PackageMetadata, SnapshotEntry};
use pnpm_pnpr_client::{
    ARTIFACT_KIND, ArtifactBlobRequest, ArtifactBlobUpload, ArtifactCandidate, ArtifactFile,
    ArtifactManifest, ArtifactPayload, BuilderProfile, CompatibilityConstraints,
    LinuxGlibcPlatform, OwnerScope, PackageIdentity, PnprClient, PublishArtifactRequest,
    ResolveArtifactsOptions, SignedArtifactEnvelope, blob_id, linux_glibc_supported_tags,
    linux_glibc_tag,
};
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
    platform: LinuxGlibcPlatform<'static>,
    private_key: Vec<u8>,
    runtime: tokio::runtime::Handle,
}

struct CandidateGroup {
    candidate: ArtifactCandidate,
    snapshots: Vec<(PackageKey, String)>,
}

pub(crate) async fn apply_shared_side_effects(
    config: &Config,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
    requires_build_by_snapshot: &RequiresBuildBySnapshot,
    allow_build_policy: &AllowBuildPolicy,
    base_cas_paths: &BaseCasPaths,
    side_effects_maps_by_snapshot: &mut SideEffectsMapsBySnapshot,
) {
    if config.ignore_scripts || config.frozen_store {
        return;
    }
    let (Some(server), Some(settings)) =
        (config.pnpr_server.as_deref(), config.remote_side_effects_cache.as_ref())
    else {
        return;
    };
    let Some(platform) = linux_glibc_platform(snapshots) else { return };
    let supported_tags = match linux_glibc_supported_tags(platform) {
        Ok(tags) => tags,
        Err(error) => {
            tracing::warn!(target: "pacquet::install", %error, "remote side-effects platform is unsupported");
            return;
        }
    };
    let Some(trusted_keys) = decoded_trusted_keys(settings) else { return };
    let Some(organization) = non_empty(&settings.organization) else { return };

    let eligible_packages: HashSet<String> = settings.packages.iter().cloned().collect();
    let roots: Vec<PackageKey> = in_lockfile_order(snapshots)
        .into_iter()
        .filter(|(snapshot_key, _)| {
            requires_build_by_snapshot.get(*snapshot_key).copied().unwrap_or(false)
                && eligible_packages.contains(&snapshot_key.name.to_string())
                && allow_build_policy.check(&snapshot_key.without_peer().to_string()) == Some(true)
                && base_cas_paths.contains_key(*snapshot_key)
        })
        .map(|(snapshot_key, _)| snapshot_key.clone())
        .collect();
    tracing::debug!(
        target: "pacquet::install",
        eligible_snapshots = roots.len(),
        "planned remote side-effects candidates",
    );
    if roots.is_empty() {
        return;
    }
    let graph = build_deps_subgraph(snapshots, packages, roots.clone());
    let mut deps_state_cache = pnpm_graph_hasher::DepsStateCache::new();
    pnpm_graph_hasher::warm_deps_state_cache(
        &graph,
        &mut deps_state_cache,
        in_lockfile_order(&graph).into_iter().map(|(key, _)| key),
    );
    let engine_name = pnpm_graph_hasher::engine_name(platform.node_major, None, None);
    let mut groups = BTreeMap::<String, CandidateGroup>::new();
    let mut collisions = HashSet::new();
    for snapshot_key in roots {
        let metadata_key = snapshot_key.without_peer();
        let Some(metadata) = packages.get(&metadata_key) else { continue };
        let Some(source_integrity) =
            metadata.resolution.checkable_integrity().map(ToString::to_string)
        else {
            continue;
        };
        let patch_hash = patch_hash(&snapshot_key);
        let input_key = pnpm_graph_hasher::calc_dep_state_input_key(
            &graph,
            &snapshot_key,
            patch_hash.as_deref(),
        );
        if collisions.contains(&input_key) {
            continue;
        }
        let local_cache_key = pnpm_graph_hasher::calc_dep_state(
            &graph,
            &mut deps_state_cache,
            &snapshot_key,
            &pnpm_graph_hasher::CalcDepStateOptions {
                engine_name: &engine_name,
                patch_file_hash: patch_hash.as_deref(),
                include_dep_graph_hash: true,
            },
        );
        if config.side_effects_cache_read()
            && side_effects_maps_by_snapshot
                .get(&snapshot_key)
                .is_some_and(|maps| maps.contains_key(&local_cache_key))
        {
            continue;
        }
        let candidate = ArtifactCandidate {
            key: input_key.clone(),
            package: PackageIdentity {
                name: metadata_key.name.to_string(),
                version: package_version(&metadata_key, metadata.version.as_deref()),
            },
            source_integrity,
            owner: OwnerScope::organization(organization.to_string()),
        };
        if let Some(group) = groups.get_mut(&input_key) {
            if group.candidate.package != candidate.package
                || group.candidate.source_integrity != candidate.source_integrity
            {
                groups.remove(&input_key);
                collisions.insert(input_key);
                continue;
            }
            group.snapshots.push((snapshot_key, local_cache_key));
        } else {
            groups.insert(
                input_key,
                CandidateGroup { candidate, snapshots: vec![(snapshot_key, local_cache_key)] },
            );
        }
    }
    if groups.is_empty() {
        return;
    }
    tracing::debug!(
        target: "pacquet::install",
        candidates = groups.len(),
        "querying remote side-effects cache",
    );

    let client = PnprClient::new(server);
    if let Err(error) = client.handshake_artifacts().await {
        tracing::warn!(target: "pacquet::install", %error, "remote side-effects cache handshake failed");
        return;
    }
    let authorization = config.auth_headers.for_url(server);
    let allowed_builds =
        groups.values().map(|group| group.candidate.package.name.clone()).collect();
    let resolved = match client
        .resolve_artifacts(ResolveArtifactsOptions {
            candidates: groups.values().map(|group| group.candidate.clone()).collect(),
            supported_tags,
            eligible_packages,
            allowed_builds,
            ignore_scripts: false,
            trusted_keys,
            authorization: authorization.clone(),
        })
        .await
    {
        Ok(resolved) => resolved,
        Err(error) => {
            tracing::warn!(target: "pacquet::install", %error, "remote side-effects cache lookup failed");
            return;
        }
    };

    for (input_key, artifact) in resolved {
        let Some(group) = groups.get(&input_key) else { continue };
        let Some((first_snapshot, _)) = group.snapshots.first() else { continue };
        let Some(base) = base_cas_paths.get(first_snapshot) else { continue };
        let mut overlay = base.clone();
        let mut downloaded = HashMap::<String, Vec<u8>>::new();
        let mut stored = HashMap::<(String, u32), PathBuf>::new();
        for deleted in &artifact.payload.manifest.deleted {
            overlay.remove(deleted);
        }
        let mut rejected = None;
        for file in &artifact.payload.manifest.added {
            let result: Result<PathBuf, String> = async {
                let storage_key = (file.integrity.clone(), file.mode);
                if let Some(path) = stored.get(&storage_key) {
                    return Ok(path.clone());
                }
                // A built package's files are mostly its own, and artifacts
                // share files with each other. The store addresses content by
                // the digest this manifest entry already carries, so anything
                // it holds is the same bytes and needs no transfer.
                //
                // Both this lookup and the write below address the store by
                // `is_executable`, so they cannot disagree about where a mode
                // belongs. The manifest only carries 0o644 and 0o755 today,
                // but the agreement must not rest on that.
                if !downloaded.contains_key(&file.integrity)
                    && let Ok(digest) = blob_id(&file.integrity)
                    && let Some(path) = config.store_dir.cas_file_path_by_mode(&digest, file.mode)
                    && tokio::fs::try_exists(&path).await.unwrap_or(false)
                {
                    stored.insert(storage_key, path.clone());
                    return Ok(path);
                }
                if !downloaded.contains_key(&file.integrity) {
                    let bytes = client
                        .download_artifact_blob(
                            &ArtifactBlobRequest {
                                owner: artifact.payload.owner.clone(),
                                integrity: file.integrity.clone(),
                            },
                            authorization.as_deref(),
                        )
                        .await
                        .map_err(|error| error.to_string())?;
                    downloaded.insert(file.integrity.clone(), bytes);
                }
                let (path, _) = config
                    .store_dir
                    .write_cas_file(
                        &downloaded[&file.integrity],
                        pnpm_fs::file_mode::is_executable(file.mode),
                    )
                    .map_err(|error| error.to_string())?;
                stored.insert(storage_key, path.clone());
                Ok(path)
            }
            .await;
            match result {
                Ok(path) => {
                    overlay.insert(file.path.clone(), path);
                }
                Err(error) => {
                    rejected = Some(error);
                    break;
                }
            }
        }
        if let Some(error) = rejected {
            tracing::warn!(
                target: "pacquet::install",
                package = %group.candidate.package.name,
                %error,
                "remote side-effects artifact was rejected",
            );
            continue;
        }
        for (snapshot_key, local_cache_key) in &group.snapshots {
            let mut maps = side_effects_maps_by_snapshot
                .get(snapshot_key)
                .map_or_else(HashMap::new, |maps| (**maps).clone());
            maps.insert(local_cache_key.clone(), overlay.clone());
            side_effects_maps_by_snapshot.insert(snapshot_key.clone(), Arc::new(maps));
        }
    }
}

/// Decode the configured trust root, or `None` when it is absent or unusable.
///
/// A key pnpm cannot decode is a configuration mistake that would silently
/// narrow what the install trusts, so the whole lookup is abandoned rather than
/// run against a partial key set.
fn decoded_trusted_keys(
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
    let platform = linux_glibc_platform(snapshots)?;
    let private_key = BASE64.decode(settings.private_key.as_ref()?).ok()?;
    let key_id = settings.key_id.clone()?;
    let builder_id = settings.builder_id.clone()?;
    let organization = non_empty(&settings.organization)?.to_string();
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
        let metadata_key = snapshot_key.without_peer();
        let package_name = metadata_key.name.to_string();
        if !self.packages.contains(&package_name) {
            return Ok(());
        }
        let Some(source_integrity) =
            metadata.resolution.checkable_integrity().map(ToString::to_string)
        else {
            return Ok(());
        };
        let input_key =
            pnpm_graph_hasher::calc_dep_state_input_key(graph, snapshot_key, patch_file_hash);
        let mut files = Vec::new();
        let mut blobs = BTreeMap::new();
        for (path, info) in diff.added.unwrap_or_default() {
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
        let payload = ArtifactPayload {
            kind: ARTIFACT_KIND.to_string(),
            package: PackageIdentity {
                name: package_name,
                version: package_version(&metadata_key, metadata.version.as_deref()),
            },
            source_integrity,
            input_key: input_key.clone(),
            owner: OwnerScope::organization(self.organization.clone()),
            builder_id: self.builder_id.clone(),
            builder_profile: self.builder_profile.clone(),
            compatibility: CompatibilityConstraints::Tagged {
                tags: vec![linux_glibc_tag(self.platform).map_err(|error| error.to_string())?],
            },
            manifest: ArtifactManifest { added: files, deleted: diff.deleted.unwrap_or_default() },
        };
        let envelope =
            SignedArtifactEnvelope::sign(&payload, self.key_id.clone(), &self.private_key)
                .map_err(|error| error.to_string())?;
        self.runtime
            .block_on(self.client.publish_artifact(
                &PublishArtifactRequest {
                    key: input_key,
                    envelope,
                    blobs: blobs.into_values().collect(),
                },
                self.authorization.as_deref(),
            ))
            .map_err(|error| error.to_string())
    }
}

fn linux_glibc_platform(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
) -> Option<LinuxGlibcPlatform<'static>> {
    if pnpm_graph_hasher::host_platform() != "linux"
        || !matches!(pnpm_graph_hasher::host_arch(), "x64" | "arm64")
    {
        return None;
    }
    let node_major =
        find_runtime_node_major(Some(snapshots)).or_else(pnpm_graph_hasher::detect_node_major)?;
    let (glibc_major, glibc_minor) = pnpm_detect_libc::glibc_version()?;
    Some(LinuxGlibcPlatform {
        architecture: pnpm_graph_hasher::host_arch(),
        node_major,
        glibc_major,
        glibc_minor,
    })
}

fn patch_hash(snapshot_key: &PackageKey) -> Option<String> {
    let rendered = snapshot_key.to_string();
    let start = pnpm_deps_path::index_of_dep_path_suffix(&rendered).patch_hash_index?;
    let value = rendered.get(start + "(patch_hash=".len()..)?;
    Some(value.split_once(')')?.0.to_string())
}

fn package_version(package_key: &PackageKey, metadata_version: Option<&str>) -> String {
    metadata_version.map_or_else(|| package_key.suffix.version().to_string(), ToString::to_string)
}

fn digest_integrity(digest: &str) -> Result<String, String> {
    if !digest.len().is_multiple_of(2) {
        return Err("CAFS digest has an odd number of hexadecimal digits".to_string());
    }
    let bytes = digest
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).map_err(|error| error.to_string())?;
            u8::from_str_radix(pair, 16).map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format!("sha512-{}", BASE64.encode(bytes)))
}

#[cfg(test)]
mod tests;
