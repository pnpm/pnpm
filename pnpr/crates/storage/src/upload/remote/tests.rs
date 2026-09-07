use super::{MAX_CHUNKS, RemoteUploadStore, UploadRecord, VersionedRecord};
use object_store::{PutMode, memory::InMemory};
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
        closed: false,
    };
    let version = backend.write(&id, &record, PutMode::Create).await.unwrap();
    let upload = backend.handle(&id, VersionedRecord { record, version }).await.unwrap();
    assert_eq!(upload.append().await.unwrap().finish().await.unwrap(), 0);
    let mut writer = upload.append().await.unwrap();
    writer.write_all(b"overflow").await.unwrap();
    assert!(writer.finish().await.is_err());
    assert_eq!(upload.offset().await.unwrap(), 0);
}
