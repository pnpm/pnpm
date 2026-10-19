use super::S3Store;
use crate::DocumentWrite;
use futures_util::StreamExt;
use object_store::{
    ObjectStoreExt, PutMode, PutOptions, PutPayload, UpdateVersion, path::Path as ObjectPath,
};
use pnpr_error::Result;

impl S3Store {
    // Records (see `storage::Storage` for the namespaces and the layout
    // contract shared with the fs backend).

    pub async fn read_record(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>> {
        match self.store.get(&self.record_key(namespace, key)).await {
            Ok(result) => {
                let bytes = result.bytes().await?;
                Ok(Some(bytes.to_vec()))
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub async fn create_record(&self, namespace: &str, key: &str, bytes: &[u8]) -> Result<bool> {
        match self.store.put_opts(
            &self.record_key(namespace, key),
            PutPayload::from(bytes.to_vec()),
            PutOptions { mode: PutMode::Create, ..PutOptions::default() },
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
        match self.store.put_opts(
            &key,
            PutPayload::from(bytes.to_vec()),
            PutOptions { mode: PutMode::Update(version), ..PutOptions::default() },
        )
        .await
        {
            Ok(_) => Ok(DocumentWrite::Written),
            Err(
                object_store::Error::AlreadyExists { .. }
                | object_store::Error::NotFound { .. }
                | object_store::Error::Precondition { .. },
            ) => Ok(DocumentWrite::Conflict),
            Err(err) => Err(err.into()),
        }
    }

    pub async fn remove_record(&self, namespace: &str, key: &str) -> Result<bool> {
        match self.store.delete(&self.record_key(namespace, key)).await {
            Ok(()) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    pub async fn remove_record_if_current(
        &self,
        namespace: &str,
        key: &str,
        expected: &[u8],
    ) -> Result<bool> {
        let key = self.record_key(namespace, key);
        let result = match self.store.get(&key).await {
            Ok(result) => result,
            Err(object_store::Error::NotFound { .. }) => return Ok(false),
            Err(err) => return Err(err.into()),
        };
        let version = UpdateVersion {
            e_tag: result.meta.e_tag.clone(),
            version: result.meta.version.clone(),
        };
        if result.bytes().await?.as_ref() != expected {
            return Ok(false);
        }
        match self.store.put_opts(
            &key,
            PutPayload::from(Vec::new()),
            PutOptions { mode: PutMode::Update(version), ..PutOptions::default() },
        )
        .await
        {
            Ok(_) => match self.store.delete(&key).await {
                Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(true),
                Err(err) => Err(err.into()),
            },
            Err(
                object_store::Error::AlreadyExists { .. }
                | object_store::Error::NotFound { .. }
                | object_store::Error::Precondition { .. },
            ) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    pub async fn list_record_keys(&self, namespace: &str) -> Result<Vec<String>> {
        let scope = format!("{}{namespace}/", self.prefix);
        let mut listing = self.store.list(Some(&ObjectPath::from(scope.as_str())));
        let mut keys = Vec::new();
        while let Some(meta) = listing.next().await {
            let meta = meta?;
            if meta.location.as_ref().ends_with('/') {
                continue;
            }
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
