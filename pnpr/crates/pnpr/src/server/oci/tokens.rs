mod grants;
use super::{Endpoint, ErrorCode, Request, error, json, parse_endpoint};
use crate::server::{
    Action, AppState, AuthedCaller, Identity, TargetRegistry,
    authentication::token_credentials,
    authorize,
    ecosystem::{addressed_registry, sha256_hex},
    resolve_ecosystem_source,
};
use axum::{
    extract::{OriginalUri, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::Response,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use grants::{GrantQuery, granted_scopes};
use p256::ecdsa::{
    Signature, SigningKey,
    signature::{Signer as _, Verifier as _},
};
use pnpr_error::RegistryError;
use pnpr_registry::Ecosystem;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{collections::BTreeMap, fmt::Write as _};

pub(super) const TOKEN_PREFIX: &str = "pnpr_oci_";
const TTL: u64 = 300;

#[derive(Serialize, Deserialize)]
pub(in crate::server) struct Claims {
    pub(in crate::server) parent: Option<String>,
    audience: String,
    expires: u64,
    scopes: BTreeMap<String, Vec<String>>,
}

fn signing_key(state: &AppState) -> Result<SigningKey, RegistryError> {
    let mut hash = Sha256::new();
    hash.update(b"pnpr OCI token signing v1\0");
    hash.update(&state.inner.config.resolution_cache_secret);
    SigningKey::from_slice(&hash.finalize())
        .map_err(|_| RegistryError::Internal {
            reason: "invalid OCI signing key".to_string(),
        })
}

pub(in crate::server) fn decode(
    state: &AppState,
    token: &str,
) -> Result<Option<Claims>, RegistryError> {
    let Some(token) = token.strip_prefix(TOKEN_PREFIX) else {
        return Ok(None);
    };
    let invalid = || RegistryError::Unauthenticated {
        resource: "OCI token".to_string(),
    };
    if !state.inner.config.http.oci.bearer_auth || token.len() > 16 * 1024 {
        return Err(invalid());
    }
    verify_claims(&signing_key(state)?, token, super::now_millis() / 1000).map(Some)
}

fn verify_claims(key: &SigningKey, token: &str, now: u64) -> Result<Claims, RegistryError> {
    let invalid = || RegistryError::Unauthenticated {
        resource: "OCI token".to_string(),
    };
    let (payload, signature) = token.split_once('.').ok_or_else(invalid)?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| invalid())?;
    let signature = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| invalid())?;
    let signature = Signature::from_slice(&signature).map_err(|_| invalid())?;
    key
        .verifying_key()
        .verify(&bytes, &signature)
        .map_err(|_| invalid())?;
    let claims: Claims = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if claims.expires <= now {
        return Err(invalid());
    }
    Ok(claims)
}

impl Claims {
    pub(in crate::server) fn permits(&self, path: &str, method: &Method) -> bool {
        let decoded = pnpr_search::percent_decode(path);
        let Some(tail) = decoded
            .strip_prefix(&self.audience)
            .and_then(|tail| tail.strip_prefix("/v2"))
        else {
            return false;
        };
        if !(tail.is_empty() || tail.starts_with('/')) {
            return false;
        }
        if tail.trim_matches('/').is_empty() {
            return matches!(*method, Method::GET | Method::HEAD);
        }
        let Some(endpoint) = parse_endpoint(tail) else {
            return false;
        };
        // Upload cancellation requires push scope, just like the rest of the upload session.
        let upload = matches!(
            endpoint,
            Endpoint::StartUpload { .. } | Endpoint::Upload { .. },
        );
        let Some(name) = endpoint.name() else {
            return false;
        };
        let Ok(name) =
            pnpr_package_name::CanonicalPackageName::parse(name, pnpr_registry::Ecosystem::Oci)
        else {
            return false;
        };
        self.allows(name.as_str(), endpoint_action(method, upload))
    }

    pub(super) fn allows(&self, name: &str, action: &str) -> bool {
        self.scopes
            .get(name)
            .is_some_and(|actions| {
                actions
                    .iter()
                    .any(|held| held == action)
            })
    }
}

/// The scope action a request needs. An upload session is push throughout,
/// including its cancellation.
fn endpoint_action(method: &Method, upload: bool) -> &'static str {
    if upload {
        return "push";
    }
    match *method {
        Method::GET | Method::HEAD => "pull",
        Method::DELETE => "delete",
        _ => "push",
    }
}

pub(super) async fn issue(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
) -> Response {
    match issue_token(&state, &identity, registry.as_deref(), &uri, &headers).await {
        Ok(response) => response,
        Err(err) => super::registry_error(err),
    }
}

async fn issue_token(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    uri: &axum::http::Uri,
    headers: &HeaderMap,
) -> Result<Response, RegistryError> {
    if !state.inner.config.http.oci.bearer_auth {
        return Ok(error(
            ErrorCode::Unsupported,
            "OCI Bearer authentication is disabled",
        ));
    }
    let target =
        addressed_registry(state, registry, Ecosystem::Oci).ok_or(RegistryError::NotFound)?;
    let raw = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(token_credentials);
    if raw.is_some() && *identity == Identity::Anonymous {
        return Ok(error(ErrorCode::Unauthorized, "invalid credentials"));
    }
    let parent = raw
        .as_ref()
        .map(|raw| sha256_hex(raw.as_bytes()));
    let record = match &parent {
        Some(parent) => state.inner.identity.auth.tokens.find_by_key(parent).await?,
        None => None,
    };
    let readonly = record.is_some_and(|record| record.readonly);
    let query = GrantQuery {
        uri,
        readonly,
    };
    let scopes = granted_scopes(state, identity, &target, &query)?;
    let audience = token_audience(uri)?;
    let claims = Claims {
        parent,
        audience,
        expires: super::now_millis() / 1000 + TTL,
        scopes,
    };
    signed_response(state, &claims)
}

fn signed_response(state: &AppState, claims: &Claims) -> Result<Response, RegistryError> {
    let payload = serde_json::to_vec(&claims)?;
    let signature: Signature = signing_key(state)?.sign(&payload);
    let token = format!(
        "{TOKEN_PREFIX}{}.{}",
        URL_SAFE_NO_PAD.encode(payload),
        URL_SAFE_NO_PAD.encode(signature.to_bytes()),
    );
    Ok(crate::server::private_no_cache(json(
        StatusCode::OK,
        &serde_json::json!({ "token": token, "expires_in": TTL }),
    )))
}

pub(super) fn challenge(
    state: &AppState,
    base: &str,
    scope: Option<(&str, &str)>,
    mut response: Response,
) -> Response {
    if response.status() != StatusCode::UNAUTHORIZED || !state.inner.config.http.oci.bearer_auth {
        return response;
    }
    let realm = format!(
        "{}{base}/token",
        state.inner.config.http.public_url.trim_end_matches('/'),
    );
    let mut value = format!(r#"Bearer realm="{realm}",service="pnpr""#);
    if let Some((name, actions)) = scope
        && let Ok(name) =
            pnpr_package_name::CanonicalPackageName::parse(name, pnpr_registry::Ecosystem::Oci)
    {
        write!(value, r#",scope="repository:{}:{actions}""#, name.as_str()).expect(
            "writing to a string cannot fail",
        );
    }
    if let Ok(value) = axum::http::HeaderValue::from_str(&value) {
        response.headers_mut().insert(header::WWW_AUTHENTICATE, value);
    }
    crate::server::private_no_cache(response)
}

impl Request {
    pub(super) fn challenge(&self, name: Option<&str>, response: Response) -> Response {
        let actions = match self.method {
            Method::GET | Method::HEAD => "pull",
            Method::DELETE => "pull,push,delete",
            _ => "pull,push",
        };
        challenge(
            &self.state,
            &self.base,
            name.map(|name| (name, actions)),
            response,
        )
    }
}

pub(in crate::server) fn rejected(state: &AppState, path: &str, method: &Method) -> Response {
    let decoded = pnpr_search::percent_decode(path);
    let response = error(
        ErrorCode::Unauthorized,
        "OCI token is expired, revoked, or outside its scope",
    );
    let Some((prefix, tail)) = decoded.split_once("/v2") else {
        return response;
    };
    let endpoint = parse_endpoint(tail);
    let name = endpoint.as_ref().and_then(Endpoint::name);
    let actions = match *method {
        Method::GET | Method::HEAD => "pull",
        Method::DELETE => "pull,push,delete",
        _ => "pull,push",
    };
    challenge(
        state,
        &format!("{prefix}/v2"),
        name.map(|name| (name, actions)),
        response,
    )
}

#[cfg(test)]
mod tests;

fn token_audience(uri: &axum::http::Uri) -> Result<String, RegistryError> {
    Ok(pnpr_search::percent_decode(
        uri
            .path()
            .strip_suffix("/v2/token")
            .ok_or(RegistryError::NotFound)?,
    ))
}
