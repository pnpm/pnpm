mod access_scope;

mod recovery;

mod behavior;

mod publication;

mod quota;

use std::{
    collections::BTreeMap,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use futures_util::{StreamExt as _, stream::BoxStream};
use object_store::{
    CopyOptions, GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore,
    ObjectStoreExt, PutMultipartOptions, PutOptions, PutPayload, PutResult, RenameOptions,
    memory::InMemory, path::Path as ObjectPath,
};
use pnpm_shared_artifact_protocol::{
    ARTIFACT_KIND, ArtifactBlobRequest, ArtifactBlobUpload, ArtifactCandidate, ArtifactFile,
    ArtifactManifest, ArtifactPayload, ArtifactSubject, ArtifactVariant, BuilderProfile,
    CompatibilityConstraints, MAX_RESOLVE_RESPONSE_SIZE, MAX_VARIANTS_PER_CANDIDATE, OwnerScope,
    PackageIdentity, PublishArtifactRequest, ResolveArtifactsRequest, ResolveArtifactsResponse,
    ResolvedArtifact, SIGNATURE_ALGORITHM, SignedArtifactEnvelope, WORKSPACE_TASK_ARTIFACT_KIND,
};
use pnpr_config::{HostedStoreConfig, normalize_key_prefix};
use pnpr_error::RegistryError;
use sha2::{Digest as _, Sha512};
use tempfile::TempDir;

use super::{
    ArtifactUsage, CompilerCacheKey, ResolveBudget, SharedArtifactStore, artifact_operation_id,
    is_variant_file, is_write_conflict, owner_key,
};

fn lookup(owner: &str) -> ResolveArtifactsRequest {
    ResolveArtifactsRequest {
        candidates: vec![ArtifactCandidate {
            key: "dependency-side-effects:v1:deps=abc".to_string(),
            subject: ArtifactSubject::dependency_side_effects(
                PackageIdentity { name: "native-addon".to_string(), version: "1.0.0".to_string() },
                "sha512-source",
            ),
            owner: OwnerScope::organization(owner),
        }],
    }
}

fn publication(builder_id: &str) -> PublishArtifactRequest {
    publication_request("dependency-side-effects:v1:deps=abc", builder_id, None)
}

/// One input key admits one artifact per set of compatibility constraints, so a
/// test wanting several of them for one dependency has to vary the platform —
/// which is the only reason a second artifact for one input is legitimate.
fn for_platform(mut request: PublishArtifactRequest, index: usize) -> PublishArtifactRequest {
    let mut payload: ArtifactPayload =
        serde_json::from_slice(&BASE64.decode(&request.envelope.payload).unwrap()).unwrap();
    // Node major, not the glibc floor: two floors for one architecture and Node
    // major both apply to a consumer meeting the higher one, so they overlap and
    // the second could not be published. Distinct Node majors never share a
    // consumer, which is what a test needing several artifacts at once wants.
    payload.compatibility = CompatibilityConstraints::Tagged {
        tags: vec![format!("pnpm:v1:linux-x64-node{}-glibc2.17", index + 1)],
    };
    request.envelope.payload = BASE64.encode(serde_json::to_vec(&payload).unwrap());
    request
}

fn publication_for_platform(index: usize) -> PublishArtifactRequest {
    for_platform(publication(&format!("ci/{index}")), index)
}

fn publication_tagged(builder_id: &str, tags: &[&str]) -> PublishArtifactRequest {
    let mut request = publication(builder_id);
    let mut payload: ArtifactPayload =
        serde_json::from_slice(&BASE64.decode(&request.envelope.payload).unwrap()).unwrap();
    payload.compatibility = CompatibilityConstraints::Tagged {
        tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
    };
    request.envelope.payload = BASE64.encode(serde_json::to_vec(&payload).unwrap());
    request
}

fn publication_with_blob(input_key: &str, builder_id: &str) -> PublishArtifactRequest {
    publication_request(input_key, builder_id, Some(b"shared addon"))
}

fn publication_request(
    input_key: &str,
    builder_id: &str,
    blob: Option<&[u8]>,
) -> PublishArtifactRequest {
    let (added, blobs) = match blob {
        Some(bytes) => {
            let integrity = format!("sha512-{}", BASE64.encode(Sha512::digest(bytes)));
            (
                vec![ArtifactFile {
                    path: "build/addon.node".to_string(),
                    integrity: integrity.clone(),
                    mode: 0o755,
                    size: bytes.len() as u64,
                }],
                vec![ArtifactBlobUpload { integrity, data: BASE64.encode(bytes) }],
            )
        }
        None => (Vec::new(), Vec::new()),
    };
    let payload = ArtifactPayload {
        kind: ARTIFACT_KIND.to_string(),
        subject: ArtifactSubject::dependency_side_effects(
            PackageIdentity { name: "native-addon".to_string(), version: "1.0.0".to_string() },
            "sha512-source",
        ),
        input_key: input_key.to_string(),
        owner: OwnerScope::organization("acme"),
        builder_id: builder_id.to_string(),
        builder_profile: BuilderProfile {
            image_digest: Some("sha256:image".to_string()),
            architecture_baseline: "x86-64-v2".to_string(),
            environment: BTreeMap::new(),
        },
        compatibility: CompatibilityConstraints::Universal,
        manifest: ArtifactManifest { added, deleted: Vec::new() },
    };
    let payload_bytes = serde_json::to_vec(&payload).unwrap();
    PublishArtifactRequest {
        key: payload.input_key,
        envelope: SignedArtifactEnvelope {
            algorithm: SIGNATURE_ALGORITHM.to_string(),
            key_id: "acme-2026".to_string(),
            payload: BASE64.encode(payload_bytes),
            signature: "MAYCAQECAQE=".to_string(),
        },
        blobs,
    }
}

fn workspace_task_publication() -> PublishArtifactRequest {
    let payload = ArtifactPayload {
        kind: WORKSPACE_TASK_ARTIFACT_KIND.to_string(),
        subject: ArtifactSubject::workspace_task("packages/app", "build"),
        input_key: "workspace-task:v1:inputs=abc".to_string(),
        owner: OwnerScope::organization("acme"),
        builder_id: "ci/linux".to_string(),
        builder_profile: BuilderProfile {
            image_digest: Some("sha256:image".to_string()),
            architecture_baseline: "x86-64-v2".to_string(),
            environment: BTreeMap::new(),
        },
        compatibility: CompatibilityConstraints::Universal,
        manifest: ArtifactManifest { added: Vec::new(), deleted: Vec::new() },
    };
    PublishArtifactRequest {
        key: payload.input_key.clone(),
        envelope: SignedArtifactEnvelope {
            algorithm: SIGNATURE_ALGORITHM.to_string(),
            key_id: "acme-2026".to_string(),
            payload: BASE64.encode(serde_json::to_vec(&payload).unwrap()),
            signature: "MAYCAQECAQE=".to_string(),
        },
        blobs: Vec::new(),
    }
}

#[derive(Debug)]
struct FailArtifactWrites {
    inner: InMemory,
    commit_before_error: bool,
    fail_deletes: bool,
    fail_next_quota_write: Option<Arc<AtomicBool>>,
    /// Stands in for the publication that won a race for a slot: the first
    /// creation of this path stores these bytes instead and reports the
    /// conflict the loser would see.
    claim_slot_first: Option<(String, Vec<u8>)>,
    /// Fails reads of the slot *after* the first, so the pre-check still finds
    /// it free and the failure lands on the re-read that follows a lost create
    /// — the only point where the loser is charged for what it did not store.
    fail_slot_read_after_first: Option<Arc<AtomicUsize>>,
    /// Stands in for a publication whose constraints merely overlap this one's.
    /// It lands once this one's variant is written, which is after the overlap
    /// scan found the entry clear — the window a conditional create on a
    /// different path cannot close. Writes pass through rather than failing.
    publish_overlapping_after_create: Option<(String, Vec<u8>)>,
    /// Fails reads of this path, so a scan that reaches it cannot finish.
    fail_reads_of: Option<String>,
    /// Applies the injected write failure to scope markers too. They pass
    /// through by default, so a test injecting a failure reaches the envelope
    /// or blob it is aimed at rather than stopping at the claim.
    fail_scope_writes: bool,
    /// Lets every operation through except the one named, so a test can put a
    /// failure exactly where it means it.
    fail_only: Option<FailOnly>,
    /// Counts writes of the usage document, which is what a reservation and a
    /// release each cost against a hosted store.
    usage_writes: Option<Arc<AtomicUsize>>,
}

#[derive(Debug)]
enum FailOnly {
    /// The quota write that follows this path being stored — the registration a
    /// publication takes again once its artifact is durable, rather than the
    /// reservation that precedes it.
    RegistrationAfter(String),
    DeleteOf(String),
    WriteOf(String),
}

impl fmt::Display for FailArtifactWrites {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("fail artifact writes")
    }
}

impl FailArtifactWrites {
    fn count_usage_write(&self, location: &ObjectPath) {
        if let Some(writes) = self.usage_writes.as_ref()
            && location.as_ref().ends_with("/quota.json")
        {
            writes.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Let another publication win the slot this write is claiming.
    async fn claim_slot_first(
        &self,
        location: &ObjectPath,
    ) -> Option<object_store::Result<PutResult>> {
        let (slot, winner) = self.claim_slot_first.as_ref()?;
        if location.as_ref() != slot {
            return None;
        }
        Some(
            async {
                self.inner
                    .put_opts(location, PutPayload::from(winner.clone()), PutOptions::default())
                    .await?;
                Err(object_store::Error::AlreadyExists {
                    path: location.to_string(),
                    source: std::io::Error::other("slot claimed by another publication").into(),
                })
            }
            .await,
        )
    }

    /// Fail only the one write the test named, letting every other through.
    async fn put_with_targeted_failure(
        &self,
        location: &ObjectPath,
        payload: PutPayload,
        options: PutOptions,
    ) -> object_store::Result<PutResult> {
        let injected = match self.fail_only.as_ref().expect("caller checked fail_only") {
            FailOnly::RegistrationAfter(stored) => {
                location.as_ref().ends_with("/quota.json")
                    && self.inner.head(&ObjectPath::from(stored.as_str())).await.is_ok()
            }
            FailOnly::WriteOf(path) => location.as_ref() == path,
            FailOnly::DeleteOf(_) => false,
        };
        if !injected {
            return self.inner.put_opts(location, payload, options).await;
        }
        if self.commit_before_error {
            self.inner.put_opts(location, payload, options).await?;
        }
        Err(object_store::Error::Generic {
            store: "test",
            source: std::io::Error::other("injected write failure").into(),
        })
    }

    async fn put_quota(
        &self,
        location: &ObjectPath,
        payload: PutPayload,
        options: PutOptions,
    ) -> object_store::Result<PutResult> {
        if self
            .fail_next_quota_write
            .as_ref()
            .is_some_and(|fail| fail.swap(false, Ordering::SeqCst))
        {
            return Err(object_store::Error::Generic {
                store: "test",
                source: std::io::Error::other("injected quota write failure").into(),
            });
        }
        self.inner.put_opts(location, payload, options).await
    }
}

#[async_trait]
impl ObjectStore for FailArtifactWrites {
    async fn put_opts(
        &self,
        location: &ObjectPath,
        payload: PutPayload,
        options: PutOptions,
    ) -> object_store::Result<PutResult> {
        self.count_usage_write(location);
        if let Some(claimed) = self.claim_slot_first(location).await {
            return claimed;
        }
        if self.fail_only.is_some() {
            return self.put_with_targeted_failure(location, payload, options).await;
        }
        if !self.fail_scope_writes && location.as_ref().contains("/scopes/") {
            return self.inner.put_opts(location, payload, options).await;
        }
        if location.as_ref().ends_with("/quota.json") {
            return self.put_quota(location, payload, options).await;
        }
        let Some((path, envelope)) = self.publish_overlapping_after_create.as_ref() else {
            if self.commit_before_error {
                self.inner.put_opts(location, payload, options).await?;
            }
            return Err(object_store::Error::Generic {
                store: "test",
                source: std::io::Error::other("injected artifact write failure").into(),
            });
        };
        let stored = self.inner.put_opts(location, payload, options).await?;
        if location.as_ref() != path {
            self.inner
                .put_opts(
                    &ObjectPath::from(path.as_str()),
                    PutPayload::from(envelope.clone()),
                    PutOptions::default(),
                )
                .await?;
        }
        Ok(stored)
    }

    async fn put_multipart_opts(
        &self,
        location: &ObjectPath,
        options: PutMultipartOptions,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        self.inner.put_multipart_opts(location, options).await
    }

    async fn get_opts(
        &self,
        location: &ObjectPath,
        options: GetOptions,
    ) -> object_store::Result<GetResult> {
        if self.fail_reads_of.as_ref().is_some_and(|path| location.as_ref() == path) {
            return Err(object_store::Error::Generic {
                store: "test",
                source: std::io::Error::other("injected variant read failure").into(),
            });
        }
        if let Some(reads) = self.fail_slot_read_after_first.as_ref()
            && self.claim_slot_first.as_ref().is_some_and(|(slot, _)| location.as_ref() == slot)
            && reads.fetch_add(1, Ordering::SeqCst) > 0
        {
            return Err(object_store::Error::Generic {
                store: "test",
                source: std::io::Error::other("injected slot read failure").into(),
            });
        }
        self.inner.get_opts(location, options).await
    }

    fn delete_stream(
        &self,
        locations: BoxStream<'static, object_store::Result<ObjectPath>>,
    ) -> BoxStream<'static, object_store::Result<ObjectPath>> {
        if let Some(FailOnly::DeleteOf(failing)) = self.fail_only.as_ref() {
            let failing = failing.clone();
            let inner = self.inner.clone();
            return locations
                .then(move |location| {
                    let (failing, inner) = (failing.clone(), inner.clone());
                    async move {
                        let location = location?;
                        if location.as_ref() == failing {
                            return Err(object_store::Error::Generic {
                                store: "test",
                                source: std::io::Error::other("injected deletion failure").into(),
                            });
                        }
                        inner.delete(&location).await?;
                        Ok(location)
                    }
                })
                .boxed();
        }
        if self.fail_deletes {
            locations
                .map(|location| {
                    location?;
                    Err(object_store::Error::Generic {
                        store: "test",
                        source: std::io::Error::other("injected artifact deletion failure").into(),
                    })
                })
                .boxed()
        } else {
            self.inner.delete_stream(locations)
        }
    }

    fn list(
        &self,
        prefix: Option<&ObjectPath>,
    ) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        self.inner.list(prefix)
    }

    fn list_with_offset(
        &self,
        prefix: Option<&ObjectPath>,
        offset: &ObjectPath,
    ) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        self.inner.list_with_offset(prefix, offset)
    }

    async fn list_with_delimiter(
        &self,
        prefix: Option<&ObjectPath>,
    ) -> object_store::Result<ListResult> {
        self.inner.list_with_delimiter(prefix).await
    }

    async fn copy_opts(
        &self,
        from: &ObjectPath,
        to: &ObjectPath,
        options: CopyOptions,
    ) -> object_store::Result<()> {
        self.inner.copy_opts(from, to, options).await
    }

    async fn rename_opts(
        &self,
        from: &ObjectPath,
        to: &ObjectPath,
        options: RenameOptions,
    ) -> object_store::Result<()> {
        self.inner.rename_opts(from, to, options).await
    }
}
