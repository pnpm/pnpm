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
        completion: None,
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
    backend.store.put(&backend.key(&id, &"b".repeat(32)), b"orphan".to_vec().into()).await.unwrap();
    assert_eq!(backend.sweep(std::time::Duration::ZERO).await.unwrap(), 1);
    assert!(backend.store.list(None).try_collect::<Vec<_>>().await.unwrap().is_empty());
}

#[tokio::test]
async fn unreadable_sessions_do_not_prevent_other_uploads_from_expiring() {
    use object_store::ObjectStoreExt;
    let disk = tempfile::TempDir::new().unwrap();
    let backend = RemoteUploadStore::new(Arc::new(InMemory::new()), "", disk.path().into());
    let unreadable = "a".repeat(32);
    let expired = "c".repeat(32);
    backend
        .store
        .put(&backend.key(&unreadable, "session.json"), b"corrupt".to_vec().into())
        .await
        .unwrap();
    backend
        .store
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
    assert!(backend.read(&expired).await.unwrap().is_none());
    assert_eq!(
        backend
            .store
            .get(&backend.key(&unreadable, "session.json"))
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap()
            .as_ref(),
        b"corrupt",
    );
    assert!(backend.store.head(&backend.key(&unreadable, &"b".repeat(32))).await.is_ok());
}
