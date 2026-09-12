use super::{Claims, verify_claims};
use axum::http::Method;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use p256::ecdsa::{Signature, SigningKey, signature::Signer as _};
use std::collections::BTreeMap;

#[test]
fn signed_claims_expire_and_cannot_be_tampered_with() {
    let key = SigningKey::from_slice(&[1; 32]).unwrap();
    let claims =
        Claims { parent: None, audience: String::new(), expires: 100, scopes: BTreeMap::new() };
    let payload = serde_json::to_vec(&claims).unwrap();
    let signature: Signature = key.sign(&payload);
    let token = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(&payload),
        URL_SAFE_NO_PAD.encode(signature.to_bytes()),
    );
    assert!(verify_claims(&key, &token, 99).is_ok());
    assert!(verify_claims(&key, &token, 100).is_err());
    let mut tampered = payload;
    tampered[0] ^= 1;
    let token = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(tampered),
        URL_SAFE_NO_PAD.encode(signature.to_bytes()),
    );
    assert!(verify_claims(&key, &token, 99).is_err());
}

#[test]
fn scope_checks_keep_upload_cancellation_and_registry_boundaries() {
    let claims = Claims {
        parent: None,
        audience: "/oci/~images".into(),
        expires: 100,
        scopes: BTreeMap::from([("acme/v2/app".into(), vec!["pull".into(), "push".into()])]),
    };
    assert!(claims.permits("/oci/~images/v2/acme/v2/app/manifests/latest", &Method::GET));
    assert!(claims.permits("/oci/~images/v2/acme/v2/app/blobs/uploads/session", &Method::DELETE,));
    assert!(!claims.permits("/oci/~images/v2/acme/v2/app/manifests/latest", &Method::DELETE));
    assert!(!claims.permits("/v2/acme/v2/app/manifests/latest", &Method::GET));
    assert!(!claims.permits("/oci/~other/v2/acme/v2/app/manifests/latest", &Method::GET));
}
