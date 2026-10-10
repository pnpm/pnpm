//! The remote tier of the task cache: one `workspace-task` artifact of the
//! shared artifact protocol per task key, kept on the [`ArtifactStore`]
//! `remoteCache` names, and signed and verified with the same keys as the
//! remote side-effects cache.
//!
//! An artifact's manifest mirrors a local entry: `meta.json` and the files
//! under `outputs/`. A verified artifact is written into the local tier and
//! restored from there through the same checks as a local entry.

use super::cache::{StoredTask, TaskCache};
use futures_util::{StreamExt as _, TryStreamExt as _, stream};
use pnpm_config::Config;
use pnpm_pnpr_client::{
    ArtifactBlobRequest, ArtifactBlobSource, ArtifactBuildPolicy, ArtifactCandidate, ArtifactFile,
    ArtifactManifest, ArtifactPayload, ArtifactPublication, ArtifactSigner, ArtifactStore,
    ArtifactSubject, CompatibilityConstraints, OwnerScope, ResolveArtifactsOptions,
    VerifiedArtifact, WORKSPACE_TASK_ARTIFACT_KIND, WORKSPACE_TASK_INPUT_KEY_PREFIX,
    decode_trusted_keys,
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::{self, Read as _},
    path::{Component, Path},
    sync::{Arc, Mutex},
};
use tokio::{runtime::Handle, sync::Semaphore, task::JoinHandle};

/// A task as the artifact's subject names it.
pub(super) struct TaskIdentity<'a> {
    /// The project's path relative to the workspace root.
    pub(super) project: &'a str,
    pub(super) task: &'a str,
}

pub(super) struct RemoteTaskCache {
    store: Arc<ArtifactStore>,
    owner: OwnerScope,
    trusted_keys: BTreeMap<String, Vec<u8>>,
    publisher: Option<Arc<TaskPublisher>>,
    runtime: Handle,
    uploads: Mutex<Vec<JoinHandle<Result<(), String>>>>,
    upload_slots: Arc<Semaphore>,
}

/// Blob downloads one restore has in flight at once.
const DOWNLOAD_CONCURRENCY: usize = 8;

/// Uploads in flight at once. An upload hashes the task's outputs only once
/// it holds a slot.
const MAX_CONCURRENT_UPLOADS: usize = 4;

/// Signs local entries as `workspace-task` artifacts.
struct TaskPublisher(ArtifactSigner);

impl RemoteTaskCache {
    /// The remote tier `remoteCache` configures. `Ok(None)` when no store is
    /// named, or when only `pnprServer` is set and nothing asks for shared
    /// artifacts. `Err` with the reason when the configuration cannot work.
    pub(super) fn open(config: &Config) -> Result<Option<RemoteTaskCache>, String> {
        let settings = config.remote_cache_settings();
        let Some(store) =
            ArtifactStore::from_config(config, &settings).map_err(|error| error.to_string())?
        else {
            return Ok(None);
        };
        if settings.url.is_none()
            && settings.org.is_none()
            && settings.trusted_keys.is_none()
        {
            return Ok(None);
        }
        let org = settings.org.as_deref().ok_or("remoteCache.org is not set")?;
        let trusted_keys = settings.trusted_keys
            .as_ref()
            .filter(|keys| !keys.is_empty())
            .ok_or("remoteCache.trustedKeys is not set")?;
        let trusted_keys = decode_trusted_keys(trusted_keys)
            .map_err(|key_id| format!("the trusted key {key_id:?} is not valid base64"))?;
        // Only `remoteCache` itself turns task publishing on: a machine that
        // publishes dependency builds through `sideEffectsCache.remote` has
        // not agreed to share task outputs and their logs.
        let publishes_tasks = config.remote_cache
            .as_ref()
            .is_some_and(|remote_cache| remote_cache.publish == Some(true));
        let publisher = publishes_tasks
            .then(|| {
                ArtifactSigner::from_settings(&settings)
                    .map(|signer| Arc::new(TaskPublisher(signer)))
            })
            .transpose()?;
        Ok(Some(RemoteTaskCache {
            store: Arc::new(store),
            owner: OwnerScope::organization(org),
            trusted_keys,
            publisher,
            runtime: Handle::try_current().map_err(|error| error.to_string())?,
            uploads: Mutex::new(Vec::new()),
            upload_slots: Arc::new(Semaphore::new(MAX_CONCURRENT_UPLOADS)),
        }))
    }

    /// Download the artifact stored for `key` into `cache`. `Ok(false)`
    /// when the store holds no artifact this machine trusts.
    ///
    /// Blocks, so it must run off the async runtime's worker threads.
    pub(super) fn fetch(
        &self,
        key: &str,
        task: &TaskIdentity<'_>,
        cache: &TaskCache,
    ) -> Result<bool, String> {
        let Some(files) = self.runtime.block_on(self.resolve(key, task))? else {
            return Ok(false);
        };
        cache
            .import(key, |staging| {
                self.runtime
                    .block_on(self.download_entry(&files, staging))
                    .map_err(io::Error::other)
            })
            .map_err(|error| format!("restoring the artifact: {error}"))?;
        Ok(true)
    }

    /// The files of the artifact stored for `key` that this machine trusts.
    async fn resolve(
        &self,
        key: &str,
        task: &TaskIdentity<'_>,
    ) -> Result<Option<Vec<ArtifactFile>>, String> {
        let resolved = self.store
            .resolve_artifacts(ResolveArtifactsOptions {
                candidates: vec![self.candidate(key, task)],
                supported_tags: Vec::new(),
                trusted_keys: self.trusted_keys.clone(),
                quarantined_envelope_digests: BTreeMap::new(),
                on_rejected_artifact: None,
                authorization: None,
                build_policy: ArtifactBuildPolicy {
                    eligible_packages: HashSet::new(),
                    allowed_builds: HashSet::new(),
                    ignore_scripts: false,
                },
            })
            .await
            .map_err(|error| error.to_string())?;
        let Some(VerifiedArtifact { payload, .. }) = resolved.into_values().next() else {
            return Ok(None);
        };
        Ok(Some(payload.manifest.added))
    }

    /// Download each blob of `files` once, straight into its file under the
    /// empty `staging` directory, and copy it to every other file it backs.
    /// Only `meta.json` and regular files under `outputs/` are accepted, so
    /// nothing an artifact carries can write outside `staging`.
    async fn download_entry(&self, files: &[ArtifactFile], staging: &Path) -> Result<(), String> {
        let first_by_integrity = prepare_staging(files, staging)?;
        stream::iter(first_by_integrity.values())
            .map(|file| async move {
                let request = ArtifactBlobRequest {
                    owner: self.owner.clone(),
                    integrity: file.integrity.clone(),
                };
                self.store
                    .download_artifact_blob_to(&request, file.size, &staging.join(&file.path))
                    .await
                    .map_err(|error| error.to_string())
            })
            .buffer_unordered(DOWNLOAD_CONCURRENCY)
            .try_collect::<()>()
            .await?;
        complete_staging(files, &first_by_integrity, staging).map_err(|error| error.to_string())
    }

    /// Start publishing `stored` under `key` when this machine publishes.
    /// The upload runs in the background, so a task waits for no upload;
    /// [`Self::finish_uploads`] waits for it.
    pub(super) fn upload(&self, key: &str, task: &TaskIdentity<'_>, stored: StoredTask) {
        let Some(publisher) = self.publisher.as_ref().map(Arc::clone) else {
            return;
        };
        let candidate = self.candidate(key, task);
        let store = Arc::clone(&self.store);
        let upload_slots = Arc::clone(&self.upload_slots);
        let upload = self.runtime.spawn(async move {
            let _slot = upload_slots.acquire_owned().await.map_err(|error| error.to_string())?;
            let request = tokio::task::spawn_blocking(move || publisher.sign(candidate, &stored))
                .await
                .map_err(|error| error.to_string())?
                .map_err(|error| format!("signing the artifact: {error}"))?;
            store.publish_artifact(&request).await.map_err(|error| error.to_string())
        });
        self.uploads
            .lock()
            .expect("upload list lock is not poisoned")
            .push(upload);
    }

    /// Wait for every upload [`Self::upload`] started, returning why each
    /// failed one did.
    pub(super) fn finish_uploads(&self) -> Vec<String> {
        let uploads =
            std::mem::take(&mut *self.uploads.lock().expect("upload list lock is not poisoned"));
        if uploads.is_empty() {
            return Vec::new();
        }
        // The caller may be a worker of the runtime the uploads run on,
        // which cannot block on it.
        std::thread::scope(|scope| {
            scope
                .spawn(|| self.runtime.block_on(join_uploads(uploads)))
                .join()
                .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
        })
    }

    fn candidate(&self, key: &str, task: &TaskIdentity<'_>) -> ArtifactCandidate {
        ArtifactCandidate {
            key: format!("{WORKSPACE_TASK_INPUT_KEY_PREFIX}{key}"),
            subject: ArtifactSubject::workspace_task(task.project, task.task),
            owner: self.owner.clone(),
        }
    }
}

async fn join_uploads(uploads: Vec<JoinHandle<Result<(), String>>>) -> Vec<String> {
    let mut failures = Vec::new();
    for upload in uploads {
        match upload.await {
            Ok(Ok(())) => {}
            Ok(Err(reason)) => failures.push(reason),
            Err(error) => failures.push(error.to_string()),
        }
    }
    failures
}

impl TaskPublisher {
    /// The local entry as a signed publication. The task key already covers
    /// the platform, so the artifact claims every machine.
    fn sign(
        &self,
        candidate: ArtifactCandidate,
        stored: &StoredTask,
    ) -> io::Result<ArtifactPublication> {
        let mut added = Vec::with_capacity(stored.files.len() + 1);
        let mut blobs = BTreeMap::new();
        let entry_files = std::iter::once("meta.json".to_string())
            .chain(
                stored.files
                    .iter()
                    .map(|file| format!("outputs/{file}")),
            );
        for path in entry_files {
            let (file, source) = artifact_file(&stored.entry_dir, path)?;
            blobs.entry(file.integrity.clone()).or_insert(source);
            added.push(file);
        }
        let payload = ArtifactPayload {
            kind: WORKSPACE_TASK_ARTIFACT_KIND.to_string(),
            subject: candidate.subject,
            input_key: candidate.key.clone(),
            owner: candidate.owner,
            builder_id: self.0.builder_id.clone(),
            builder_profile: self.0.builder_profile.clone(),
            compatibility: CompatibilityConstraints::Universal,
            manifest: ArtifactManifest { added, deleted: Vec::new() },
        };
        let envelope = self.0.sign(&payload).map_err(io::Error::other)?;
        Ok(ArtifactPublication {
            key: candidate.key,
            envelope,
            blobs: blobs.into_values().collect(),
        })
    }
}

/// One file of a local entry as the manifest lists it, and the file its
/// blob is uploaded from. The file is hashed as it is read, never held whole.
fn artifact_file(entry_dir: &Path, path: String) -> io::Result<(ArtifactFile, ArtifactBlobSource)> {
    let source = entry_dir.join(&path);
    let mode = if is_executable(&source)? { 0o755 } else { 0o644 };
    let mut reader = fs::File::open(&source)?;
    let mut hasher = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha512);
    let mut buffer = vec![0; 256 * 1024];
    let mut size = 0_u64;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.input(&buffer[..read]);
        size += read as u64;
    }
    let integrity = hasher.result().to_string();
    let file = ArtifactFile { integrity: integrity.clone(), mode, size, path };
    Ok((file, ArtifactBlobSource { integrity, size, path: source }))
}

fn is_executable(path: &Path) -> io::Result<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        Ok(pnpm_fs::file_mode::is_executable(fs::metadata(path)?.permissions().mode()))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(false)
    }
}

/// Check every file of an artifact and create the directories it goes in,
/// returning the first file each blob backs, which is the one downloaded.
fn prepare_staging<'files>(
    files: &'files [ArtifactFile],
    staging: &Path,
) -> Result<HashMap<&'files str, &'files ArtifactFile>, String> {
    let mut first_by_integrity = HashMap::new();
    for file in files {
        if !is_entry_path(Path::new(&file.path)) || !matches!(file.mode, 0o644 | 0o755) {
            return Err(format!("unexpected entry {}", file.path));
        }
        if let Some(parent) = staging.join(&file.path).parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        first_by_integrity.entry(file.integrity.as_str()).or_insert(file);
    }
    Ok(first_by_integrity)
}

/// Copy each downloaded blob to the other files it backs, and mark the
/// executable ones.
fn complete_staging(
    files: &[ArtifactFile],
    first_by_integrity: &HashMap<&str, &ArtifactFile>,
    staging: &Path,
) -> io::Result<()> {
    for file in files {
        let target = staging.join(&file.path);
        let first = first_by_integrity[file.integrity.as_str()];
        if !std::ptr::eq(first, file) {
            fs::copy(staging.join(&first.path), &target)?;
        }
        if file.mode == 0o755 {
            pnpm_fs::file_mode::make_file_executable(&fs::File::open(&target)?)?;
        }
    }
    Ok(())
}

fn is_entry_path(path: &Path) -> bool {
    let mut components = path.components();
    let first = components.next();
    let rest_is_normal = components
        .clone()
        .all(|component| matches!(component, Component::Normal(_)));
    match first {
        Some(Component::Normal(name)) if name == "meta.json" => components.next().is_none(),
        Some(Component::Normal(name)) if name == "outputs" => {
            rest_is_normal && components.next().is_some()
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests;
