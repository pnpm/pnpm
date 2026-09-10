use super::{FetchOutcome, Upstream};
use pnpm_network::{
    ThrottledResponse, UNPRIORITIZED, is_url_secure_for_credentials, read_limited_body,
};
use pnpr_error::{RegistryError, Result};
use reqwest::{StatusCode, Url, header};
use serde::Deserialize;
use std::time::{Duration, Instant};

pub(super) struct CachedToken {
    token: String,
    expires: Instant,
}

impl Upstream {
    /// Fetch an OCI object, negotiating repository-scoped pull credentials.
    /// Credentials never travel to a layer CDN or a redirected token endpoint.
    pub async fn fetch_oci(
        &self,
        repository: &str,
        endpoint: &str,
        accept: &str,
    ) -> Result<FetchOutcome<ThrottledResponse>> {
        self.fetch_oci_request(repository, endpoint, accept, reqwest::Method::GET).await
    }

    /// Read OCI object headers without transferring the body.
    pub async fn head_oci(
        &self,
        repository: &str,
        endpoint: &str,
        accept: &str,
    ) -> Result<FetchOutcome<ThrottledResponse>> {
        self.fetch_oci_request(repository, endpoint, accept, reqwest::Method::HEAD).await
    }

    async fn fetch_oci_request(
        &self,
        repository: &str,
        endpoint: &str,
        accept: &str,
        method: reqwest::Method,
    ) -> Result<FetchOutcome<ThrottledResponse>> {
        self.ensure_available()?;
        let (base, mut url) = self.oci_object_url(repository, endpoint)?;
        let started = Instant::now();
        let mut bearer = self.cached_oci_token(repository);
        let mut negotiated = false;
        for _ in 0..8 {
            self.ensure_allowed_url(url.as_str())?;
            let guard = self
                .client
                .acquire_for_url_without_redirects_with_priority(url.as_str(), UNPRIORITIZED)
                .await;
            let request = guard
                .request(method.clone(), url.clone())
                .timeout(self.timeout.saturating_sub(started.elapsed()))
                .header(header::ACCEPT, accept);
            let request = self.with_oci_credentials(request, &url, &base, bearer.as_deref());
            let response = self.run(request, url.as_str()).await?;
            if response.status() == StatusCode::UNAUTHORIZED
                && !negotiated
                && url.origin() == base.origin()
            {
                let challenge = self.oci_challenge(&response)?;
                drop(response);
                drop(guard);
                bearer = Some(self.oci_token(&base, repository, challenge).await?);
                negotiated = true;
                continue;
            }
            if response.status().is_redirection() {
                url = self.oci_redirect_target(&response, &url, &base)?;
                continue;
            }
            return self.finish_oci_fetch(response, guard, &url, started).await;
        }
        Err(self.oci_error("too many OCI redirects"))
    }

    async fn finish_oci_fetch(
        &self,
        response: reqwest::Response,
        guard: pnpm_network::ThrottledClientGuard<'_>,
        url: &Url,
        started: Instant,
    ) -> Result<FetchOutcome<ThrottledResponse>> {
        if response.status() == StatusCode::NOT_FOUND {
            self.breaker.record_success();
            return Ok(FetchOutcome::NotFound);
        }
        let response = self.checked(response, url.as_str()).await?;
        self.breaker.record_success();
        Ok(FetchOutcome::Ok(
            guard.retain_for_body(response, self.timeout.saturating_sub(started.elapsed())),
        ))
    }

    fn oci_object_url(&self, repository: &str, endpoint: &str) -> Result<(Url, Url)> {
        let base = Url::parse(&self.base).map_err(|_| self.oci_error("invalid registry URL"))?;
        let url = base
            .join(&format!("v2/{repository}/{endpoint}"))
            .map_err(|_| self.oci_error("invalid OCI object URL"))?;
        Ok((base, url))
    }

    fn oci_challenge(&self, response: &reqwest::Response) -> Result<Challenge> {
        response
            .headers()
            .get(header::WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_challenge)
            .ok_or_else(|| self.oci_error("unsupported OCI authentication challenge"))
    }

    fn cached_oci_token(&self, repository: &str) -> Option<String> {
        self.oci_tokens
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(repository)
            .filter(|token| token.expires > Instant::now())
            .map(|token| token.token.clone())
    }

    /// Attach the registry's credentials, but only to the registry itself
    /// over a transport that will not leak them.
    fn with_oci_credentials(
        &self,
        request: reqwest::RequestBuilder,
        url: &Url,
        base: &Url,
        bearer: Option<&str>,
    ) -> reqwest::RequestBuilder {
        if url.origin() != base.origin() || !is_url_secure_for_credentials(url.as_str()) {
            return request;
        }
        let request = request.headers(self.request_headers(url.as_str()));
        match bearer {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }

    /// Where a redirect points, once it is known to stay inside the download
    /// allowlist.
    fn oci_redirect_target(
        &self,
        response: &reqwest::Response,
        url: &Url,
        base: &Url,
    ) -> Result<Url> {
        let target = response
            .headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| url.join(value).ok())
            .ok_or_else(|| self.oci_error("invalid OCI redirect"))?;
        if !oci_download_allowed(base, &target) {
            return Err(self.oci_error("OCI redirect is outside the download allowlist"));
        }
        Ok(target)
    }

    async fn oci_token(
        &self,
        base: &Url,
        repository: &str,
        challenge: Challenge,
    ) -> Result<String> {
        let mut realm =
            Url::parse(&challenge.realm).map_err(|_| self.oci_error("invalid OCI token realm"))?;
        if !token_realm_allowed(base, &realm) {
            return Err(self.oci_error("OCI token realm is not trusted"));
        }
        realm.set_query(None);
        realm
            .query_pairs_mut()
            .append_pair("service", &challenge.service)
            .append_pair("scope", &format!("repository:{repository}:pull"));
        let guard = self
            .client
            .acquire_for_url_without_redirects_with_priority(realm.as_str(), UNPRIORITIZED)
            .await;
        let mut headers = self.request_headers(realm.as_str());
        if realm.origin() != base.origin()
            && is_url_secure_for_credentials(realm.as_str())
            && let Some(authorization) = self.headers.get(header::AUTHORIZATION)
        {
            headers.insert(header::AUTHORIZATION, authorization.clone());
        }
        let response = guard
            .get(realm.clone())
            .timeout(self.timeout)
            .headers(headers)
            .send()
            .await
            .map_err(|source| RegistryError::Upstream { url: self.base.clone(), source })?;
        if !response.status().is_success() {
            return Err(self.oci_error("OCI token service refused authentication"));
        }
        let body = read_limited_body(response, 64 * 1024)
            .await
            .map_err(|source| RegistryError::Upstream { url: self.base.clone(), source })?;
        if body.truncated {
            return Err(self.oci_error("OCI token response is too large"));
        }
        let token: TokenResponse = serde_json::from_slice(&body.bytes)
            .map_err(|_| self.oci_error("invalid OCI token response"))?;
        self.cache_oci_token(repository, token)
    }

    fn cache_oci_token(&self, repository: &str, token: TokenResponse) -> Result<String> {
        let expires_in = token.expires_in.unwrap_or(60).min(3600);
        let token = token
            .token
            .or(token.access_token)
            .filter(|token| !token.is_empty())
            .ok_or_else(|| self.oci_error("OCI token response contains no token"))?;
        let mut cache = self.oci_tokens.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.retain(|_, token| token.expires > Instant::now());
        if cache.len() >= 256 {
            cache.clear();
        }
        if expires_in > 5 {
            cache.insert(
                repository.to_string(),
                CachedToken {
                    token: token.clone(),
                    expires: Instant::now() + Duration::from_secs(expires_in - 5),
                },
            );
        }
        Ok(token)
    }

    fn oci_error(&self, reason: &str) -> RegistryError {
        RegistryError::UpstreamResponse { url: self.base.clone(), reason: reason.to_string() }
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    token: Option<String>,
    access_token: Option<String>,
    expires_in: Option<u64>,
}

struct Challenge {
    realm: String,
    service: String,
}

fn parse_challenge(value: &str) -> Option<Challenge> {
    let (scheme, mut remaining) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("Bearer") {
        return None;
    }
    let mut realm = None;
    let mut service = None;
    while !remaining.trim().is_empty() {
        let (key, tail) = remaining.trim_start().split_once('=')?;
        let tail = tail.trim_start().strip_prefix('"')?;
        let end = tail.find('"')?;
        let value = &tail[..end];
        if value.contains('\\') {
            return None;
        }
        match key.trim() {
            "realm" if realm.is_none() => realm = Some(value.to_string()),
            "service" if service.is_none() => service = Some(value.to_string()),
            "realm" | "service" => return None,
            _ => {}
        }
        remaining = tail[end + 1..].trim_start();
        if !remaining.is_empty() {
            remaining = remaining.strip_prefix(',')?;
        }
    }
    Some(Challenge { realm: realm?, service: service.unwrap_or_default() })
}

fn token_realm_allowed(base: &Url, realm: &Url) -> bool {
    realm.username().is_empty()
        && realm.password().is_none()
        && realm.fragment().is_none()
        && (realm.origin() == base.origin()
            || (base.origin().ascii_serialization() == "https://registry-1.docker.io"
                && realm.origin().ascii_serialization() == "https://auth.docker.io"
                && realm.path() == "/token"))
}

/// The configured origin and the public layer hosts used by Docker Hub and GHCR.
#[must_use]
pub fn oci_download_allowed(base: &Url, target: &Url) -> bool {
    if !matches!(target.scheme(), "http" | "https")
        || !target.username().is_empty()
        || target.password().is_some()
    {
        return false;
    }
    if target.origin() == base.origin() {
        return true;
    }
    let origin = target.origin().ascii_serialization();
    match base.origin().ascii_serialization().as_str() {
        "https://registry-1.docker.io" => matches!(
            origin.as_str(),
            "https://production.cloudflare.docker.com"
                | "https://production.cloudfront.docker.com"
                | "https://docker-images-prod.6aa30f8b08e16409b46e0173d6de2f56.r2.cloudflarestorage.com",
        ),
        "https://ghcr.io" => origin == "https://pkg-containers.githubusercontent.com",
        _ => false,
    }
}

#[cfg(test)]
mod tests;
