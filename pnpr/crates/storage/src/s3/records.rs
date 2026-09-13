use super::{
    DocumentWrite, ObjectPath, ObjectStoreExt, PutMode, PutOptions, PutPayload, Result, S3Store,
    StreamExt, UpdateVersion,
};

impl S3Store {
    pub async fn read_record(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>> {
        match self.store.get(&self.record_key(namespace, key)).await {
            Ok(result) => Ok(Some(result.bytes().await?.to_vec())),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub async fn create_record(&self, namespace: &str, key: &str, bytes: &[u8]) -> Result<bool> {
        match self.store.put_opts(
            &self.record_key(namespace, key),
            PutPayload::from(bytes.to_vec()),
            PutOptions {
                mode: PutMode::Create,
                ..PutOptions::default()
            },
        )
        .await
        {
            Ok(_) => Ok(true),
            Err(
                object_store::Error::AlreadyExists { .. }
                | object_store::Error::Precondition { .. },
            ) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    /// Rewrite a record under `If-Match` on the version the caller read, so
    /// only one replica can claim a record whose copy is current. The bytes
    /// are compared as well: a rewrite the caller computed from something
    /// other than what the bucket holds is a conflict even where the version
    /// still matches.
    pub async fn replace_record_if_current(
        &self,
        namespace: &str,
        key: &str,
        expected: &[u8],
        bytes: &[u8],
    ) -> Result<DocumentWrite> {
        let key = self.record_key(namespace, key);
        let result = match self.store.get(&key).await {
            Ok(result) => result,
            // Gone: an approval that finished, or a rejection. A store that
            // failed for any other reason is an error, not a conflict.
            Err(object_store::Error::NotFound { .. }) => return Ok(DocumentWrite::Conflict),
            Err(err) => return Err(err.into()),
        };
        let version = UpdateVersion {
            e_tag: result.meta.e_tag.clone(),
            version: result.meta.version.clone(),
        };
        if result.bytes().await?.as_ref() != expected {
            return Ok(DocumentWrite::Conflict);
        }
        if self.write_object_if_current(&key, bytes, Some(&version)).await? {
            Ok(DocumentWrite::Written)
        } else {
            Ok(DocumentWrite::Conflict)
        }
    }

    pub async fn remove_record(&self, namespace: &str, key: &str) -> Result<bool> {
        match self.store.delete(&self.record_key(namespace, key)).await {
            Ok(()) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    pub async fn list_record_keys(&self, namespace: &str) -> Result<Vec<String>> {
        let scope = format!("{}{namespace}/", self.prefix);
        let mut listing = self.store.list(Some(&ObjectPath::from(scope.as_str())));
        let mut keys = Vec::new();
        while let Some(meta) = listing.next().await {
            let meta = meta?;
            // `ObjectPath` normalizes what it is built from, so compare
            // against the same normalization rather than the raw prefix.
            let Some(key) = meta.location
                .as_ref()
                .strip_prefix(ObjectPath::from(scope.as_str()).as_ref())
            else {
                continue;
            };
            keys.push(key.trim_start_matches('/').to_string());
        }
        Ok(keys)
    }

    fn record_key(&self, namespace: &str, key: &str) -> ObjectPath {
        ObjectPath::from(format!("{}{namespace}/{key}", self.prefix))
    }
}
