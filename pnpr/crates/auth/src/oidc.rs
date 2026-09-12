mod workload_verification;

mod sessions;

mod provider_config;
use provider_config::{build_providers, secure_url};

mod workload;
use workload::{
    bound_user, match_workload_binding, token_payload, token_verifier, verify_workload,
};

mod network;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as BASE64_URL_SAFE_NO_PAD};
use chrono::Utc;
use openidconnect::{
    AccessTokenHash, AuthType, AuthorizationCode, ClientId, ClientSecret, CsrfToken,
    EndpointMaybeSet, EndpointNotSet, EndpointSet, HttpRequest, HttpResponse, IssuerUrl, Nonce,
    OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, TokenResponse,
    core::{
        CoreAuthenticationFlow, CoreClient, CoreClientAuthMethod, CoreIdToken, CoreIdTokenVerifier,
        CoreJwsSigningAlgorithm, CoreProviderMetadata, CoreTokenResponse,
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
    collections::{HashMap, HashSet, VecDeque},
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex as AsyncMutex, Semaphore};
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
    attempts: Mutex<VecDeque<String>>,
    exchanges: Semaphore,
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
        let providers = build_providers(configs, public_url)?;
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
            attempts: Mutex::new(VecDeque::new()),
            exchanges: Semaphore::new(16),
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
        self.validate_pending_login(&login, provider_name, state, code)?;
        let _exchange = self.exchanges.try_acquire().map_err(|_| unavailable())?;
        self.record_attempt(&login.state_hash)?;
        let provider = self.providers.get(provider_name).ok_or_else(rejected)?;
        let metadata = self.metadata(provider, false).await?;
        let response = self
            .login_client(provider, metadata.clone())?
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .map_err(|_| rejected())?
            .set_pkce_verifier(PkceCodeVerifier::new(login.verifier))
            .request_async(self)
            .await
            .map_err(|_| rejected())?;
        let token = response.id_token().ok_or_else(rejected)?;
        let expiration = self
            .verified_expiration(provider, &metadata, &response, token, &Nonce::new(login.nonce))
            .await?;
        let binding = bound_user(&provider.config, token)?;
        let now = Utc::now().timestamp();
        if login.expires <= now {
            return Err(rejected());
        }
        let mut consumed = self.consumed.lock().expect("OIDC consumed mutex poisoned");
        consumed.retain(|_, expires| *expires > now);
        if consumed.contains_key(&login.state_hash) || consumed.len() >= MAX_ENTRIES {
            return Err(rejected());
        }
        let session = self.issue_session(&binding.username, expiration)?;
        consumed.insert(login.state_hash, login.expires);
        Ok(session)
    }

    fn validate_pending_login(
        &self,
        login: &PendingLogin,
        provider_name: &str,
        state: &str,
        code: &str,
    ) -> Result<()> {
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
        Ok(())
    }

    /// Verify the ID token against the provider's keys, refreshing them once
    /// when the cached keys reject it, and check the access token hash it
    /// carries. Returns the verified claims' expiration.
    async fn verified_expiration(
        &self,
        provider: &Provider,
        metadata: &CoreProviderMetadata,
        response: &CoreTokenResponse,
        token: &CoreIdToken,
        nonce: &Nonce,
    ) -> Result<i64> {
        let mut verifier = token_verifier(&provider.config, metadata)?;
        if token.claims(&verifier, nonce).is_err() {
            verifier = token_verifier(&provider.config, &self.metadata(provider, true).await?)?;
        }
        let claims = token.claims(&verifier, nonce).map_err(|_| rejected())?;
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
        Ok(claims.expiration().timestamp())
    }

    fn record_attempt(&self, state_hash: &str) -> Result<()> {
        let mut attempts = self.attempts.lock().expect("OIDC attempts mutex poisoned");
        if attempts.iter().any(|state| state == state_hash) {
            return Err(rejected());
        }
        if attempts.len() >= MAX_ENTRIES {
            attempts.pop_front();
        }
        attempts.push_back(state_hash.to_string());
        Ok(())
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
