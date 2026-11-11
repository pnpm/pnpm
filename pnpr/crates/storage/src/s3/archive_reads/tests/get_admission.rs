use crate::{BlobId, s3::S3Store};
use async_trait::async_trait;
use axum::body::to_bytes;
use futures_util::stream::BoxStream;
use object_store::{
    CopyOptions, GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore,
    ObjectStoreExt as _, PutMultipartOptions, PutOptions, PutPayload, PutResult, memory::InMemory,
    path::Path,
};
use pnpr_package_name::Ecosystem;
use std::{
    fmt,
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Semaphore;

#[derive(Debug)]
struct BlockingGetStore {
    inner: InMemory,
    started: Semaphore,
    resume: Semaphore,
    gets: AtomicUsize,
    conditions: Mutex<Vec<Option<String>>>,
}

impl fmt::Display for BlockingGetStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("blocking GET store")
    }
}

#[async_trait]
impl ObjectStore for BlockingGetStore {
    async fn get_opts(
        &self,
        location: &Path,
        options: GetOptions,
    ) -> object_store::Result<GetResult> {
        if !options.head {
            self.gets.fetch_add(1, Ordering::SeqCst);
            self.conditions
                .lock()
                .unwrap()
                .push(options.if_match.clone());
            self.started.add_permits(1);
            self.resume
                .acquire()
                .await
                .unwrap()
                .forget();
        }
        self.inner.get_opts(location, options).await
    }

    async fn put_opts(
        &self,
        location: &Path,
        payload: PutPayload,
        options: PutOptions,
    ) -> object_store::Result<PutResult> {
        self.inner.put_opts(location, payload, options).await
    }

    async fn put_multipart_opts(
        &self,
        location: &Path,
        options: PutMultipartOptions,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        self.inner.put_multipart_opts(location, options).await
    }

    fn delete_stream(
        &self,
        locations: BoxStream<'static, object_store::Result<Path>>,
    ) -> BoxStream<'static, object_store::Result<Path>> {
        self.inner.delete_stream(locations)
    }

    fn list(&self, prefix: Option<&Path>) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        self.inner.list(prefix)
    }

    async fn list_with_delimiter(&self, prefix: Option<&Path>) -> object_store::Result<ListResult> {
        self.inner.list_with_delimiter(prefix).await
    }

    async fn copy_opts(
        &self,
        from: &Path,
        to: &Path,
        options: CopyOptions,
    ) -> object_store::Result<()> {
        self.inner.copy_opts(from, to, options).await
    }
}

#[tokio::test]
async fn cold_gets_wait_for_admission_and_warm_gets_use_one_conditional_request() {
    let temp = tempfile::TempDir::new().unwrap();
    let backend = Arc::new(BlockingGetStore {
        inner: InMemory::new(),
        started: Semaphore::new(0),
        resume: Semaphore::new(0),
        gets: AtomicUsize::new(0),
        conditions: Mutex::default(),
    });
    let store = S3Store::new(
        Arc::clone(&backend) as Arc<dyn ObjectStore>,
        String::new(),
        temp.path().to_path_buf(),
    );
    let request_started = Arc::new(Semaphore::new(0));
    let mut tasks = Vec::new();
    for index in 0..5 {
        let bytes = format!("payload-{index}");
        let key = BlobId::of_bytes(Ecosystem::Npm, bytes.as_bytes()).key(1);
        backend.inner
            .put(&store.archive_key(&key), bytes.into_bytes().into())
            .await
            .unwrap();
        let request_started = Arc::clone(&request_started);
        let store = store.clone();
        tasks.push(tokio::spawn(async move {
            request_started.add_permits(1);
            store
                .open_archive(&key)
                .await
                .unwrap()
                .unwrap()
        }));
    }
    complete_before_deadline(request_started.acquire_many(5)).await.unwrap().forget();
    complete_before_deadline(backend.started.acquire_many(4)).await.unwrap().forget();
    assert_eq!(backend.gets.load(Ordering::SeqCst), 4);
    backend.resume.add_permits(5);
    for task in tasks {
        let (body, _) = complete_before_deadline(task).await.unwrap();
        assert!(!complete_before_deadline(to_bytes(body, usize::MAX)).await.unwrap().is_empty());
    }
    assert_eq!(backend.gets.load(Ordering::SeqCst), 5);
    let key = BlobId::of_bytes(Ecosystem::Npm, b"payload-0").key(1);
    backend.resume.add_permits(1);
    let (body, _) = complete_before_deadline(store.open_archive(&key)).await.unwrap().unwrap();
    assert_eq!(
        complete_before_deadline(to_bytes(body, usize::MAX)).await.unwrap(),
        b"payload-0".as_slice(),
    );
    assert_eq!(backend.gets.load(Ordering::SeqCst), 6);
    assert!(backend.conditions.lock().unwrap()[5].is_some());

    backend.inner
        .put(&store.archive_key(&key), b"corrupted".to_vec().into())
        .await
        .unwrap();
    backend.resume.add_permits(2);
    assert!(complete_before_deadline(store.open_archive(&key)).await.is_err());
    assert_eq!(backend.gets.load(Ordering::SeqCst), 8);
    let conditions = backend.conditions.lock().unwrap();
    assert!(conditions[6].is_some());
    assert!(conditions[7].is_none());
}

async fn complete_before_deadline<Output>(
    operation: impl Future<Output = Output> + Send,
) -> Output {
    tokio::time::timeout(Duration::from_secs(30), operation).await
        .expect("controlled object-store operation timed out")
}
