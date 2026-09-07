mod network;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as BASE64_URL_SAFE_NO_PAD};
use chrono::Utc;
use openidconnect::{
    AccessTokenHash, AuthType, AuthorizationCode, ClientId, ClientSecret, CsrfToken,
    EndpointMaybeSet, EndpointNotSet, EndpointSet, HttpRequest, HttpResponse, IssuerUrl, Nonce,
    OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, TokenResponse,
    core::{
        CoreAuthenticationFlow, CoreClient, CoreClientAuthMethod, CoreIdToken, CoreIdTokenVerifier,
        CoreJwsSigningAlgorithm, CoreProviderMetadata,
    },
};
use p256::ecdsa::{
    Signature, SigningKey,
    signature::{Signer as _, Verifier as _},
};
use pnpr_config::oidc::{OidcBinding, OidcProvider, OidcWorkload};
use pnpr_error::{RegistryError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::sync::Mutex as AsyncMutex;
use url::Url;

const MAX_ENTRIES: usize = 1024;
const LOGIN_TTL: Duration = Duration::from_mins(5);
const METADATA_TTL: Duration = Duration::from_mins(5);
const REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const SESSION_PREFIX: &str = "pnpr_oidc_";

type LoginClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

pub struct OidcState {
    providers: HashMap<String, Provider>,
    http: reqwest::Client,
    public_url: String,
    state_key: SigningKey,
    consumed: Mutex<HashMap<String, i64>>,
    sessions: Mutex<HashMap<String, Session>>,
}

struct Provider {
    config: OidcProvider,
    metadata: AsyncMutex<MetadataCache>,
    refresh: AsyncMutex<()>,
}

#[derive(Default)]
struct MetadataCache {
    value: Option<(Instant, CoreProviderMetadata)>,
    attempted_at: Option<Instant>,
}

#[derive(Serialize, Deserialize)]
struct PendingLogin {
    provider: String,
    nonce: String,
    verifier: String,
    state_hash: String,
    expires: i64,
}

struct Session {
    username: String,
    expires: i64,
}

pub struct LoginStart {
    pub url: String,
    pub state: String,
    pub browser_secret: String,
}

pub struct LoginSession {
    pub token: String,
    pub expires: i64,
}

impl OidcState {
    pub fn new(configs: &[OidcProvider], public_url: &str) -> Result<Self> {
        let mut providers = HashMap::new();
        for config in configs {
            validate_provider(config)?;
            if providers
                .insert(
                    config.name.clone(),
                    Provider {
                        config: config.clone(),
                        metadata: AsyncMutex::new(MetadataCache::default()),
                        refresh: AsyncMutex::new(()),
                    },
                )
                .is_some()
            {
                return Err(invalid_config("duplicate OIDC provider name"));
            }
            if config.login.is_some() {
                secure_url(public_url)?;
                let url =
                    Url::parse(public_url).map_err(|_| invalid_config("invalid public URL"))?;
                if url.path() != "/" && !url.path().is_empty() {
                    return Err(invalid_config(
                        "OIDC login requires --public-url at the origin root",
                    ));
                }
            }
        }
        let state_key =
            SigningKey::from_bytes((&super::fresh_secret()).into()).map_err(|_| unavailable())?;
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .dns_resolver(std::sync::Arc::new(network::PublicResolver(
                pnpm_network::native_dns_resolver(),
            )))
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|_| unavailable())?;
        Ok(Self {
            providers,
            http,
            public_url: public_url.trim_end_matches('/').to_string(),
            state_key,
            consumed: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
        })
    }

    pub async fn start(&self, provider_name: &str) -> Result<LoginStart> {
        let provider = self.providers.get(provider_name).ok_or(RegistryError::NotFound)?;
        if provider.config.login.is_none() {
            return Err(RegistryError::NotFound);
        }
        let metadata = self.metadata(provider, false).await?;
        let client = self.login_client(provider, metadata)?;
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, state, nonce) = client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .set_pkce_challenge(challenge)
            .url();
        let pending = PendingLogin {
            provider: provider_name.to_string(),
            nonce: nonce.secret().clone(),
            verifier: verifier.secret().clone(),
            state_hash: super::sha256_hex(state.secret().as_bytes()),
            expires: Utc::now().timestamp() + LOGIN_TTL.as_secs() as i64,
        };
        let browser_secret = self.seal_login(&pending)?;
        Ok(LoginStart { url: url.to_string(), state: state.secret().clone(), browser_secret })
    }

    pub async fn finish(
        &self,
        provider_name: &str,
        state: &str,
        browser_secret: &str,
        code: &str,
    ) -> Result<LoginSession> {
        let login = self.open_login(browser_secret)?;
        let now = Utc::now().timestamp();
        if login.provider != provider_name
            || login.expires <= now
            || login.state_hash != super::sha256_hex(state.as_bytes())
            || code.is_empty()
            || code.len() > 8192
        {
            return Err(rejected());
        }
        if self
            .consumed
            .lock()
            .expect("OIDC consumed mutex poisoned")
            .contains_key(&login.state_hash)
        {
            return Err(rejected());
        }
        let nonce = Nonce::new(login.nonce);
        let provider = self.providers.get(provider_name).ok_or_else(rejected)?;
        let metadata = self.metadata(provider, false).await?;
        let client = self.login_client(provider, metadata.clone())?;
        let response = client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .map_err(|_| rejected())?
            .set_pkce_verifier(PkceCodeVerifier::new(login.verifier))
            .request_async(self)
            .await
            .map_err(|_| rejected())?;
        let token = response.id_token().ok_or_else(rejected)?;
        let mut verifier = token_verifier(&provider.config, &metadata)?;
        if token.claims(&verifier, &nonce).is_err() {
            verifier = token_verifier(&provider.config, &self.metadata(provider, true).await?)?;
        }
        let claims = token.claims(&verifier, &nonce).map_err(|_| rejected())?;
        if let Some(expected) = claims.access_token_hash() {
            let actual = AccessTokenHash::from_token(
                response.access_token(),
                token.signing_alg().map_err(|_| rejected())?,
                token.signing_key(&verifier).map_err(|_| rejected())?,
            )
            .map_err(|_| rejected())?;
            if actual != *expected {
                return Err(rejected());
            }
        }
        let payload = token_payload(&token.to_string())?;
        validate_claims(&provider.config, &payload)?;
        let users = &provider.config.login.as_ref().ok_or_else(rejected)?.users;
        let binding = unique_binding(users.iter(), &payload)?;
        let now = Utc::now().timestamp();
        if login.expires <= now {
            return Err(rejected());
        }
        let mut consumed = self.consumed.lock().expect("OIDC consumed mutex poisoned");
        consumed.retain(|_, expires| *expires > now);
        if consumed.contains_key(&login.state_hash) || consumed.len() >= MAX_ENTRIES {
            return Err(rejected());
        }
        let session = self.issue_session(&binding.username, claims.expiration().timestamp())?;
        consumed.insert(login.state_hash, login.expires);
        Ok(session)
    }

    fn seal_login(&self, login: &PendingLogin) -> Result<String> {
        let bytes = serde_json::to_vec(login).map_err(|_| unavailable())?;
        let signature: Signature = self.state_key.sign(&bytes);
        Ok(format!(
            "{}.{}",
            BASE64_URL_SAFE_NO_PAD.encode(bytes),
            BASE64_URL_SAFE_NO_PAD.encode(signature.to_bytes()),
        ))
    }

    fn open_login(&self, cookie: &str) -> Result<PendingLogin> {
        if cookie.len() > 4096 {
            return Err(rejected());
        }
        let (payload, signature) = cookie.split_once('.').ok_or_else(rejected)?;
        let payload = BASE64_URL_SAFE_NO_PAD.decode(payload).map_err(|_| rejected())?;
        let signature = BASE64_URL_SAFE_NO_PAD.decode(signature).map_err(|_| rejected())?;
        let signature = Signature::from_slice(&signature).map_err(|_| rejected())?;
        self.state_key.verifying_key().verify(&payload, &signature).map_err(|_| rejected())?;
        serde_json::from_slice(&payload).map_err(|_| rejected())
    }

    /// Resolves only pnpr-issued browser sessions. Unknown or expired session tokens fail closed.
    pub fn session(&self, raw: &str) -> Result<Option<String>> {
        if !raw.starts_with(SESSION_PREFIX) {
            return Ok(None);
        }
        let now = Utc::now().timestamp();
        let mut sessions = self.sessions.lock().expect("OIDC session mutex poisoned");
        let hash = super::sha256_hex(raw.as_bytes());
        if let Some(session) = sessions.get(&hash)
            && session.expires > now
        {
            return Ok(Some(session.username.clone()));
        }
        sessions.remove(&hash);
        Err(rejected())
    }

    pub fn revoke_session(&self, raw: &str) -> bool {
        self.sessions
            .lock()
            .expect("OIDC session mutex poisoned")
            .remove(&super::sha256_hex(raw.as_bytes()))
            .is_some()
    }

    /// Verifies workload credentials against configured issuers only. The returned restrictions
    /// must be enforced before treating the mapped username as an authenticated caller.
    pub async fn workload(&self, raw: &str) -> Result<Option<OidcWorkload>> {
        if !self.providers.values().any(|provider| !provider.config.workloads.is_empty())
            || raw.split('.').count() != 3
        {
            return Ok(None);
        }
        let payload = token_payload(raw)?;
        let issuer = payload.get("iss").and_then(Value::as_str).ok_or_else(rejected)?;
        let mut matched = None;
        for provider in self.providers.values() {
            if provider.config.issuer != issuer || provider.config.workloads.is_empty() {
                continue;
            }
            let metadata = self.metadata(provider, false).await?;
            if verify_workload(&provider.config, &metadata, raw).is_err() {
                let refreshed = self.metadata(provider, true).await?;
                if verify_workload(&provider.config, &refreshed, raw).is_err() {
                    continue;
                }
            }
            for workload in &provider.config.workloads {
                if binding_matches(&workload.identity, &payload) {
                    if matched.is_some() {
                        return Err(rejected());
                    }
                    matched = Some(workload.clone());
                }
            }
        }
        matched.map(Some).ok_or_else(rejected)
    }

    fn issue_session(&self, username: &str, expiration: i64) -> Result<LoginSession> {
        let now = Utc::now().timestamp();
        let expires = expiration.min(now + 3600);
        if expires <= now {
            return Err(rejected());
        }
        let token = format!("{SESSION_PREFIX}{}", random_secret()?);
        let mut sessions = self.sessions.lock().expect("OIDC session mutex poisoned");
        sessions.retain(|_, session| session.expires > now);
        if sessions.len() >= MAX_ENTRIES {
            return Err(unavailable());
        }
        sessions.insert(
            super::sha256_hex(token.as_bytes()),
            Session { username: username.to_string(), expires },
        );
        Ok(LoginSession { token, expires })
    }

    fn login_client(
        &self,
        provider: &Provider,
        metadata: CoreProviderMetadata,
    ) -> Result<LoginClient> {
        let config = &provider.config;
        let login = config.login.as_ref().ok_or_else(rejected)?;
        let auth_type = if login.client_secret.is_none() {
            AuthType::RequestBody
        } else if metadata
            .token_endpoint_auth_methods_supported()
            .is_none_or(|methods| methods.contains(&CoreClientAuthMethod::ClientSecretBasic))
        {
            AuthType::BasicAuth
        } else if metadata
            .token_endpoint_auth_methods_supported()
            .is_some_and(|methods| methods.contains(&CoreClientAuthMethod::ClientSecretPost))
        {
            AuthType::RequestBody
        } else {
            return Err(invalid_config(
                "OIDC provider does not support client secret authentication",
            ));
        };
        Ok(CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(config.audience.clone()),
            login.client_secret.clone().map(ClientSecret::new),
        )
        .set_auth_type(auth_type)
        .set_redirect_uri(
            RedirectUrl::new(format!("{}/-/oidc/{}/callback", self.public_url, config.name))
                .map_err(|_| invalid_config("invalid OIDC callback URL"))?,
        ))
    }

    async fn metadata(&self, provider: &Provider, refresh: bool) -> Result<CoreProviderMetadata> {
        let cached = cached_metadata(&*provider.metadata.lock().await, refresh);
        if let Some(metadata) = cached {
            return Ok(metadata);
        }
        let _refresh = provider.refresh.lock().await;
        {
            let mut cache = provider.metadata.lock().await;
            if let Some(metadata) = cached_metadata(&cache, refresh) {
                return Ok(metadata);
            }
            if cache.attempted_at.is_some_and(|at| at.elapsed() < REFRESH_INTERVAL) {
                return Err(unavailable());
            }
            cache.attempted_at = Some(Instant::now());
        }
        let issuer = IssuerUrl::new(provider.config.issuer.clone()).map_err(|_| rejected())?;
        let metadata =
            CoreProviderMetadata::discover_async(issuer, self).await.map_err(|_| unavailable())?;
        secure_url(metadata.authorization_endpoint().as_str())?;
        if let Some(endpoint) = metadata.token_endpoint() {
            secure_url(endpoint.as_str())?;
        }
        provider.metadata.lock().await.value = Some((Instant::now(), metadata.clone()));
        Ok(metadata)
    }

    async fn http_request(&self, request: HttpRequest) -> Result<HttpResponse> {
        secure_url(&request.uri().to_string())?;
        network::validate_destination(&request.uri().to_string())?;
        let (parts, body) = request.into_parts();
        let mut response = self
            .http
            .request(parts.method, parts.uri.to_string())
            .headers(parts.headers)
            .body(body)
            .send()
            .await
            .map_err(|_| unavailable())?;
        let mut builder = openidconnect::http::Response::builder().status(response.status());
        for (key, value) in response.headers() {
            builder = builder.header(key, value);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
            if body.len() + chunk.len() > 1024 * 1024 {
                return Err(unavailable());
            }
            body.extend_from_slice(&chunk);
        }
        builder.body(body).map_err(|_| unavailable())
    }
}

fn cached_metadata(cache: &MetadataCache, refresh: bool) -> Option<CoreProviderMetadata> {
    let recently_attempted = cache.attempted_at.is_some_and(|at| at.elapsed() < REFRESH_INTERVAL);
    cache
        .value
        .as_ref()
        .filter(|(fetched, _)| fetched.elapsed() < METADATA_TTL && (!refresh || recently_attempted))
        .map(|(_, metadata)| metadata.clone())
}

impl<'client> openidconnect::AsyncHttpClient<'client> for OidcState {
    type Error = RegistryError;
    type Future =
        std::pin::Pin<Box<dyn std::future::Future<Output = Result<HttpResponse>> + Send + 'client>>;

    fn call(&'client self, request: HttpRequest) -> Self::Future {
        Box::pin(self.http_request(request))
    }
}

fn verify_workload(
    config: &OidcProvider,
    metadata: &CoreProviderMetadata,
    raw: &str,
) -> Result<()> {
    let token: CoreIdToken = raw.parse().map_err(|_| rejected())?;
    let verifier = token_verifier(config, metadata)?;
    token.claims(&verifier, |_: Option<&Nonce>| Ok(())).map_err(|_| rejected())?;
    validate_claims(config, &token_payload(raw)?)
}

fn token_verifier(
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

fn validate_claims(config: &OidcProvider, payload: &Value) -> Result<()> {
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

fn token_payload(raw: &str) -> Result<Value> {
    if raw.len() > 16 * 1024 {
        return Err(rejected());
    }
    let payload = raw.split('.').nth(1).ok_or_else(rejected)?;
    let bytes = BASE64_URL_SAFE_NO_PAD.decode(payload).map_err(|_| rejected())?;
    serde_json::from_slice(&bytes).map_err(|_| rejected())
}

fn validate_times(payload: &Value) -> Result<()> {
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

fn unique_binding<'binding>(
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

fn binding_matches(binding: &OidcBinding, payload: &Value) -> bool {
    payload.get("sub").and_then(Value::as_str) == Some(binding.subject.as_str())
        && binding.claims.iter().all(|(key, expected)| {
            payload.get(key).and_then(Value::as_str) == Some(expected.as_str())
        })
}

fn validate_provider(config: &OidcProvider) -> Result<()> {
    if config.name.is_empty()
        || config.name.len() > 64
        || !config
            .name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        || config.audience.is_empty()
    {
        return Err(invalid_config(
            "OIDC providers require a name (letters, digits, '-' or '_') and audience",
        ));
    }
    secure_url(&config.issuer)?;
    let mut subjects = HashSet::new();
    for binding in config
        .login
        .iter()
        .flat_map(|login| &login.users)
        .chain(config.workloads.iter().map(|workload| &workload.identity))
    {
        super::validate_username(&binding.username)
            .map_err(|_| invalid_config("invalid OIDC username"))?;
        if binding.subject.is_empty() || !subjects.insert(&binding.subject) {
            return Err(invalid_config(
                "OIDC subjects must be nonempty and unique within each provider",
            ));
        }
    }
    if config.login.as_ref().is_some_and(|login| login.users.is_empty())
        || (config.login.is_none() && config.workloads.is_empty())
    {
        return Err(invalid_config("OIDC providers require explicit user or workload bindings"));
    }
    Ok(())
}

fn secure_url(raw: &str) -> Result<()> {
    let url = Url::parse(raw).map_err(|_| invalid_config("invalid OIDC URL"))?;
    let secure = url.scheme() == "https";
    #[cfg(test)]
    let secure = secure || (url.scheme() == "http" && url.host_str() == Some("127.0.0.1"));
    if !secure
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid_config(
            "OIDC URLs require HTTPS without credentials, query, or fragment",
        ));
    }
    Ok(())
}

fn random_secret() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| unavailable())?;
    Ok(BASE64_URL_SAFE_NO_PAD.encode(bytes))
}

fn invalid_config(reason: &str) -> RegistryError {
    RegistryError::InvalidConfig { reason: reason.to_string() }
}

fn rejected() -> RegistryError {
    RegistryError::Unauthenticated { resource: "OIDC credentials".to_string() }
}

fn unavailable() -> RegistryError {
    RegistryError::Internal { reason: "OIDC provider unavailable".to_string() }
}

#[cfg(test)]
mod tests;
