use std::collections::{BTreeMap, HashSet};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use p256::{
    ecdsa::{SigningKey, signature::Signer as _},
    pkcs8::EncodePublicKey as _,
};
use sha2::{Digest as _, Sha512};

use crate::{
    ARTIFACT_KIND, ArtifactFile, ArtifactManifest, ArtifactPayload, BuilderProfile,
    CompatibilityConstraints, OwnerScope, SIGNATURE_ALGORITHM, SignedArtifactEnvelope, blob_id,
    compatibility_rank, validate_manifest_path, verify_blob,
};

fn integrity(bytes: &[u8]) -> String {
    format!("sha512-{}", BASE64.encode(Sha512::digest(bytes)))
}

fn payload(file_integrity: String) -> ArtifactPayload {
    ArtifactPayload {
        kind: ARTIFACT_KIND.to_string(),
        source_integrity: "sha512-source".to_string(),
        input_key: "dependency-side-effects:v1:deps=abc".to_string(),
        owner: OwnerScope::organization("acme"),
        builder_id: "ci/main/42".to_string(),
        builder_profile: BuilderProfile {
            image_digest: Some("sha256:image".to_string()),
            architecture_baseline: "x86-64-v2".to_string(),
            environment: BTreeMap::from([("CFLAGS".to_string(), "-O2".to_string())]),
        },
        compatibility: CompatibilityConstraints::Tagged {
            tags: vec!["pnpm:v1:linux-x64-node22-glibc".to_string()],
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
    let constraints =
        CompatibilityConstraints::Tagged { tags: vec!["fallback".to_string(), "best".to_string()] };
    let supported = vec!["best".to_string(), "fallback".to_string()];
    assert_eq!(compatibility_rank(&constraints, &supported), Some(0));
    assert_eq!(
        compatibility_rank(&CompatibilityConstraints::Universal, &supported),
        Some(supported.len()),
    );
    assert_eq!(
        compatibility_rank(
            &CompatibilityConstraints::Tagged { tags: vec!["unknown".to_string()] },
            &supported
        ),
        None,
    );
}

#[test]
fn owner_namespaces_are_domain_separated() {
    let namespaces = HashSet::from([
        OwnerScope::organization("foo").namespace(),
        OwnerScope::Publisher { package: "foo".to_string() }.namespace(),
    ]);
    assert_eq!(namespaces.len(), 2);
}
