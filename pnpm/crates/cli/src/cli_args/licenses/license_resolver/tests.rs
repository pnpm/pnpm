use super::{MAX_LICENSE_FILE_SIZE, detect_license_from_text, resolve_license_from_dir};
use tempfile::TempDir;

#[test]
fn detects_known_license_names_in_text() {
    assert_eq!(detect_license_from_text("(The MIT License)"), Some("MIT".to_string()));
    assert_eq!(
        detect_license_from_text("Apache-2.0 or MIT"),
        Some("Apache-2.0 OR MIT".to_string()),
    );
    assert_eq!(detect_license_from_text("custom terms"), None);
}

#[tokio::test]
async fn resolves_license_file_when_manifest_has_no_license() {
    let dir = TempDir::new().unwrap();
    tokio::fs::write(dir.path().join("LICENSE"), "(The MIT License)").await.unwrap();

    assert_eq!(resolve_license_from_dir(None, dir.path()).await, Some("MIT".to_string()));
    assert_eq!(
        resolve_license_from_dir(Some("SEE LICENSE IN LICENSE".to_string()), dir.path()).await,
        Some("MIT".to_string()),
    );

    tokio::fs::write(dir.path().join("LICENSE"), "custom terms").await.unwrap();
    assert_eq!(
        resolve_license_from_dir(Some("SEE LICENSE IN LICENSE".to_string()), dir.path()).await,
        Some("Unknown".to_string()),
    );
}

#[tokio::test]
async fn skips_non_file_license_candidates() {
    let dir = TempDir::new().unwrap();
    tokio::fs::create_dir(dir.path().join("LICENSE")).await.unwrap();
    tokio::fs::write(dir.path().join("LICENCE"), "(The MIT License)").await.unwrap();

    assert_eq!(resolve_license_from_dir(None, dir.path()).await, Some("MIT".to_string()));
}

#[tokio::test]
async fn bounds_license_file_reads() {
    let dir = TempDir::new().unwrap();
    tokio::fs::write(dir.path().join("LICENSE"), vec![b'M'; MAX_LICENSE_FILE_SIZE + 1])
        .await
        .unwrap();
    tokio::fs::write(dir.path().join("LICENCE"), "Apache-2.0").await.unwrap();

    assert_eq!(resolve_license_from_dir(None, dir.path()).await, Some("Apache-2.0".to_string()));
}

#[cfg(unix)]
#[tokio::test]
async fn does_not_follow_license_file_symlinks() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    tokio::fs::write(outside.path().join("LICENSE"), "(The MIT License)").await.unwrap();
    symlink(outside.path().join("LICENSE"), dir.path().join("LICENSE")).unwrap();
    tokio::fs::write(dir.path().join("LICENCE"), "Apache-2.0").await.unwrap();

    assert_eq!(resolve_license_from_dir(None, dir.path()).await, Some("Apache-2.0".to_string()));
}
