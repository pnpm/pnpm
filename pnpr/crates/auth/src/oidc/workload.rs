use super::{
    BASE64_URL_SAFE_NO_PAD, ClientId, CoreIdToken, CoreIdTokenVerifier, CoreJwsSigningAlgorithm,
    CoreProviderMetadata, IssuerUrl, Nonce, OidcBinding, OidcProvider, OidcWorkload, Provider,
    Result, Utc, Value, rejected,
};
use base64::Engine as _;

pub(super) fn verify_workload(
    config: &OidcProvider,
    metadata: &CoreProviderMetadata,
    raw: &str,
) -> Result<()> {
    let token: CoreIdToken = raw.parse().map_err(|_| rejected())?;
    let verifier = token_verifier(config, metadata)?;
    token.claims(&verifier, |_: Option<&Nonce>| Ok(())).map_err(|_| rejected())?;
    validate_claims(config, &token_payload(raw)?)
}

/// The one configured user the token's claims bind to.
pub(super) fn bound_user<'p>(
    config: &'p OidcProvider,
    token: &CoreIdToken,
) -> Result<&'p OidcBinding> {
    let payload = token_payload(&token.to_string())?;
    validate_claims(config, &payload)?;
    let users = &config.login.as_ref().ok_or_else(rejected)?.users;
    unique_binding(users.iter(), &payload)
}

pub(super) fn token_verifier(
    config: &OidcProvider,
    metadata: &CoreProviderMetadata,
) -> Result<CoreIdTokenVerifier<'static>> {
    Ok(CoreIdTokenVerifier::new_public_client(
        ClientId::new(config.audience.clone()),
        IssuerUrl::new(config.issuer.clone()).map_err(|_| rejected())?,
        metadata.jwks().clone(),
    )
    .set_allowed_algs([
        CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
        CoreJwsSigningAlgorithm::EcdsaP256Sha256,
    ]))
}

pub(super) fn validate_claims(config: &OidcProvider, payload: &Value) -> Result<()> {
    validate_times(payload)?;
    if let Some(party) = payload.get("azp") {
        if party.as_str() != Some(config.audience.as_str()) {
            return Err(rejected());
        }
    } else if payload
        .get("aud")
        .and_then(Value::as_array)
        .is_some_and(|audiences| audiences.len() > 1)
    {
        return Err(rejected());
    }
    Ok(())
}

pub(super) fn token_payload(raw: &str) -> Result<Value> {
    if raw.len() > 16 * 1024 {
        return Err(rejected());
    }
    let payload = raw.split('.').nth(1).ok_or_else(rejected)?;
    let bytes = BASE64_URL_SAFE_NO_PAD.decode(payload).map_err(|_| rejected())?;
    serde_json::from_slice(&bytes).map_err(|_| rejected())
}

pub(super) fn validate_times(payload: &Value) -> Result<()> {
    let now = Utc::now().timestamp();
    let issued = payload.get("iat").and_then(Value::as_i64).ok_or_else(rejected)?;
    let expires = payload.get("exp").and_then(Value::as_i64).ok_or_else(rejected)?;
    if issued > now + 60 || expires <= now || expires <= issued {
        return Err(rejected());
    }
    if let Some(not_before) = payload.get("nbf")
        && not_before.as_i64().is_none_or(|not_before| not_before > now)
    {
        return Err(rejected());
    }
    Ok(())
}

pub(super) fn unique_binding<'binding>(
    bindings: impl Iterator<Item = &'binding OidcBinding>,
    payload: &Value,
) -> Result<&'binding OidcBinding> {
    let mut matches = bindings.filter(|binding| binding_matches(binding, payload));
    let binding = matches.next().ok_or_else(rejected)?;
    if matches.next().is_some() {
        return Err(rejected());
    }
    Ok(binding)
}

/// Record the one workload binding this token satisfies. Two matching
/// bindings make the identity ambiguous, which is rejected rather than
/// resolved by declaration order.
pub(super) fn match_workload_binding(
    provider: &Provider,
    payload: &Value,
    matched: &mut Option<OidcWorkload>,
) -> Result<()> {
    for workload in &provider.config.workloads {
        if !binding_matches(&workload.identity, payload) {
            continue;
        }
        if matched.is_some() {
            return Err(rejected());
        }
        *matched = Some(workload.clone());
    }
    Ok(())
}

pub(super) fn binding_matches(binding: &OidcBinding, payload: &Value) -> bool {
    payload.get("sub").and_then(Value::as_str) == Some(binding.subject.as_str())
        && binding.claims.iter().all(|(key, expected)| {
            payload.get(key).and_then(Value::as_str) == Some(expected.as_str())
        })
}
