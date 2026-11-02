//! Metadata kept as one document per package in the hosted store.

use super::{PackageMetadata, PackageView, TagChange, TagUpdate};
use crate::{
    DOCUMENT_WRITE_RETRIES, DocumentUpdate, DocumentWrite, HostedBackend, HostedDocumentForUpdate,
    HostedDocumentVersion,
    publish::{PACKUMENT_FORMAT_VERSION, now_iso},
    wait_after_document_write_conflict,
};
use async_trait::async_trait;
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::CanonicalPackageName;
use serde_json::{Map, Value};
use std::sync::Arc;

#[derive(Debug)]
pub(super) struct DocumentMetadata {
    hosted: Arc<dyn HostedBackend>,
}

impl DocumentMetadata {
    pub(super) fn new(hosted: Arc<dyn HostedBackend>) -> Self {
        Self { hosted }
    }
}

#[async_trait]
impl PackageMetadata for DocumentMetadata {
    async fn read_view(&self, name: &CanonicalPackageName) -> Result<Option<PackageView>> {
        let bytes = self.hosted.read_document(name).await?;
        Ok(bytes.map(|bytes| PackageView { bytes: bytes.into(), generation: None }))
    }

    async fn read_for_update(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<HostedDocumentForUpdate>> {
        self.hosted.read_document_for_update(name).await
    }

    async fn write_if_current(
        &self,
        name: &CanonicalPackageName,
        bytes: &[u8],
        version: Option<&HostedDocumentVersion>,
    ) -> Result<DocumentWrite> {
        self.hosted.write_document_if_current(name, bytes, version).await
    }

    async fn change_tag(
        &self,
        name: &CanonicalPackageName,
        change: TagChange<'_>,
    ) -> Result<TagUpdate> {
        let mut moved = false;
        let update = update_with_retry(self, name, DOCUMENT_WRITE_RETRIES, |existing| {
            retag_packument(existing, change, &mut moved)
        })
        .await?;
        Ok(match update {
            DocumentUpdate::Written => TagUpdate::Committed { generation: None, moved },
            DocumentUpdate::NotFound => TagUpdate::NotFound,
        })
    }

    async fn remove_package(&self, _name: &CanonicalPackageName) -> Result<bool> {
        Ok(false)
    }

    async fn package_names(&self) -> Result<Vec<String>> {
        self.hosted.list_package_names().await
    }
}

/// Read a document, transform it, and write it back while the store still
/// holds what was read, retrying on conflict with capped backoff.
///
/// `build` receives the current bytes (`None` when the document is absent)
/// and returns the bytes to write, or `Ok(None)` to stop with
/// [`DocumentUpdate::NotFound`]; a `build` error stops without retrying. After
/// `retries` conflicts the write fails with
/// [`RegistryError::DocumentWriteConflict`].
pub(crate) async fn update_with_retry<Build>(
    metadata: &dyn PackageMetadata,
    name: &CanonicalPackageName,
    retries: usize,
    mut build: Build,
) -> Result<DocumentUpdate>
where
    Build: FnMut(Option<&[u8]>) -> Result<Option<Vec<u8>>>,
{
    for attempt in 0..retries {
        let (existing, version) = match metadata.read_for_update(name).await? {
            Some(document) => (Some(document.bytes), Some(document.version)),
            None => (None, None),
        };
        let Some(new_bytes) = build(existing.as_deref())? else {
            return Ok(DocumentUpdate::NotFound);
        };
        match metadata.write_if_current(name, &new_bytes, version.as_ref()).await? {
            DocumentWrite::Written => return Ok(DocumentUpdate::Written),
            DocumentWrite::Conflict if attempt + 1 < retries => {
                wait_after_document_write_conflict(attempt).await;
            }
            DocumentWrite::Conflict => {}
        }
    }
    Err(RegistryError::DocumentWriteConflict { package: name.as_str().to_string() })
}

/// The stored packument with `change` applied to its `dist-tags` and
/// `time.modified` stamped, or `None` when no packument is stored. Sets
/// `moved` when the tag pointed elsewhere.
fn retag_packument(
    existing: Option<&[u8]>,
    change: TagChange<'_>,
    moved: &mut bool,
) -> Result<Option<Vec<u8>>> {
    let Some(bytes) = existing else { return Ok(None) };
    let mut packument: Value = serde_json::from_slice(bytes)?;
    check_format_version(&packument)?;
    let packument_obj = packument.as_object_mut().ok_or_else(|| not_an_object("packument"))?;
    packument_obj.insert("formatVersion".to_string(), Value::from(PACKUMENT_FORMAT_VERSION));
    let tags = object_entry(packument_obj, "dist-tags")?;
    let previous = match change.target {
        Some(version) => tags.insert(change.tag.to_string(), Value::String(version.to_string())),
        None => tags.remove(change.tag),
    };
    *moved = previous.as_ref().and_then(Value::as_str) != change.target;
    object_entry(packument_obj, "time")?.insert("modified".to_string(), Value::String(now_iso()));
    Ok(Some(serde_json::to_vec_pretty(&packument)?))
}

/// Refuse a packument written by a newer pnpr.
pub(crate) fn check_format_version(packument: &Value) -> Result<()> {
    let Some(format_version) = packument.get("formatVersion").and_then(Value::as_u64) else {
        return Ok(());
    };
    if format_version <= u64::from(PACKUMENT_FORMAT_VERSION) {
        return Ok(());
    }
    Err(RegistryError::DocumentFormatTooNew {
        document: "packument".to_string(),
        format_version: u32::try_from(format_version).unwrap_or(u32::MAX),
        supported_version: PACKUMENT_FORMAT_VERSION,
    })
}

/// The object stored under `key`, inserting an empty one when absent.
fn object_entry<'map>(
    packument: &'map mut Map<String, Value>,
    key: &str,
) -> Result<&'map mut Map<String, Value>> {
    packument
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| not_an_object(key))
}

fn not_an_object(what: &str) -> RegistryError {
    RegistryError::BadRequest { reason: format!("stored {what} is not an object") }
}
