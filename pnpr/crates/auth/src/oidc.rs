pub use handoffs::HANDOFF_TTL;
pub use sessions::{LoginReturn, LoginSession};
pub use web_login::{WEB_LOGIN_TTL, WebLoginPoll, WebLoginTokens};

mod workload_verification;

mod groups;
mod handoffs;
mod sessions;
mod web_login;
use sessions::BrowserSessions;

mod provider_config;
use provider_config::{build_providers, secure_url};

mod workload;
use workload::{
    bound_user, match_workload_binding, token_payload, token_verifier, verify_workload,
};

mod network;

mod login_client;

mod pending_login;
use pending_login::PendingLogin;

mod provider_metadata;
use provider_metadata::{MetadataCache, MetadataFreshness};

mod transport;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as BASE64_URL_SAFE_NO_PAD};
use chrono::Utc;
use openidconnect::{
    AccessTokenHash, AuthType, AuthorizationCode, ClientId, ClientSecret, CsrfToken,
    EndpointMaybeSet, EndpointNotSet, EndpointSet, HttpRequest, HttpResponse, IssuerUrl,
    JsonWebKeySetUrl, Nonce, OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl,
    TokenResponse,
    core::{
        CoreAuthenticationFlow, CoreClient, CoreClientAuthMethod, CoreIdToken, CoreIdTokenVerifier,
        CoreJsonWebKeySet, CoreJwsSigningAlgorithm, CoreProviderMetadata, CoreTokenResponse,
    },
};
use p256::ecdsa::{Signature, SigningKey};
use pnpr_config::oidc::{OidcBinding, OidcProvider, OidcTeamGrant, OidcWorkload};
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
const SESSION_PREFIX: &str = "pnpr_oidc_";

pub struct OidcState {
    providers: HashMap<String, Provider>,
    http: reqwest::Client,
    public_url: String,
    state_key: SigningKey,
    consumed: Mutex<HashMap<String, i64>>,
    attempts: Mutex<VecDeque<String>>,
    exchanges: Semaphore,
    browser: BrowserSessions,
}

struct Provider {
    config: OidcProvider,
    metadata: AsyncMutex<MetadataCache>,
    refresh: AsyncMutex<()>,
}

/// Who a browser session signed in as.
#[derive(Debug, Clone)]
pub struct SessionUser {
    pub username: String,
    /// The team grants the session's groups claim earned at sign-in.
    pub teams: Vec<OidcTeamGrant>,
}

pub struct LoginStart {
    pub url: String,
    pub state: String,
    pub browser_secret: String,
}

impl OidcState {
    pub fn new(configs: &[OidcProvider], public_url: &str) -> Result<Self> {
        let providers = build_providers(configs, public_url)?;
        let state_key =
            SigningKey::from_bytes((&super::fresh_secret()).into()).map_err(|_| unavailable())?;
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .dns_resolver(network::public_resolver())
            .tls_certs_merge(pnpm_network::bundled_root_certs().iter().cloned())
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
            browser: BrowserSessions::default(),
        })
    }

    pub async fn start(&self, provider_name: &str, returns_to: LoginReturn) -> Result<LoginStart> {
        let provider = self.providers.get(provider_name).ok_or(RegistryError::NotFound)?;
        if provider.config.login.is_none() {
            return Err(RegistryError::NotFound);
        }
        let metadata = self.metadata(provider, MetadataFreshness::Cached).await?;
        let client = self.login_client(provider, metadata.login.clone().ok_or_else(unavailable)?)?;
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
            returns_to,
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
        let metadata = self.metadata(provider, MetadataFreshness::Cached).await?;
        let response = self
            .login_client(provider, metadata.login.clone().ok_or_else(unavailable)?)?
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .map_err(|_| rejected())?
            .set_pkce_verifier(PkceCodeVerifier::new(login.verifier))
            .request_async(self)
            .await
            .map_err(|_| rejected())?;
        let token = response.id_token().ok_or_else(rejected)?;
        let expiration = self.verified_expiration(
            provider,
            &metadata.jwks,
            &response,
            token,
            &Nonce::new(login.nonce),
        )
        .await?;
        let user = bound_user(&provider.config, token)?;
        let now = Utc::now().timestamp();
        if login.expires <= now {
            return Err(rejected());
        }
        let mut consumed = self.consumed.lock().expect("OIDC consumed mutex poisoned");
        consumed.retain(|_, expires| *expires > now);
        if consumed.contains_key(&login.state_hash) || consumed.len() >= MAX_ENTRIES {
            return Err(rejected());
        }
        let session = self.issue_session(user, expiration, login.returns_to)?;
        consumed.insert(login.state_hash, login.expires);
        Ok(session)
    }

    /// Verify the ID token against the provider's keys, refreshing them once
    /// when the cached keys reject it, and check the access token hash it
    /// carries. Returns the verified claims' expiration.
    async fn verified_expiration(
        &self,
        provider: &Provider,
        jwks: &CoreJsonWebKeySet,
        response: &CoreTokenResponse,
        token: &CoreIdToken,
        nonce: &Nonce,
    ) -> Result<i64> {
        let mut verifier = token_verifier(&provider.config, jwks)?;
        if token.claims(&verifier, nonce).is_err() {
            verifier = token_verifier(
                &provider.config,
                &self.metadata(provider, MetadataFreshness::Refetched).await?.jwks,
            )?;
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
