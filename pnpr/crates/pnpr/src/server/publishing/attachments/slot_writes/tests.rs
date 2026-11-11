use super::{AttachmentPaths, TarballStorage};
use tempfile::TempDir;

#[tokio::test]
async fn tar_registry_attachment_temp_matches_stale_sweep() {
    let dir = TempDir::new().unwrap();
    let base = dir.path().join("archive");
    let slot = pnpr_storage::unique_tmp_path(&base);

    let tgz_paths = AttachmentPaths::for_slot(&slot, TarballStorage::Tgz);
    assert_eq!(tgz_paths.attachment, slot);
    assert!(tgz_paths.tar.is_none());

    let paths = AttachmentPaths::for_slot(&slot, TarballStorage::Tar);
    assert_ne!(paths.attachment, slot);
    assert_eq!(paths.tar.as_deref(), Some(slot.as_path()));
    tokio::fs::write(&paths.attachment, b"partial-tgz").await.unwrap();
    tokio::fs::write(&slot, b"partial-tar").await.unwrap();

    let swept = pnpr_storage::sweep_stale_temps(&base, std::time::Duration::ZERO)
        .await
        .unwrap();
    assert_eq!(swept.files, 2);
    assert!(!paths.attachment.exists());
    assert!(!slot.exists());
}
