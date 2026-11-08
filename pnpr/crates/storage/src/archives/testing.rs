//! Storing a file of a package the way a publish does, for tests.

use crate::{PromotedArchive, Storage};
use pnpr_package_name::CanonicalPackageName;

/// Store `bytes` as the file `file` of `name` and map the file to them,
/// unless the file is mapped already: the slot, identity, row, promotion,
/// and mapping a publish of one file goes through.
pub(crate) async fn store_file(
    storage: &Storage,
    name: &CanonicalPackageName,
    file: &str,
    bytes: &[u8],
) -> PromotedArchive {
    let promoted = promote_file(storage, name, file, bytes).await;
    storage.map_hosted_artifact(promoted.artifact.clone()).await.unwrap();
    promoted
}

/// [`store_file`] without the mapping, which the caller makes or leaves out.
pub(crate) async fn promote_file(
    storage: &Storage,
    name: &CanonicalPackageName,
    file: &str,
    bytes: &[u8],
) -> PromotedArchive {
    let slot = storage.reserve_hosted_blob(name, file).await.unwrap();
    tokio::fs::write(&slot.tmp_path, bytes).await.unwrap();
    let (id, size) = storage.identify_blob_slot(&slot).await.unwrap();
    let rows = storage
        .blob_rows(vec![id.clone()])
        .await
        .unwrap();
    let row = rows.get(&id);
    storage
        .promote_blob_slot(slot, (id, size), row)
        .await
        .unwrap()
}

/// The bytes `storage` serves for the file `file` of `name`.
pub(crate) async fn served(
    storage: &Storage,
    name: &CanonicalPackageName,
    file: &str,
) -> Option<Vec<u8>> {
    let (body, _) = storage.open_hosted_blob(name, file).await.unwrap()?;
    Some(axum::body::to_bytes(body, usize::MAX).await.unwrap().to_vec())
}
