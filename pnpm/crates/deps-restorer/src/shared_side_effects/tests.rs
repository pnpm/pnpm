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
    for (bytes, mode) in [
        (b"executable addon".as_slice(), 0o755),
        (b"plain addon".as_slice(), 0o644),
        (b"owner-executable addon".as_slice(), 0o744),
    ] {
        let (written, _) =
            store_dir.write_cas_file(bytes, pnpm_fs::file_mode::is_executable(mode)).unwrap();
        let integrity = format!("sha512-{}", BASE64.encode(Sha512::digest(bytes)));
        let digest = pnpm_pnpr_client::blob_id(&integrity).unwrap();

        let located = store_dir
            .cas_file_path_by_mode(&digest, mode)
            .expect("an artifact digest must address a CAS path");
        assert_eq!(located, written, "mode {mode:o}");
        assert!(located.is_file(), "mode {mode:o}");
    }
}
