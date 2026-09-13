use super::{
    AccessTokenHash, AuthorizationCode, CoreAuthenticationFlow, CoreIdToken, CoreProviderMetadata,
    CoreTokenResponse, CsrfToken, LOGIN_TTL, LoginSession, LoginStart, MAX_ENTRIES, Nonce,
    OAuth2TokenResponse, OidcState, PendingLogin, PkceCodeChallenge, PkceCodeVerifier, Provider,
    RegistryError, Result, TokenResponse, Utc, bound_user, rejected, token_verifier, unavailable,
};

impl OidcState {
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
            state_hash: super::super::sha256_hex(state.secret().as_bytes()),
            expires: Utc::now().timestamp() + LOGIN_TTL.as_secs() as i64,
        };
        let browser_secret = self.seal_login(&pending)?;
        Ok(LoginStart {
            url: url.to_string(),
            state: state.secret().clone(),
            browser_secret,
        })
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
        let _exchange = self.exchanges
            .try_acquire()
            .map_err(|_| unavailable())?;
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
        let expiration = self.verified_expiration(
            provider,
            &metadata,
            &response,
            token,
            &Nonce::new(login.nonce),
        )
        .await?;
        let binding = bound_user(&provider.config, token)?;
        self.claim_login_session(
            login.state_hash,
            login.expires,
            &binding.username,
            expiration,
        )
    }

    fn claim_login_session(
        &self,
        state_hash: String,
        expires: i64,
        username: &str,
        expiration: i64,
    ) -> Result<LoginSession> {
        let now = Utc::now().timestamp();
        if expires <= now {
            return Err(rejected());
        }
        let mut consumed = self.consumed.lock().expect("OIDC consumed mutex poisoned");
        consumed.retain(|_, expires| *expires > now);
        if consumed.contains_key(&state_hash) || consumed.len() >= MAX_ENTRIES {
            return Err(rejected());
        }
        let session = self.issue_session(username, expiration)?;
        consumed.insert(state_hash, expires);
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
            || login.state_hash != super::super::sha256_hex(state.as_bytes())
            || code.is_empty()
            || code.len() > 8192
        {
            return Err(rejected());
        }
        if self.consumed
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
        let claims = token
            .claims(&verifier, nonce)
            .map_err(|_| rejected())?;
        if let Some(expected) = claims.access_token_hash() {
            let actual = AccessTokenHash::from_token(
                response.access_token(),
                token
                    .signing_alg()
                    .map_err(|_| rejected())?,
                token
                    .signing_key(&verifier)
                    .map_err(|_| rejected())?,
            )
            .map_err(|_| rejected())?;
            if actual != *expected {
                return Err(rejected());
            }
        }
        Ok(claims.expiration().timestamp())
    }
}
