use std::collections::{BTreeMap, BTreeSet, HashSet};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use p256::{
    SecretKey,
    ecdsa::{SigningKey, signature::Signer as _},
    pkcs8::{EncodePrivateKey as _, EncodePublicKey as _},
};
use sha2::{Digest as _, Sha512};

use crate::{
    ARTIFACT_KIND, ArtifactBlobUpload, ArtifactCandidate, ArtifactFile, ArtifactManifest,
    ArtifactPayload, ArtifactSubject, BuilderProfile, CompatibilityConstraints,
    CompatibilityScopes, LinuxGlibcPlatform, MacOsPlatform, OwnerScope, PackageIdentity,
    PublishArtifactRequest, SIGNATURE_ALGORITHM, SignedArtifactEnvelope,
    WORKSPACE_TASK_ARTIFACT_KIND, WindowsPlatform, blob_id, compatibility_rank,
    compatibility_scopes, linux_glibc_supported_tags, linux_glibc_tag, macos_supported_tags,
    macos_tag, platform_fingerprint, validate_manifest_path, verify_blob, windows_supported_tags,
    windows_tag,
};

fn integrity(bytes: &[u8]) -> String {
    format!("sha512-{}", BASE64.encode(Sha512::digest(bytes)))
}

fn linux(architecture: &str, glibc_minor: u32) -> LinuxGlibcPlatform<'_> {
    LinuxGlibcPlatform { architecture, node_major: 22, glibc_major: 2, glibc_minor }
}

fn macos(architecture: &str, macos_major: u32, macos_minor: u32) -> MacOsPlatform<'_> {
    MacOsPlatform { architecture, node_major: 22, macos_major, macos_minor }
}

fn windows(
    architecture: &str,
    windows_major: u32,
    windows_minor: u32,
    windows_build: u32,
) -> WindowsPlatform<'_> {
    WindowsPlatform { architecture, node_major: 22, windows_major, windows_minor, windows_build }
}

fn payload(file_integrity: String) -> ArtifactPayload {
    ArtifactPayload {
        kind: ARTIFACT_KIND.to_string(),
        subject: ArtifactSubject::dependency_side_effects(
            PackageIdentity { name: "native-addon".to_string(), version: "1.0.0".to_string() },
            "sha512-source",
        ),
        input_key: "dependency-side-effects:v1:deps=abc".to_string(),
        owner: OwnerScope::organization("acme"),
        builder_id: "ci/main/42".to_string(),
        builder_profile: BuilderProfile {
            image_digest: Some("sha256:image".to_string()),
            architecture_baseline: "x86-64-v2".to_string(),
            environment: BTreeMap::from([("CFLAGS".to_string(), "-O2".to_string())]),
        },
        compatibility: CompatibilityConstraints::Tagged {
            tags: vec![linux_glibc_tag(linux("x64", 17)).unwrap()],
        },
        manifest: ArtifactManifest {
            added: vec![ArtifactFile {
                path: "build/addon.node".to_string(),
                integrity: file_integrity,
                mode: 0o755,
                size: 5,
            }],
            deleted: vec!["src/intermediate.o".to_string()],
        },
    }
}

#[test]
fn validates_subject_specific_artifact_identity() {
    let owner = OwnerScope::organization("acme");
    ArtifactCandidate {
        key: "workspace-task:v1:inputs=abc".to_string(),
        subject: ArtifactSubject::workspace_task("packages/app", "build"),
        owner: owner.clone(),
    }
    .validate()
    .unwrap();

    let publisher = OwnerScope::Publisher { package: "native-addon".to_string() };
    let workspace_task = ArtifactCandidate {
        key: "workspace-task:v1:inputs=abc".to_string(),
        subject: ArtifactSubject::workspace_task("packages/app", "build"),
        owner: publisher,
    };
    assert!(workspace_task.validate().is_err());

    let mut task_payload = payload(integrity(b"addon"));
    task_payload.kind = WORKSPACE_TASK_ARTIFACT_KIND.to_string();
    task_payload.input_key = "workspace-task:v1:inputs=abc".to_string();
    task_payload.subject = ArtifactSubject::workspace_task("packages/app", "build");
    task_payload.owner = owner;
    task_payload.validate().unwrap();

    task_payload.kind = ARTIFACT_KIND.to_string();
    assert!(task_payload.validate().is_err());
}

#[test]
fn verifies_the_exact_signed_payload_bytes() {
    let file_integrity = integrity(b"addon");
    let payload_bytes = serde_json::to_vec(&payload(file_integrity)).unwrap();
    let private_key = SigningKey::from_slice(&[7; 32]).unwrap();
    let signature: p256::ecdsa::Signature = private_key.sign(&payload_bytes);
    let envelope = SignedArtifactEnvelope {
        algorithm: SIGNATURE_ALGORITHM.to_string(),
        key_id: "acme-2026".to_string(),
        payload: BASE64.encode(&payload_bytes),
        signature: BASE64.encode(signature.to_der().as_bytes()),
    };
    let public_key =
        p256::PublicKey::from(private_key.verifying_key()).to_public_key_der().unwrap();

    let verified = envelope.verify(public_key.as_bytes()).unwrap();
    assert_eq!(verified.owner, OwnerScope::organization("acme"));
    assert_eq!(envelope.digest().unwrap().len(), 64);

    let other_private_key = SigningKey::from_slice(&[8; 32]).unwrap();
    let other_public_key =
        p256::PublicKey::from(other_private_key.verifying_key()).to_public_key_der().unwrap();
    assert!(envelope.verify(other_public_key.as_bytes()).is_err());
}

#[test]
fn verifies_signature_before_rejecting_a_trusted_invalid_payload() {
    let mut invalid = payload(integrity(b"addon"));
    invalid.manifest.added[0].path = "../escape".to_string();
    let payload_bytes = serde_json::to_vec(&invalid).unwrap();
    let private_key = SigningKey::from_slice(&[7; 32]).unwrap();
    let signature: p256::ecdsa::Signature = private_key.sign(&payload_bytes);
    let envelope = SignedArtifactEnvelope {
        algorithm: SIGNATURE_ALGORITHM.to_string(),
        key_id: "acme-2026".to_string(),
        payload: BASE64.encode(&payload_bytes),
        signature: BASE64.encode(signature.to_der().as_bytes()),
    };
    let public_key =
        p256::PublicKey::from(private_key.verifying_key()).to_public_key_der().unwrap();

    assert_eq!(envelope.verify_signature(public_key.as_bytes()).unwrap(), invalid);
    assert!(envelope.verify(public_key.as_bytes()).is_err());
    assert_eq!(envelope.digest().unwrap().len(), 64);
}

#[test]
fn verifies_signature_before_deserializing_a_trusted_malformed_payload() {
    let payload_bytes = b"{";
    let private_key = SigningKey::from_slice(&[7; 32]).unwrap();
    let signature: p256::ecdsa::Signature = private_key.sign(payload_bytes);
    let envelope = SignedArtifactEnvelope {
        algorithm: SIGNATURE_ALGORITHM.to_string(),
        key_id: "acme-2026".to_string(),
        payload: BASE64.encode(payload_bytes),
        signature: BASE64.encode(signature.to_der().as_bytes()),
    };
    let public_key =
        p256::PublicKey::from(private_key.verifying_key()).to_public_key_der().unwrap();

    assert_eq!(envelope.verify_signature_bytes(public_key.as_bytes()).unwrap(), payload_bytes);
    assert!(envelope.verify_signature(public_key.as_bytes()).is_err());
    assert_eq!(envelope.digest().unwrap().len(), 64);
}

/// An oversized envelope must be refused from its encoded length, before the
/// decoder allocates the bytes it describes.
#[test]
fn rejects_oversized_envelope_fields_without_decoding_them() {
    let oversized_payload = SignedArtifactEnvelope {
        algorithm: SIGNATURE_ALGORITHM.to_string(),
        key_id: "acme-2026".to_string(),
        payload: "A".repeat(crate::MAX_ENCODED_SIGNED_PAYLOAD_SIZE + 1),
        signature: BASE64.encode([0; 8]),
    };
    let error = oversized_payload.decode_payload().unwrap_err().to_string();
    assert!(error.contains("signed payload exceeds"), "{error}");

    let payload_bytes = serde_json::to_vec(&payload(integrity(b"addon"))).unwrap();
    let oversized_signature = SignedArtifactEnvelope {
        algorithm: SIGNATURE_ALGORITHM.to_string(),
        key_id: "acme-2026".to_string(),
        payload: BASE64.encode(&payload_bytes),
        signature: "A".repeat(crate::MAX_ENCODED_SIGNATURE_SIZE + 1),
    };
    let error = oversized_signature.digest().unwrap_err().to_string();
    assert!(error.contains("DER-encoded P-256 signature"), "{error}");
}

#[test]
fn signs_a_payload_that_the_verifier_accepts() {
    let secret_key = SecretKey::from_slice(&[7; 32]).unwrap();
    let private_key_der = secret_key.to_pkcs8_der().unwrap();
    let private_key = SigningKey::from(secret_key);
    let public_key =
        p256::PublicKey::from(private_key.verifying_key()).to_public_key_der().unwrap();
    let expected = payload(integrity(b"addon"));

    let envelope =
        SignedArtifactEnvelope::sign(&expected, "acme-2026", private_key_der.as_bytes()).unwrap();

    assert_eq!(envelope.verify(public_key.as_bytes()).unwrap(), expected);
}

#[test]
fn envelope_digest_matches_the_cross_stack_vector() {
    let envelope = SignedArtifactEnvelope {
        algorithm: SIGNATURE_ALGORITHM.to_string(),
        key_id: "acme-2026".to_string(),
        payload: "eyJraW5kIjoiZGVwZW5kZW5jeS1zaWRlLWVmZmVjdHM6djEiLCJzdWJqZWN0Ijp7ImtpbmQiOiJkZXBlbmRlbmN5LXNpZGUtZWZmZWN0cyIsInBhY2thZ2UiOnsibmFtZSI6Im5hdGl2ZS1hZGRvbiIsInZlcnNpb24iOiIxLjAuMCJ9LCJzb3VyY2VJbnRlZ3JpdHkiOiJzaGE1MTItc291cmNlIn0sImlucHV0S2V5IjoiZGVwZW5kZW5jeS1zaWRlLWVmZmVjdHM6djE6ZGVwcz1hYmMiLCJvd25lciI6eyJ0eXBlIjoib3JnYW5pemF0aW9uIiwibmFtZSI6ImFjbWUifSwiYnVpbGRlcklkIjoiY2kvbWFpbi80MiIsImJ1aWxkZXJQcm9maWxlIjp7ImltYWdlRGlnZXN0Ijoic2hhMjU2OmltYWdlIiwiYXJjaGl0ZWN0dXJlQmFzZWxpbmUiOiJ4ODYtNjQtdjIiLCJlbnZpcm9ubWVudCI6eyJDRkxBR1MiOiItTzIifX0sImNvbXBhdGliaWxpdHkiOnsia2luZCI6InRhZ2dlZCIsInRhZ3MiOlsicG5wbTp2MTpsaW51eC14NjQtbm9kZTIyLWdsaWJjMi4xNyJdfSwibWFuaWZlc3QiOnsiYWRkZWQiOlt7InBhdGgiOiJidWlsZC9hZGRvbi5ub2RlIiwiaW50ZWdyaXR5Ijoic2hhNTEyLUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBPT0iLCJtb2RlIjo0OTMsInNpemUiOjV9XSwiZGVsZXRlZCI6WyJzcmMvaW50ZXJtZWRpYXRlLm8iXX19".to_string(),
        signature: "MAYCAQECAQE=".to_string(),
    };
    assert_eq!(
        envelope.digest().unwrap(),
        "20b3fbc179563fc173c1bd306b8d088eb0eebb6fa40998e55d645c414f1964f5",
    );

    let mut noncanonical = envelope;
    noncanonical.payload.push('=');
    assert!(noncanonical.digest().is_err());
}

#[test]
fn rejects_paths_before_the_importer_can_see_them() {
    for unsafe_path in [
        "/absolute",
        "../escape",
        "a/../escape",
        r"a\b",
        "C:/escape",
        "double//segment",
        "dot/./segment",
        "trailing-dot.",
        "trailing-space ",
        "dir/trailing-dot.",
        "dir/trailing-space ",
        "dir/addon.node:payload",
        "CON",
        "dir/NUL.txt",
        "dir/com1.js",
        "COM¹",
        "dir/LPT².txt",
        "dir/LpT9",
        "nul\0byte",
    ] {
        assert!(validate_manifest_path(unsafe_path).is_err(), "accepted {unsafe_path:?}");
    }
    validate_manifest_path("build/Release/addon.node").unwrap();
}

#[test]
fn rejects_duplicate_and_case_colliding_paths() {
    let file_integrity = integrity(b"addon");
    let mut artifact = payload(file_integrity);
    artifact.manifest.deleted.push("BUILD/addon.node".to_string());
    assert!(artifact.validate().is_err());
}

#[test]
fn rejects_inconsistent_sizes_for_one_blob() {
    let file_integrity = integrity(b"addon");
    let mut artifact = payload(file_integrity.clone());
    artifact.manifest.added.push(ArtifactFile {
        path: "build/addon-copy.node".to_string(),
        integrity: file_integrity,
        mode: 0o755,
        size: 6,
    });
    assert!(artifact.validate().is_err());
}

#[test]
fn validates_publication_blobs_before_transport() {
    let file_integrity = integrity(b"addon");
    let artifact = payload(file_integrity.clone());
    let request = PublishArtifactRequest {
        key: artifact.input_key.clone(),
        envelope: SignedArtifactEnvelope {
            algorithm: SIGNATURE_ALGORITHM.to_string(),
            key_id: "acme-2026".to_string(),
            payload: BASE64.encode(serde_json::to_vec(&artifact).unwrap()),
            signature: "eA==".to_string(),
        },
        blobs: vec![ArtifactBlobUpload {
            integrity: file_integrity.clone(),
            data: BASE64.encode(b"addon"),
        }],
    };
    let validated = request.validate().unwrap();
    assert_eq!(validated.blobs[&file_integrity], b"addon");

    let mut duplicate = request.clone();
    duplicate.blobs.push(duplicate.blobs[0].clone());
    assert!(duplicate.validate().is_err());

    let mut unreferenced = request;
    unreferenced.blobs[0].integrity = integrity(b"other");
    assert!(unreferenced.validate().is_err());
}

#[test]
fn verifies_blob_bytes_and_derives_a_path_safe_id() {
    let file_integrity = integrity(b"addon");
    verify_blob(&file_integrity, b"addon").unwrap();
    assert!(verify_blob(&file_integrity, b"poison").is_err());
    let id = blob_id(&file_integrity).unwrap();
    assert_eq!(id.len(), 128);
    assert!(id.bytes().all(|byte| byte.is_ascii_hexdigit()));
}

#[test]
fn compatibility_uses_the_consumers_preference_order() {
    let supported = linux_glibc_supported_tags(linux("x64", 39)).unwrap();
    let closest =
        CompatibilityConstraints::Tagged { tags: vec![linux_glibc_tag(linux("x64", 31)).unwrap()] };
    let fallback =
        CompatibilityConstraints::Tagged { tags: vec![linux_glibc_tag(linux("x64", 17)).unwrap()] };
    assert_eq!(compatibility_rank(&closest, &supported), Some(8));
    assert_eq!(compatibility_rank(&fallback, &supported), Some(22));
    assert_eq!(
        compatibility_rank(&CompatibilityConstraints::Universal, &supported),
        Some(u64::MAX),
    );
    assert_eq!(
        compatibility_rank(
            &CompatibilityConstraints::Tagged {
                tags: vec![linux_glibc_tag(linux("arm64", 17)).unwrap()],
            },
            &supported
        ),
        None,
    );
}

#[test]
fn macos_compatibility_uses_product_version_floors() {
    let supported = macos_supported_tags(macos("arm64", 15, 5)).unwrap();
    assert_eq!(supported, ["pnpm:v1:darwin-arm64-node22-macos15.5"]);
    assert_eq!(
        platform_fingerprint(&supported).unwrap(),
        "b56fa5629b56d18308bbf7978d61b9afaf862e133ad18aef31588e0888eef3f8",
    );
    assert_eq!(
        compatibility_rank(
            &CompatibilityConstraints::Tagged {
                tags: vec![macos_tag(macos("arm64", 15, 4)).unwrap()],
            },
            &supported,
        ),
        Some(65),
    );
    assert_eq!(
        compatibility_rank(
            &CompatibilityConstraints::Tagged {
                tags: vec![macos_tag(macos("arm64", 14, 6)).unwrap()],
            },
            &supported,
        ),
        Some(1_000_063),
    );
    assert_eq!(
        compatibility_rank(
            &CompatibilityConstraints::Tagged {
                tags: vec![macos_tag(macos("arm64", 16, 0)).unwrap()],
            },
            &supported,
        ),
        None,
    );
    assert_eq!(
        compatibility_rank(&CompatibilityConstraints::Universal, &supported),
        Some(u64::MAX),
    );

    let multiple_supported =
        vec![macos_tag(macos("arm64", 15, 5)).unwrap(), macos_tag(macos("arm64", 14, 6)).unwrap()];
    assert_eq!(
        compatibility_rank(
            &CompatibilityConstraints::Tagged {
                tags: vec![macos_tag(macos("arm64", 14, 6)).unwrap()],
            },
            &multiple_supported,
        ),
        Some(1),
    );
    assert_eq!(
        compatibility_rank(
            &CompatibilityConstraints::Tagged {
                tags: vec![macos_tag(macos("arm64", 15, 4)).unwrap()],
            },
            &multiple_supported,
        ),
        Some(65),
    );
}

#[test]
fn windows_compatibility_uses_kernel_version_floors() {
    let supported = windows_supported_tags(windows("x64", 10, 0, 26_100)).unwrap();
    assert_eq!(supported, ["pnpm:v1:win32-x64-node22-windows10.0.26100"]);
    assert_eq!(
        platform_fingerprint(&supported).unwrap(),
        "f5590f12a6d651acdcb3b60d7d25a5d2e1ad2f5af3e53d841391dec9e871c46e",
    );
    assert_eq!(
        compatibility_rank(
            &CompatibilityConstraints::Tagged {
                tags: vec![windows_tag(windows("x64", 10, 0, 22_621)).unwrap()],
            },
            &supported,
        ),
        Some(3_543),
    );
    assert_eq!(
        compatibility_rank(
            &CompatibilityConstraints::Tagged {
                tags: vec![windows_tag(windows("x64", 6, 3, 9_600)).unwrap()],
            },
            &supported,
        ),
        Some(3_997_016_564),
    );
    assert_eq!(
        compatibility_rank(
            &CompatibilityConstraints::Tagged {
                tags: vec![windows_tag(windows("x64", 10, 0, 26_101)).unwrap()],
            },
            &supported,
        ),
        None,
    );
    assert_eq!(
        compatibility_rank(&CompatibilityConstraints::Universal, &supported),
        Some(u64::MAX),
    );
}

#[test]
fn scopes_name_the_machines_constraints_reach() {
    let tagged = |tags: &[&str]| CompatibilityConstraints::Tagged {
        tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
    };
    let scopes = |tags: &[&str]| match compatibility_scopes(&tagged(tags)) {
        CompatibilityScopes::These(scopes) => scopes,
        CompatibilityScopes::Every => unreachable!("tagged constraints name their machines"),
    };
    let overlap = |left: &[&str], right: &[&str]| !scopes(left).is_disjoint(&scopes(right));

    assert_eq!(
        compatibility_scopes(&CompatibilityConstraints::Universal),
        CompatibilityScopes::Every,
    );
    assert_eq!(
        scopes(&["pnpm:v1:linux-x64-node22-glibc2.17"]),
        BTreeSet::from(["linux-x64-node22".to_string()]),
    );

    // A floor never separates two tags: a consumer meeting the higher one meets
    // the lower as well, on every vocabulary that has floors.
    assert!(overlap(
        &["pnpm:v1:linux-x64-node22-glibc2.17"],
        &["pnpm:v1:linux-x64-node22-glibc2.31"],
    ));
    assert!(overlap(
        &["pnpm:v1:darwin-arm64-node22-macos13.0"],
        &["pnpm:v1:darwin-arm64-node22-macos14.2"],
    ));
    assert!(overlap(
        &["pnpm:v1:win32-x64-node22-windows10.0.17763"],
        &["pnpm:v1:win32-x64-node22-windows10.0.22000"],
    ));

    // Operating system, architecture, and Node major each keep them apart.
    assert!(!overlap(
        &["pnpm:v1:linux-x64-node22-glibc2.17"],
        &["pnpm:v1:linux-arm64-node22-glibc2.17"],
    ));
    assert!(!overlap(
        &["pnpm:v1:linux-x64-node22-glibc2.17"],
        &["pnpm:v1:linux-x64-node24-glibc2.17"],
    ));
    assert!(!overlap(
        &["pnpm:v1:linux-x64-node22-glibc2.17"],
        &["pnpm:v1:darwin-x64-node22-macos13.0"],
    ));

    // A set reaches every machine any of its tags reaches.
    assert!(overlap(
        &["pnpm:v1:linux-arm64-node22-glibc2.17", "pnpm:v1:linux-x64-node22-glibc2.31"],
        &["pnpm:v1:linux-x64-node22-glibc2.17"],
    ));

    // A tag that describes no machine reaches none: unparsable, or naming a
    // floor that belongs to another operating system.
    assert!(scopes(&["not-a-tag"]).is_empty());
    assert!(scopes(&["pnpm:v1:linux-x64-node22-macos13.0"]).is_empty());
    assert!(scopes(&["pnpm:v1:darwin-x64-node22-glibc2.17"]).is_empty());
}

#[test]
fn compatibility_tags_and_platform_fingerprints_are_canonical() {
    let supported = linux_glibc_supported_tags(linux("x64", 3)).unwrap();
    assert_eq!(
        supported,
        [
            "pnpm:v1:linux-x64-node22-glibc2.3",
            "pnpm:v1:linux-x64-node22-glibc2.2",
            "pnpm:v1:linux-x64-node22-glibc2.1",
            "pnpm:v1:linux-x64-node22-glibc2.0",
        ],
    );
    assert_eq!(
        platform_fingerprint(&supported).unwrap(),
        "fdfaaed730a56031779ee5e572e1e82aad454501ec5fbcfad6648e8a1e465f0c",
    );

    for invalid in [
        "pnpm:v2:linux-x64-node22-glibc2.17",
        "pnpm:v1:darwin-x64-node22-glibc2.17",
        "pnpm:v1:darwin-x64-node22-macos15",
        "pnpm:v1:darwin-x64-node22-macos015.5",
        "pnpm:v1:win32-x64-node22-windows10.0",
        "pnpm:v1:win32-x64-node22-windows10.0.026100",
        "pnpm:v1:linux-x64-node022-glibc2.17",
        "pnpm:v1:linux-x64-node22-glibc02.17",
        "pnpm:v1:linux-x64-node22-glibc2",
    ] {
        let mut artifact = payload(integrity(b"addon"));
        artifact.compatibility =
            CompatibilityConstraints::Tagged { tags: vec![invalid.to_string()] };
        assert!(artifact.validate().is_err());
        assert_eq!(compatibility_rank(&artifact.compatibility, &supported), None);
    }
}

#[test]
fn publisher_owner_must_match_the_signed_package() {
    let mut artifact = payload(integrity(b"addon"));
    artifact.owner = OwnerScope::Publisher { package: "another-package".to_string() };
    assert!(artifact.validate().is_err());
}

#[test]
fn owner_namespaces_are_domain_separated() {
    let namespaces = HashSet::from([
        OwnerScope::organization("foo").namespace(),
        OwnerScope::Publisher { package: "foo".to_string() }.namespace(),
    ]);
    assert_eq!(namespaces.len(), 2);
}
