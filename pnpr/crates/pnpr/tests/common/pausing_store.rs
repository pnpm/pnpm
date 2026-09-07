use async_trait::async_trait;
use futures_util::stream::BoxStream;
use object_store::{
    CopyOptions, GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore,
    PutMultipartOptions, PutOptions, PutPayload, PutResult, memory::InMemory, path::Path,
};
use std::{fmt, sync::Mutex};
use tokio::sync::Notify;

#[derive(Debug, Default)]
pub struct PausingStore {
    inner: InMemory,
    filename: Mutex<Option<String>>,
    write_filename: Mutex<Option<String>>,
    pub started: Notify,
    pub resume: Notify,
}

impl PausingStore {
    pub fn pause_write(&self, filename: String) {
        *self.write_filename.lock().unwrap() = Some(filename);
    }

    pub fn pause(&self, filename: String) {
        *self.filename.lock().unwrap() = Some(filename);
    }
}

impl fmt::Display for PausingStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("pausing object store")
    }
}

#[async_trait]
impl ObjectStore for PausingStore {
    async fn get_opts(
        &self,
        location: &Path,
        options: GetOptions,
    ) -> object_store::Result<GetResult> {
        let pause = {
            let mut filename = self.filename.lock().unwrap();
            let pause = filename
                .as_ref()
                .is_some_and(|filename| location.filename() == Some(filename.as_str()));
            if pause {
                *filename = None;
            }
            pause
        };
        if pause {
            self.started.notify_one();
            self.resume.notified().await;
        }
        self.inner.get_opts(location, options).await
    }

    async fn put_opts(
        &self,
        location: &Path,
        payload: PutPayload,
        options: PutOptions,
    ) -> object_store::Result<PutResult> {
        let pause = {
            let mut filename = self.write_filename.lock().unwrap();
            let pause = filename
                .as_ref()
                .is_some_and(|filename| location.filename() == Some(filename.as_str()));
            if pause {
                *filename = None;
            }
            pause
        };
        if pause {
            self.started.notify_one();
            self.resume.notified().await;
        }
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
