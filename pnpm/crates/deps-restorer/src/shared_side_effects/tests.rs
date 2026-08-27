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

/// Content that does not hash to the digest it is filed under is not offered,
/// so the caller falls back to its own verified download rather than
/// installing whatever happens to sit there.
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

/// The store addresses its own regular files, so anything else at the digest
/// path is a miss the download and CAS write can repair.
#[tokio::test]
async fn a_non_regular_file_is_not_reused_as_store_content() {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    use sha2::{Digest as _, Sha512};

    let store = tempfile::tempdir().unwrap();
    let store_dir = pnpm_store_dir::StoreDir::new(store.path());
    let bytes = b"addon".as_slice();
    let (path, _) = store_dir.write_cas_file(bytes, true).unwrap();
    let integrity = format!("sha512-{}", BASE64.encode(Sha512::digest(bytes)));
    let digest = pnpm_pnpr_client::blob_id(&integrity).unwrap();

    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(!super::store_holds(&path, &digest).await.unwrap());

    #[cfg(unix)]
    {
        let fifo = store.path().join("fifo");
        assert!(
            std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success(),
            "mkfifo is needed to plant a FIFO at a store path",
        );
        assert!(
            !super::store_holds(&fifo, &digest).await.unwrap(),
            "a FIFO must be refused rather than held open waiting for a writer",
        );

        let outside = store.path().join("outside");
        std::fs::write(&outside, bytes).unwrap();
        std::fs::remove_dir(&path).unwrap();
        std::os::unix::fs::symlink(&outside, &path).unwrap();
        assert!(
            !super::store_holds(&path, &digest).await.unwrap(),
            "a link naming correct bytes still imports content the store does not own",
        );
    }
}

/// A restore only happens where the remote cache applies at all:
/// `linux_glibc_platform` refuses anything but linux-glibc on x64 or arm64.
/// The cfg spells out that same contract, so on a host the feature cannot
/// serve — musl, or a linux arch pacquet publishes no artifacts for — these
/// tests are absent rather than failing on a platform they never described.
#[cfg(all(
    target_os = "linux",
    target_env = "gnu",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod restore {
    use crate::{AllowBuildPolicy, RequiresBuildBySnapshot, SideEffectsMapsBySnapshot};
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    use p256::{
        SecretKey,
        pkcs8::{EncodePrivateKey as _, EncodePublicKey as _},
    };
    use pnpm_config::{Config, RemoteSideEffectsCacheSettings};
    use pnpm_lockfile::{PackageKey, PackageMetadata, SnapshotEntry};
    use pnpm_pnpr_client::{
        ARTIFACT_KIND, ArtifactFile, ArtifactManifest, ArtifactPayload, BuilderProfile,
        CompatibilityConstraints, OwnerScope, ResolveArtifactsRequest, SignedArtifactEnvelope,
        linux_glibc_supported_tags,
    };
    use pnpm_shared_artifact_protocol::{
        ArtifactVariant, ResolveArtifactsResponse, ResolvedArtifact,
    };
    use pnpm_store_dir::StoreDir;
    use sha2::{Digest as _, Sha512};
    use std::{
        collections::{BTreeMap, HashMap, HashSet},
        path::PathBuf,
    };

    const PACKAGE: &str = "native-addon";
    const SNAPSHOT: &str = "native-addon@1.0.0";
    const BUILT_FILE: &str = "build/addon.node";
    const BUILT_MODE: u32 = 0o755;
    const KEY_ID: &str = "acme-2026";
    const ORGANIZATION: &str = "acme";
    /// Seeds the fixture signing key. The trust root the config carries is
    /// derived from it rather than pinned separately, so the two cannot drift.
    const PRIVATE_KEY_BYTES: [u8; 32] = [7; 32];

    fn built_bytes() -> &'static [u8] {
        b"native addon built here"
    }

    fn integrity_of(bytes: &[u8]) -> String {
        format!("sha512-{}", BASE64.encode(Sha512::digest(bytes)))
    }

    /// The lockfile's own Node pin, so the compatibility tag the artifact has
    /// to carry follows from the fixture rather than from whichever Node
    /// happens to be installed on the machine running the test.
    const NODE_RUNTIME: &str = "node@runtime:22.0.0";

    fn snapshots() -> HashMap<PackageKey, SnapshotEntry> {
        HashMap::from([
            (SNAPSHOT.parse().expect("snapshot key"), SnapshotEntry::default()),
            (NODE_RUNTIME.parse().expect("runtime key"), SnapshotEntry::default()),
        ])
    }

    fn packages() -> HashMap<PackageKey, PackageMetadata> {
        let metadata = serde_json::json!({
            "resolution": { "integrity": integrity_of(b"the source tarball") },
            "version": "1.0.0",
        });
        HashMap::from([(
            SNAPSHOT.parse().expect("package key"),
            serde_json::from_value(metadata).expect("package metadata"),
        )])
    }

    /// A config that reaches `server` for `PACKAGE` and trusts the fixture key.
    fn config(server: &str, store_dir: &StoreDir) -> Config {
        let mut config = Config::new();
        config.store_dir = store_dir.clone();
        config.pnpr_server = Some(server.to_string());
        config.remote_side_effects_cache = Some(RemoteSideEffectsCacheSettings {
            organization: ORGANIZATION.to_string(),
            packages: vec![PACKAGE.to_string()],
            trusted_keys: Some(BTreeMap::from([(KEY_ID.to_string(), public_key())])),
            ..Default::default()
        });
        config
    }

    fn secret_key() -> SecretKey {
        SecretKey::from_slice(&PRIVATE_KEY_BYTES).expect("fixture private key")
    }

    fn public_key() -> String {
        BASE64.encode(
            secret_key().public_key().to_public_key_der().expect("fixture public key").as_bytes(),
        )
    }

    /// Sign the artifact the server offers for `request`'s one candidate.
    ///
    /// The input key and source integrity are echoed from the request because
    /// both are derived from the host's node major and the lockfile, and the
    /// client discards any variant that does not match the candidate it asked
    /// about.
    fn signed_response(request: &[u8], compatibility_tag: &str) -> String {
        let request: ResolveArtifactsRequest =
            serde_json::from_slice(request).expect("resolve request");
        let [candidate] = request.candidates.as_slice() else {
            panic!("expected exactly one candidate, got {}", request.candidates.len());
        };
        let bytes = built_bytes();
        let payload = ArtifactPayload {
            kind: ARTIFACT_KIND.to_string(),
            package: candidate.package.clone(),
            source_integrity: candidate.source_integrity.clone(),
            input_key: candidate.key.clone(),
            owner: OwnerScope::organization(ORGANIZATION),
            builder_id: "ci/main/1".to_string(),
            builder_profile: BuilderProfile {
                image_digest: None,
                architecture_baseline: "x86-64-v2".to_string(),
                environment: BTreeMap::new(),
            },
            compatibility: CompatibilityConstraints::Tagged {
                tags: vec![compatibility_tag.to_string()],
            },
            manifest: ArtifactManifest {
                added: vec![ArtifactFile {
                    path: BUILT_FILE.to_string(),
                    integrity: integrity_of(bytes),
                    mode: BUILT_MODE,
                    size: bytes.len() as u64,
                }],
                deleted: Vec::new(),
            },
        };
        // Signed the way a publisher signs, so the fixture cannot drift from
        // the wire format the client verifies — and so a payload this test
        // builds wrong fails here rather than being silently discarded as an
        // unverifiable variant.
        let envelope = SignedArtifactEnvelope::sign(
            &payload,
            KEY_ID,
            secret_key().to_pkcs8_der().expect("fixture private key").as_bytes(),
        )
        .expect("sign the fixture payload");
        let response = ResolveArtifactsResponse {
            artifacts: vec![ResolvedArtifact {
                key: candidate.key.clone(),
                variants: vec![ArtifactVariant { envelope }],
            }],
        };
        serde_json::to_string(&response).expect("serialize response")
    }

    /// Restore against a server that offers the artifact, asserting that the
    /// blob endpoint was hit exactly `expected_downloads` times, and return
    /// the path the resulting overlay maps the built file to.
    async fn restore(store_dir: &StoreDir, expected_downloads: usize) -> PathBuf {
        let snapshots = snapshots();
        let packages = packages();
        let platform = super::super::linux_glibc_platform(&snapshots)
            .expect("a linux glibc host describes a platform");
        assert_eq!(
            platform.node_major, 22,
            "the lockfile's Node pin, not the machine's Node, must decide the platform",
        );
        let mut supported_tags = linux_glibc_supported_tags(platform).expect("supported tags");
        let compatibility_tag = supported_tags.swap_remove(0);

        let mut server = mockito::Server::new_async().await;
        let handshake = server
            .mock("GET", "/-/pnpr")
            .with_header("content-type", "application/json")
            .with_body(r#"{"pnpr":{"versions":[0],"artifacts":[0]}}"#)
            .create_async()
            .await;
        let resolve = server
            .mock("POST", "/-/pnpr/v0/artifacts/resolve")
            .with_header("content-type", "application/json")
            .with_body_from_request(move |request| {
                signed_response(request.body().expect("resolve body"), &compatibility_tag).into()
            })
            .create_async()
            .await;
        let blob = server
            .mock("POST", "/-/pnpr/v0/artifacts/blob")
            .with_body(built_bytes())
            .expect(expected_downloads)
            .create_async()
            .await;

        let snapshot_key: PackageKey = SNAPSHOT.parse().expect("snapshot key");
        let mut side_effects = SideEffectsMapsBySnapshot::new();
        super::super::apply_shared_side_effects(
            &config(&server.url(), store_dir),
            &snapshots,
            &packages,
            &RequiresBuildBySnapshot::from([(snapshot_key.clone(), true)]),
            &AllowBuildPolicy::new(HashSet::from([PACKAGE.to_string()]), HashSet::new(), false),
            &HashMap::from([(snapshot_key.clone(), HashMap::new())]),
            &mut side_effects,
        )
        .await;

        handshake.assert_async().await;
        resolve.assert_async().await;
        blob.assert_async().await;

        let maps = side_effects.get(&snapshot_key).expect("the snapshot must be restored");
        let [overlay] = maps.values().collect::<Vec<_>>()[..] else {
            panic!("expected one cache key, got {}", maps.len());
        };
        overlay.get(BUILT_FILE).expect("the built file must be in the overlay").clone()
    }

    #[tokio::test]
    async fn content_the_store_lacks_is_downloaded() {
        let store = tempfile::tempdir().expect("tempdir");
        let store_dir = StoreDir::new(store.path());

        let restored = restore(&store_dir, 1).await;

        let (written, _) = store_dir
            .write_cas_file(built_bytes(), true)
            .expect("re-writing the same bytes names the same path");
        assert_eq!(restored, written);
        assert_eq!(std::fs::read(&restored).expect("read restored"), built_bytes());
    }

    /// Same restore, same server as its sibling above; only the seeded store
    /// differs. The download it makes and this one does not is the whole
    /// difference between them.
    #[tokio::test]
    async fn content_the_store_already_holds_is_not_downloaded() {
        let store = tempfile::tempdir().expect("tempdir");
        let store_dir = StoreDir::new(store.path());
        let (seeded, _) = store_dir.write_cas_file(built_bytes(), true).expect("seed the store");

        let restored = restore(&store_dir, 0).await;

        assert_eq!(restored, seeded);
    }
}
