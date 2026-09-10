use super::{
    ARTIFACT_OBJECT_PREFIX, ARTIFACT_QUOTA_OBJECT, ARTIFACT_USAGE_FILE, Arc, BoxStream,
    MAX_GLOBAL_ARTIFACT_BYTES, MAX_OWNER_ARTIFACT_BYTES, ObjectMeta, ObjectPath, ObjectStore,
    ObjectStoreExt, PutMode, PutOptions, PutPayload, QuotaCoordination, Result,
    SharedArtifactStore, is_create_conflict, stored_object_too_large,
};

impl SharedArtifactStore {
    pub(super) fn object_store(store: Arc<dyn ObjectStore>, prefix: &str) -> Self {
        Self {
            store,
            prefix: format!("{prefix}{ARTIFACT_OBJECT_PREFIX}/"),
            quota: QuotaCoordination::Conditional,
            owner_limit: MAX_OWNER_ARTIFACT_BYTES,
            global_limit: MAX_GLOBAL_ARTIFACT_BYTES,
        }
    }

    pub(super) async fn create_object(
        &self,
        relative: &str,
        bytes: impl Into<PutPayload>,
    ) -> Result<bool> {
        match self
            .store
            .put_opts(
                &self.object_path(relative),
                bytes.into(),
                PutOptions { mode: PutMode::Create, ..PutOptions::default() },
            )
            .await
        {
            Ok(_) => Ok(true),
            Err(error) if is_create_conflict(&error) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    pub(super) async fn read_object_bounded(
        &self,
        relative: &str,
        max_size: u64,
    ) -> Result<Option<Vec<u8>>> {
        match self.store.get(&self.object_path(relative)).await {
            Ok(result) => {
                if result.meta.size > max_size {
                    return Err(stored_object_too_large(result.meta.size, max_size));
                }
                Ok(Some(result.bytes().await?.to_vec()))
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub(super) async fn read_object_path(&self, path: &ObjectPath) -> Result<Option<Vec<u8>>> {
        match self.store.get(path).await {
            Ok(result) => Ok(Some(result.bytes().await?.to_vec())),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub(super) fn list_objects(
        &self,
        relative_prefix: Option<&str>,
    ) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        let prefix = relative_prefix.map(|prefix| self.object_path(prefix)).or_else(|| {
            (!self.prefix.is_empty()).then(|| ObjectPath::from(self.prefix.trim_end_matches('/')))
        });
        self.store.list(prefix.as_ref())
    }

    pub(super) fn object_path(&self, relative: &str) -> ObjectPath {
        ObjectPath::from(format!("{}{relative}", self.prefix))
    }

    pub(super) fn relative_path<'a>(&self, path: &'a ObjectPath) -> Option<&'a str> {
        path.as_ref().strip_prefix(&self.prefix)
    }

    pub(super) fn quota_object(&self) -> &str {
        match &self.quota {
            QuotaCoordination::Local { .. } => ARTIFACT_USAGE_FILE,
            QuotaCoordination::Conditional => ARTIFACT_QUOTA_OBJECT,
        }
    }
}
