use super::{MAX_CHUNKS, RemoteUploadStore, UploadRecord, VersionedRecord, same_object_version};
use futures_util::stream::BoxStream;
use object_store::{
    CopyOptions, GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore,
    PutMode, PutMultipartOptions, PutOptions, PutPayload, PutResult, UpdateVersion,
    memory::InMemory, path::Path,
};
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use std::sync::Arc;

#[tokio::test]
async fn a_full_chunk_list_still_accepts_an_empty_completion_request() {
    let disk = tempfile::TempDir::new().unwrap();
    let backend = RemoteUploadStore::new(Arc::new(InMemory::new()), "", disk.path().into());
    let id = "a".repeat(32);
    let record = UploadRecord {
        repository: "app".into(),
        chunks: vec!["b".repeat(32); MAX_CHUNKS],
        size: 0,
        completion: None,
        closed: false,
    };
    let version = backend.write(&id, &record, PutMode::Create).await.unwrap();
    let upload = backend
        .handle(&id, VersionedRecord { record, version })
        .await
        .unwrap();
    assert_eq!(
        upload
            .append()
            .await
            .unwrap()
            .finish()
            .await
            .unwrap(),
        0,
    );
    let mut writer = upload.append().await.unwrap();
    writer.write_all(b"overflow").await.unwrap();
    assert!(writer.finish().await.is_err());
    assert_eq!(upload.offset().await.unwrap(), 0);
}

#[tokio::test]
async fn expiry_removes_unrecorded_chunks_even_when_they_are_listed_first() {
    use futures_util::TryStreamExt;
    use object_store::ObjectStoreExt;
    let disk = tempfile::TempDir::new().unwrap();
    let backend = RemoteUploadStore::new(Arc::new(InMemory::new()), "", disk.path().into());
    let id = "a".repeat(32);
    let record = UploadRecord {
        repository: "app".into(),
        chunks: Vec::new(),
        size: 0,
        completion: None,
        closed: false,
    };
    backend.write(&id, &record, PutMode::Create).await.unwrap();
    backend.store
        .put(&backend.key(&id, &"b".repeat(32)), b"orphan".to_vec().into())
        .await
        .unwrap();
    assert_eq!(backend.sweep(std::time::Duration::ZERO).await.unwrap(), 1);
    assert!(
        backend.store
            .list(None)
            .try_collect::<Vec<_>>()
            .await
            .unwrap()
            .is_empty(),
    );
}

#[tokio::test]
async fn unreadable_sessions_do_not_prevent_other_uploads_from_expiring() {
    use object_store::ObjectStoreExt;
    let disk = tempfile::TempDir::new().unwrap();
    let backend = RemoteUploadStore::new(Arc::new(InMemory::new()), "", disk.path().into());
    let unreadable = "a".repeat(32);
    let expired = "c".repeat(32);
    backend.store
        .put(&backend.key(&unreadable, "session.json"), b"corrupt".to_vec().into())
        .await
        .unwrap();
    backend.store
        .put(&backend.key(&unreadable, &"b".repeat(32)), b"keep".to_vec().into())
        .await
        .unwrap();
    let record = UploadRecord {
        repository: "app".into(),
        chunks: Vec::new(),
        size: 0,
        completion: None,
        closed: false,
    };
    backend.write(&expired, &record, PutMode::Create).await.unwrap();
    assert_eq!(backend.sweep(std::time::Duration::ZERO).await.unwrap(), 1);
    assert!(
        backend
            .read(&expired)
            .await
            .unwrap()
            .is_none(),
    );
    assert_eq!(
        backend.store
            .get(&backend.key(&unreadable, "session.json"))
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap()
            .as_ref(),
        b"corrupt",
    );
    assert!(
        backend.store
            .head(&backend.key(&unreadable, &"b".repeat(32)))
            .await
            .is_ok(),
    );
}

/// Reports a version id on PUT but not on GET, as Cloudflare R2 does.
#[derive(Debug)]
struct VersionOnlyOnPutStore(InMemory);

impl std::fmt::Display for VersionOnlyOnPutStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "VersionOnlyOnPutStore")
    }
}

#[async_trait::async_trait]
impl ObjectStore for VersionOnlyOnPutStore {
    async fn put_opts(
        &self,
        location: &Path,
        payload: PutPayload,
        opts: PutOptions,
    ) -> object_store::Result<PutResult> {
        let result = self.0.put_opts(location, payload, opts).await?;
        Ok(PutResult { version: result.e_tag.clone(), ..result })
    }

    async fn put_multipart_opts(
        &self,
        location: &Path,
        opts: PutMultipartOptions,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        self.0.put_multipart_opts(location, opts).await
    }

    async fn get_opts(
        &self,
        location: &Path,
        options: GetOptions,
    ) -> object_store::Result<GetResult> {
        let result = self.0.get_opts(location, options).await?;
        assert_eq!(result.meta.version, None);
        Ok(result)
    }

    fn delete_stream(
        &self,
        locations: BoxStream<'static, object_store::Result<Path>>,
    ) -> BoxStream<'static, object_store::Result<Path>> {
        self.0.delete_stream(locations)
    }

    fn list(&self, prefix: Option<&Path>) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        self.0.list(prefix)
    }

    async fn list_with_delimiter(&self, prefix: Option<&Path>) -> object_store::Result<ListResult> {
        self.0.list_with_delimiter(prefix).await
    }

    async fn copy_opts(
        &self,
        from: &Path,
        to: &Path,
        options: CopyOptions,
    ) -> object_store::Result<()> {
        self.0.copy_opts(from, to, options).await
    }
}

#[tokio::test]
async fn an_empty_chunk_is_accepted_when_reads_omit_the_version_id() {
    let disk = tempfile::TempDir::new().unwrap();
    let backend = RemoteUploadStore::new(
        Arc::new(VersionOnlyOnPutStore(InMemory::new())),
        "",
        disk.path().into(),
    );
    let repository = CanonicalPackageName::parse("app", Ecosystem::Oci).unwrap();
    let upload = backend.begin(&repository).await.unwrap();
    assert_eq!(
        upload
            .append()
            .await
            .unwrap()
            .finish()
            .await
            .unwrap(),
        0,
    );
    let mut writer = upload.append().await.unwrap();
    writer.write_all(b"bytes").await.unwrap();
    assert_eq!(writer.finish().await.unwrap(), 5);
    assert_eq!(
        upload
            .append()
            .await
            .unwrap()
            .finish()
            .await
            .unwrap(),
        5,
    );
}

fn observed(e_tag: Option<&str>, version: Option<&str>) -> UpdateVersion {
    UpdateVersion { e_tag: e_tag.map(Into::into), version: version.map(Into::into) }
}

#[test]
fn version_ids_decide_when_both_observations_carry_one() {
    assert!(same_object_version(&observed(Some("a"), Some("1")), &observed(Some("b"), Some("1"))));
    assert!(!same_object_version(&observed(Some("a"), Some("1")), &observed(Some("a"), Some("2"))));
}

#[test]
fn etags_decide_when_either_observation_lacks_a_version_id() {
    assert!(same_object_version(&observed(Some("a"), Some("1")), &observed(Some("a"), None)));
    assert!(!same_object_version(&observed(Some("a"), Some("1")), &observed(Some("b"), None)));
}
