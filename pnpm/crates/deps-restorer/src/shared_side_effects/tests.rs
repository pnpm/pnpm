use super::package_version;
use pnpm_lockfile::PackageKey;

#[test]
fn package_identity_uses_the_manifest_version_for_non_registry_sources() {
    let file: PackageKey = "native-addon@file:../native-addon.tgz".parse().unwrap();
    assert_eq!(package_version(&file, Some("1.0.0")), "1.0.0");

    let registry: PackageKey = "native-addon@2.0.0".parse().unwrap();
    assert_eq!(package_version(&registry, None), "2.0.0");
}

/// The skip that keeps a restore from re-fetching content the store already
/// has depends on the artifact's digest and mode naming the same CAS path the
/// store wrote. Nothing else would notice them drifting apart: the lookup
/// would simply always miss and every install would download again.
///
/// `0o744` is here even though a manifest carrying it is refused before
/// hydration ever sees it. The two sides must agree on their own terms rather
/// than because a validator elsewhere happens to allow only two modes: an
/// owner-executable file would otherwise be looked up as executable and
/// written as not, and the permission a restore installs would depend on what
/// the store already held.
#[test]
fn an_artifact_digest_addresses_the_file_the_store_wrote() {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    use sha2::{Digest as _, Sha512};

    let store = tempfile::tempdir().unwrap();
    let store_dir = pnpm_store_dir::StoreDir::new(store.path());
    // One content, so only the mode can make the paths differ.
    let bytes = b"addon".as_slice();
    let integrity = format!("sha512-{}", BASE64.encode(Sha512::digest(bytes)));
    let digest = pnpm_pnpr_client::blob_id(&integrity).unwrap();
    let mut located_by_mode = Vec::new();
    for mode in [0o755, 0o644, 0o744] {
        let (written, _) =
            store_dir.write_cas_file(bytes, pnpm_fs::file_mode::is_executable(mode)).unwrap();
        let located = store_dir
            .cas_file_path_by_mode(&digest, mode)
            .expect("an artifact digest must address a CAS path");
        assert_eq!(located, written, "mode {mode:o}");
        assert!(located.is_file(), "mode {mode:o}");
        located_by_mode.push((mode, located));
    }

    let executable = &located_by_mode[0].1;
    let plain = &located_by_mode[1].1;
    assert_ne!(executable, plain, "one content must not share a path across modes");
    assert_eq!(&located_by_mode[2].1, executable, "any executable bit files as executable");
}

/// Reuse answers to `verifyStoreIntegrity`: content that does not hash to the
/// digest it is filed under is not offered, so the caller falls back to its own
/// verified download rather than installing whatever happens to sit there.
#[tokio::test]
async fn corrupted_store_content_is_not_reused() {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    use sha2::{Digest as _, Sha512};

    let store = tempfile::tempdir().unwrap();
    let store_dir = pnpm_store_dir::StoreDir::new(store.path());
    let bytes = b"addon".as_slice();
    let (path, _) = store_dir.write_cas_file(bytes, true).unwrap();
    let integrity = format!("sha512-{}", BASE64.encode(Sha512::digest(bytes)));
    let digest = pnpm_pnpr_client::blob_id(&integrity).unwrap();
    assert!(super::store_holds(&path, &digest).await.unwrap());

    std::fs::write(&path, b"tampered").unwrap();
    assert!(
        !super::store_holds(&path, &digest).await.unwrap(),
        "the write this skips checks the destination whatever verifyStoreIntegrity says",
    );

    std::fs::remove_file(&path).unwrap();
    assert!(!super::store_holds(&path, &digest).await.unwrap());
}
