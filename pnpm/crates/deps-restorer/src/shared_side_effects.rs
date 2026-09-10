use crate::{
    AllowBuildPolicy, RemoteSideEffectsQuarantineBySnapshot, RequiresBuildBySnapshot,
    SideEffectsBySnapshot, SideEffectsMapsBySnapshot, StoreIndexKeysBySnapshot,
    build_deps_subgraph, deps_graph::in_lockfile_order,
    install_frozen_lockfile::find_runtime_node_major,
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use pnpm_config::Config;
use pnpm_lockfile::{PackageKey, PackageMetadata, SnapshotEntry};
use pnpm_pnpr_client::{
    ARTIFACT_KIND, ArtifactBlobRequest, ArtifactBlobUpload, ArtifactCandidate, ArtifactFile,
    ArtifactManifest, ArtifactPayload, ArtifactSubject, BuilderProfile, CompatibilityConstraints,
    LinuxGlibcPlatform, MacOsPlatform, OwnerScope, PackageIdentity, PnprClient, PnprClientError,
    PublishArtifactRequest, RejectedArtifact, ResolveArtifactsOptions, SignedArtifactEnvelope,
    WindowsPlatform, blob_id, linux_glibc_supported_tags, linux_glibc_tag, macos_supported_tags,
    macos_tag, windows_supported_tags, windows_tag,
};
use pnpm_shared_artifact_protocol::compatibility_rank;
use pnpm_store_dir::{CafsFileInfo, RemoteSideEffectsOrigin, SideEffectsDiff, StoreIndexWriter};
use sha2::{Digest as _, Sha512};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};
#[cfg(windows)]
use sysinfo::System;

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

#[derive(Clone, Copy)]
enum ArtifactPlatform<'a> {
    LinuxGlibc(LinuxGlibcPlatform<'a>),
    MacOs(MacOsPlatform<'a>),
    Windows(WindowsPlatform<'a>),
}

impl ArtifactPlatform<'_> {
    fn node_major(self) -> u32 {
        match self {
            Self::LinuxGlibc(platform) => platform.node_major,
            Self::MacOs(platform) => platform.node_major,
            Self::Windows(platform) => platform.node_major,
        }
    }

    fn supported_tags(
        self,
    ) -> Result<Vec<String>, pnpm_shared_artifact_protocol::ArtifactProtocolError> {
        match self {
            Self::LinuxGlibc(platform) => linux_glibc_supported_tags(platform),
            Self::MacOs(platform) => macos_supported_tags(platform),
            Self::Windows(platform) => windows_supported_tags(platform),
        }
    }

    fn tag(self) -> Result<String, pnpm_shared_artifact_protocol::ArtifactProtocolError> {
        match self {
            Self::LinuxGlibc(platform) => linux_glibc_tag(platform),
            Self::MacOs(platform) => macos_tag(platform),
            Self::Windows(platform) => windows_tag(platform),
        }
    }
}

struct CandidateGroup {
    candidate: ArtifactCandidate,
    snapshots: Vec<(PackageKey, String, String)>,
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

    let roots = eligible_roots(
        options.snapshots,
        options.requires_build_by_snapshot,
        options.allow_build_policy,
        options.base_cas_paths,
        &setup.eligible_packages,
    );
    tracing::debug!(
        target: "pacquet::install",
        eligible_snapshots = roots.len(),
        "planned remote side-effects candidates",
    );
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

/// Resolve the groups' artifacts on the configured pnpr server,
/// quarantine the rejected ones and overlay the rest.
async fn fetch_remote_artifacts(
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
struct RemoteCacheSetup {
    supported_tags: Vec<String>,
    trusted_keys: BTreeMap<String, Vec<u8>>,
    owner: OwnerScope,
    eligible_packages: HashSet<String>,
    node_major: u32,
}

fn remote_cache_setup(
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

/// The snapshots whose built output the remote cache may supply: an
/// eligible package with a build to run, an explicit allow-build
/// verdict, and a materialized base file map to overlay.
fn eligible_roots(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    requires_build_by_snapshot: &RequiresBuildBySnapshot,
    allow_build_policy: &AllowBuildPolicy,
    base_cas_paths: &BaseCasPaths,
    eligible_packages: &HashSet<String>,
) -> Vec<PackageKey> {
    in_lockfile_order(snapshots)
        .into_iter()
        .filter(|(snapshot_key, _)| {
            requires_build_by_snapshot.get(*snapshot_key).copied().unwrap_or(false)
                && eligible_packages.contains(&snapshot_key.name.to_string())
                && allow_build_policy.check(&snapshot_key.without_peer().to_string()) == Some(true)
                && base_cas_paths.contains_key(*snapshot_key)
        })
        .map(|(snapshot_key, _)| snapshot_key.clone())
        .collect()
}

/// The read-only inputs of [`plan_candidate_groups`].
struct CandidatePlan<'a> {
    config: &'a Config,
    snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
    packages: &'a HashMap<PackageKey, PackageMetadata>,
    setup: &'a RemoteCacheSetup,
    side_effects_by_snapshot: &'a SideEffectsBySnapshot,
    store_index_keys_by_snapshot: &'a StoreIndexKeysBySnapshot,
}

/// Group the eligible snapshots by artifact input key, dropping the
/// ones a persisted or locally cached overlay already covers.
///
/// Two snapshots that hash to one input key but describe different
/// subjects are a collision: neither may be looked up, because the
/// server answers per key.
async fn plan_candidate_groups(
    plan: &CandidatePlan<'_>,
    roots: Vec<PackageKey>,
    mut persisted_remote: HashMap<(PackageKey, String), HashMap<String, PathBuf>>,
    side_effects_maps_by_snapshot: &mut SideEffectsMapsBySnapshot,
) -> BTreeMap<String, CandidateGroup> {
    let mut hasher = DepStateHasher::new(plan, &roots);
    let mut groups = BTreeMap::<String, CandidateGroup>::new();
    let mut collisions = HashSet::new();
    for snapshot_key in roots {
        let patch_hash = patch_hash(&snapshot_key);
        let input_key = hasher.input_key(&snapshot_key, patch_hash.as_deref());
        if collisions.contains(&input_key) {
            continue;
        }
        let Some(planned) = plan_root(
            plan,
            &mut hasher,
            RootKeys {
                snapshot_key: &snapshot_key,
                input_key: &input_key,
                patch_hash: patch_hash.as_deref(),
            },
            &mut persisted_remote,
            side_effects_maps_by_snapshot,
        )
        .await
        else {
            continue;
        };
        group_candidate(
            &mut groups,
            &mut collisions,
            input_key,
            planned.candidate,
            (snapshot_key, planned.local_cache_key, planned.store_index_key),
        );
    }
    groups
}

/// The dep graph the eligible roots hash over, with its memo and the
/// install's engine string.
struct DepStateHasher {
    graph: HashMap<PackageKey, pnpm_graph_hasher::DepsGraphNode<PackageKey>>,
    cache: pnpm_graph_hasher::DepsStateCache<PackageKey>,
    engine_name: String,
}

impl DepStateHasher {
    fn new(plan: &CandidatePlan<'_>, roots: &[PackageKey]) -> Self {
        let graph = build_deps_subgraph(plan.snapshots, plan.packages, roots.iter().cloned());
        let mut cache = pnpm_graph_hasher::DepsStateCache::new();
        pnpm_graph_hasher::warm_deps_state_cache(
            &graph,
            &mut cache,
            in_lockfile_order(&graph).into_iter().map(|(key, _)| key),
        );
        Self {
            graph,
            cache,
            engine_name: pnpm_graph_hasher::engine_name(plan.setup.node_major, None, None),
        }
    }

    fn input_key(&self, snapshot_key: &PackageKey, patch_hash: Option<&str>) -> String {
        pnpm_graph_hasher::calc_dep_state_input_key(&self.graph, snapshot_key, patch_hash)
    }

    fn local_cache_key(&mut self, snapshot_key: &PackageKey, patch_hash: Option<&str>) -> String {
        pnpm_graph_hasher::calc_dep_state(
            &self.graph,
            &mut self.cache,
            snapshot_key,
            &pnpm_graph_hasher::CalcDepStateOptions {
                engine_name: &self.engine_name,
                patch_file_hash: patch_hash,
                include_dep_graph_hash: true,
            },
        )
    }
}

struct RootKeys<'r> {
    snapshot_key: &'r PackageKey,
    input_key: &'r str,
    patch_hash: Option<&'r str>,
}

struct PlannedRoot {
    candidate: ArtifactCandidate,
    local_cache_key: String,
    store_index_key: String,
}

/// `None` when a persisted or locally cached overlay already covers the
/// root, or the store index holds no row for it.
async fn plan_root(
    plan: &CandidatePlan<'_>,
    hasher: &mut DepStateHasher,
    root: RootKeys<'_>,
    persisted_remote: &mut HashMap<(PackageKey, String), HashMap<String, PathBuf>>,
    side_effects_maps_by_snapshot: &mut SideEffectsMapsBySnapshot,
) -> Option<PlannedRoot> {
    let candidate =
        artifact_candidate(plan, root.snapshot_key, root.input_key, plan.setup.owner.clone())?;
    let local_cache_key = hasher.local_cache_key(root.snapshot_key, root.patch_hash);
    if reuse_persisted_overlay(
        plan,
        &candidate,
        root.snapshot_key,
        &local_cache_key,
        persisted_remote,
        side_effects_maps_by_snapshot,
    )
    .await
    {
        return None;
    }
    if plan.config.side_effects_cache_read()
        && side_effects_maps_by_snapshot
            .get(root.snapshot_key)
            .is_some_and(|maps| maps.contains_key(&local_cache_key))
    {
        return None;
    }
    let store_index_key = plan.store_index_keys_by_snapshot.get(root.snapshot_key).cloned()?;
    Some(PlannedRoot { candidate, local_cache_key, store_index_key })
}

/// The artifact one snapshot would look up. `None` when the package has
/// no metadata row, or no integrity to bind the artifact's subject to.
fn artifact_candidate(
    plan: &CandidatePlan<'_>,
    snapshot_key: &PackageKey,
    input_key: &str,
    owner: OwnerScope,
) -> Option<ArtifactCandidate> {
    let metadata_key = snapshot_key.without_peer();
    let metadata = plan.packages.get(&metadata_key)?;
    let source_integrity = metadata.resolution.checkable_integrity().map(ToString::to_string)?;
    Some(ArtifactCandidate {
        key: input_key.to_owned(),
        subject: ArtifactSubject::dependency_side_effects(
            PackageIdentity {
                name: metadata_key.name.to_string(),
                version: package_version(&metadata_key, metadata.version.as_deref()),
            },
            source_integrity,
        ),
        owner,
    })
}

/// Whether a previous install's persisted overlay still stands for this
/// snapshot: its stored diff verifies against the candidate and every
/// blob it names is still in the store. A reused overlay is re-inserted
/// into the live maps and needs no lookup.
async fn reuse_persisted_overlay(
    plan: &CandidatePlan<'_>,
    candidate: &ArtifactCandidate,
    snapshot_key: &PackageKey,
    local_cache_key: &str,
    persisted_remote: &mut HashMap<(PackageKey, String), HashMap<String, PathBuf>>,
    side_effects_maps_by_snapshot: &mut SideEffectsMapsBySnapshot,
) -> bool {
    let Some(overlay) =
        persisted_remote.remove(&(snapshot_key.clone(), local_cache_key.to_owned()))
    else {
        return false;
    };
    let diff = plan
        .side_effects_by_snapshot
        .get(snapshot_key)
        .and_then(|diffs| diffs.get(local_cache_key))
        .filter(|diff| {
            stored_remote_side_effects_are_verified(
                diff,
                candidate,
                plan.config.pnpr_server.as_deref(),
                &plan.setup.supported_tags,
                &plan.setup.trusted_keys,
            )
        });
    let Some(diff) = diff else {
        return false;
    };
    match stored_remote_side_effects_blobs_are_valid(diff, &overlay).await {
        Ok(true) => {
            insert_side_effects_map(
                side_effects_maps_by_snapshot,
                snapshot_key.clone(),
                local_cache_key.to_owned(),
                overlay,
            );
            true
        }
        Ok(false) => false,
        // An artifact that cannot be checked is not looked up remotely
        // either: the local build stands in for it.
        Err(error) => {
            tracing::warn!(
                target: "pacquet::install",
                package = %dependency_package(candidate).name,
                %error,
                "persisted remote side-effects artifact could not be checked",
            );
            true
        }
    }
}

/// Add the candidate to its input key's group. Two snapshots that share
/// an input key but not a subject cancel the key outright: the group is
/// dropped and the key remembered so later snapshots skip it too.
fn group_candidate(
    groups: &mut BTreeMap<String, CandidateGroup>,
    collisions: &mut HashSet<String>,
    input_key: String,
    candidate: ArtifactCandidate,
    entry: (PackageKey, String, String),
) {
    let Some(group) = groups.get_mut(&input_key) else {
        groups.insert(input_key, CandidateGroup { candidate, snapshots: vec![entry] });
        return;
    };
    if group.candidate.subject != candidate.subject {
        groups.remove(&input_key);
        collisions.insert(input_key);
        return;
    }
    group.snapshots.push(entry);
}

/// Ask the remote cache which of the planned groups it can supply.
/// `None` when the lookup could not run: a failed handshake or query is
/// a cache miss, never an install failure.
async fn resolve_remote_artifacts(
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
    let quarantined_envelope_digests = groups
        .iter()
        .map(|(input_key, group)| {
            let digests = group
                .snapshots
                .iter()
                .filter_map(|(snapshot_key, _, _)| {
                    remote_side_effects_quarantine_by_snapshot
                        .get(snapshot_key)
                        .and_then(|channels| channels.get(server))
                })
                .flatten()
                .cloned()
                .collect();
            (input_key.clone(), digests)
        })
        .collect();
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

/// What one resolved artifact needs to be staged into the store.
struct ResolvedArtifactContext<'a> {
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
async fn apply_resolved_artifact(
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
struct StagedArtifact {
    overlay: HashMap<String, PathBuf>,
    added: HashMap<String, CafsFileInfo>,
}

async fn stage_artifact(
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

fn remote_diff(
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
fn record_group(
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

fn report_rejected_artifact(
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
async fn stage_artifact_blob(
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
            return Err((
                "shared artifact blob does not match its declared size".to_string(),
                true,
            ));
        }
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
async fn stored_blob_path(
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

fn take_persisted_remote_side_effects(
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
fn take_snapshot_overlays(
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

fn insert_side_effects_map(
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

fn stored_remote_side_effects_are_verified(
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

fn manifest_matches_diff(manifest: &ArtifactManifest, diff: &SideEffectsDiff) -> bool {
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

async fn stored_remote_side_effects_blobs_are_valid(
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

fn quarantine_remote_side_effects(
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

/// Reads the store in chunks this size while hashing, so a large CAS blob
/// is never held in memory whole.
const STORE_READ_CHUNK: usize = 64 * 1024;

/// Whether the store already holds `digest` at `path`.
///
/// Verified unconditionally rather than answering to `verifyStoreIntegrity`:
/// the download this skips would have ended in a CAS write, and that path
/// checks content already at the destination whatever the setting says.
/// Hashing a local file is far cheaper than the transfer it avoids.
///
/// A missing file is an ordinary miss; any other failure is reported rather
/// than quietly redownloaded.
async fn store_holds(path: &Path, digest: &str) -> Result<bool, String> {
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
        self.runtime
            .block_on(
                self.client.publish_artifact(
                    &PublishArtifactRequest {
                        key: input_key,
                        envelope: SignedArtifactEnvelope::sign(
                            &payload,
                            self.key_id.clone(),
                            &self.private_key,
                        )
                        .map_err(|error| error.to_string())?,
                        blobs: upload.blobs.into_values().collect(),
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

fn artifact_platform(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
) -> Option<ArtifactPlatform<'static>> {
    let architecture = pnpm_graph_hasher::host_arch();
    if !matches!(architecture, "x64" | "arm64") {
        return None;
    }
    let node_major =
        find_runtime_node_major(Some(snapshots)).or_else(pnpm_graph_hasher::detect_node_major)?;
    match pnpm_graph_hasher::host_platform() {
        "linux" => {
            let (glibc_major, glibc_minor) = pnpm_detect_libc::glibc_version()?;
            Some(ArtifactPlatform::LinuxGlibc(LinuxGlibcPlatform {
                architecture,
                node_major,
                glibc_major,
                glibc_minor,
            }))
        }
        "darwin" => {
            let (macos_major, macos_minor) = macos_product_version()?;
            Some(ArtifactPlatform::MacOs(MacOsPlatform {
                architecture,
                node_major,
                macos_major,
                macos_minor,
            }))
        }
        "win32" => {
            let (windows_major, windows_minor, windows_build) = windows_kernel_version()?;
            Some(ArtifactPlatform::Windows(WindowsPlatform {
                architecture,
                node_major,
                windows_major,
                windows_minor,
                windows_build,
            }))
        }
        _ => None,
    }
}

fn macos_product_version() -> Option<(u32, u32)> {
    let output = Command::new("/usr/bin/sw_vers").arg("-productVersion").output().ok()?;
    output.status.success().then_some(())?;
    parse_macos_product_version(std::str::from_utf8(&output.stdout).ok()?)
}

fn parse_macos_product_version(value: &str) -> Option<(u32, u32)> {
    let mut components = value.trim().split('.');
    let major = components.next()?.parse().ok()?;
    let minor = components.next()?.parse().ok()?;
    (major > 0 && major < 1_000_000 && minor < 1_000_000).then_some((major, minor))
}

#[cfg(windows)]
fn windows_kernel_version() -> Option<(u32, u32, u32)> {
    let build = System::kernel_version()?.parse().ok()?;
    // Windows 10 and 11 both use NT kernel version 10.0; sysinfo provides the build number.
    validate_windows_kernel_version(10, 0, build)
}

#[cfg(not(windows))]
fn windows_kernel_version() -> Option<(u32, u32, u32)> {
    None
}

#[cfg(any(windows, test))]
fn validate_windows_kernel_version(major: u32, minor: u32, build: u32) -> Option<(u32, u32, u32)> {
    (major > 0 && major < 1_000 && minor < 1_000 && build > 0 && build < 1_000_000)
        .then_some((major, minor, build))
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
