//! Discovery of a provider's metadata, and the per-provider cache that
//! keeps one copy alive for [`METADATA_TTL`] while [`REFRESH_INTERVAL`] bounds
//! how often a failed discovery is retried.

use std::sync::Arc;

use super::{
    CoreJsonWebKeySet, CoreProviderMetadata, Deserialize, Duration, Instant, IssuerUrl,
    JsonWebKeySetUrl, OidcState, Provider, Result, rejected, secure_url, unavailable,
};
use openidconnect::{
    AsyncHttpClient as _,
    http::{Method, Request, StatusCode, header::ACCEPT},
};

const METADATA_TTL: Duration = Duration::from_mins(5);
const REFRESH_INTERVAL: Duration = Duration::from_secs(30);

/// How fresh a provider's metadata has to be. `Refetched` asks for a new
/// discovery, and takes the cached copy only when one was attempted within
/// [`REFRESH_INTERVAL`], which bounds the retries a rejected token triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MetadataFreshness {
    Cached,
    Refetched,
}

/// A provider's discovered metadata. Workload verification needs only the
/// signing keys, so issuers without an interactive flow, such as GitHub
/// Actions, need not publish an `authorization_endpoint`.
pub(super) struct ProviderMetadata {
    pub(super) jwks: CoreJsonWebKeySet,
    /// The full metadata, present only for a provider with `login` configured.
    pub(super) login: Option<CoreProviderMetadata>,
}

/// The discovery document fields every provider must publish.
#[derive(Deserialize)]
struct SigningKeysDocument {
    issuer: IssuerUrl,
    jwks_uri: JsonWebKeySetUrl,
}

#[derive(Default)]
pub(super) struct MetadataCache {
    pub(super) value: Option<(Instant, Arc<ProviderMetadata>)>,
    pub(super) attempted_at: Option<Instant>,
}

impl OidcState {
    pub(super) async fn metadata(
        &self,
        provider: &Provider,
        freshness: MetadataFreshness,
    ) -> Result<Arc<ProviderMetadata>> {
        let cached = cached_metadata(&*provider.metadata.lock().await, freshness);
        if let Some(metadata) = cached {
            return Ok(metadata);
        }
        let _refresh = provider.refresh.lock().await;
        {
            let mut cache = provider.metadata.lock().await;
            if let Some(metadata) = cached_metadata(&cache, freshness) {
                return Ok(metadata);
            }
            if cache.attempted_at.is_some_and(|at| at.elapsed() < REFRESH_INTERVAL) {
                return Err(unavailable());
            }
            cache.attempted_at = Some(Instant::now());
        }
        let metadata = Arc::new(self.discover(provider).await?);
        provider.metadata.lock().await.value = Some((Instant::now(), Arc::clone(&metadata)));
        Ok(metadata)
    }

    async fn discover(&self, provider: &Provider) -> Result<ProviderMetadata> {
        let issuer = IssuerUrl::new(provider.config.issuer.clone()).map_err(|_| rejected())?;
        let url = issuer.join(".well-known/openid-configuration").map_err(|_| rejected())?;
        let request = Request::builder()
            .method(Method::GET)
            .uri(url.as_str())
            .header(ACCEPT, "application/json")
            .body(Vec::new())
            .map_err(|_| unavailable())?;
        let response = self.call(request).await?;
        if response.status() != StatusCode::OK {
            return Err(unavailable());
        }
        let document: SigningKeysDocument =
            serde_json::from_slice(response.body()).map_err(|_| unavailable())?;
        if document.issuer != issuer {
            return Err(unavailable());
        }
        let jwks = CoreJsonWebKeySet::fetch_async(&document.jwks_uri, self)
            .await
            .map_err(|_| unavailable())?;
        let login = match provider.config.login {
            Some(_) => Some(login_metadata(response.body(), jwks.clone())?),
            None => None,
        };
        Ok(ProviderMetadata { jwks, login })
    }
}

fn login_metadata(document: &[u8], jwks: CoreJsonWebKeySet) -> Result<CoreProviderMetadata> {
    let metadata: CoreProviderMetadata =
        serde_json::from_slice(document).map_err(|_| unavailable())?;
    secure_url(metadata.authorization_endpoint().as_str())?;
    if let Some(endpoint) = metadata.token_endpoint() {
        secure_url(endpoint.as_str())?;
    }
    Ok(metadata.set_jwks(jwks))
}

fn cached_metadata(
    cache: &MetadataCache,
    freshness: MetadataFreshness,
) -> Option<Arc<ProviderMetadata>> {
    let unexpired = cache.value
        .as_ref()
        .filter(|(fetched, _)| fetched.elapsed() < METADATA_TTL);
    match freshness {
        MetadataFreshness::Cached => unexpired,
        MetadataFreshness::Refetched => unexpired.filter(|_| {
            cache.attempted_at.is_some_and(|at| at.elapsed() < REFRESH_INTERVAL)
        }),
    }
    .map(|(_, metadata)| Arc::clone(metadata))
}
