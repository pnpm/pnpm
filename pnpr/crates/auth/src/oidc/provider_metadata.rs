use super::{
    CoreProviderMetadata, Duration, Instant, IssuerUrl, OidcState, Provider, Result, rejected,
    secure_url, unavailable,
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

#[derive(Default)]
pub(super) struct MetadataCache {
    pub(super) value: Option<(Instant, CoreProviderMetadata)>,
    pub(super) attempted_at: Option<Instant>,
}

impl OidcState {
    pub(super) async fn metadata(
        &self,
        provider: &Provider,
        freshness: MetadataFreshness,
    ) -> Result<CoreProviderMetadata> {
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
}

fn cached_metadata(
    cache: &MetadataCache,
    freshness: MetadataFreshness,
) -> Option<CoreProviderMetadata> {
    let unexpired = cache.value
        .as_ref()
        .filter(|(fetched, _)| fetched.elapsed() < METADATA_TTL);
    match freshness {
        MetadataFreshness::Cached => unexpired,
        MetadataFreshness::Refetched => unexpired.filter(|_| {
            cache.attempted_at.is_some_and(|at| at.elapsed() < REFRESH_INTERVAL)
        }),
    }
    .map(|(_, metadata)| metadata.clone())
}
