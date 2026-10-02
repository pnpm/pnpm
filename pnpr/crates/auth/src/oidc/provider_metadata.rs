use super::{
    CoreProviderMetadata, Duration, Instant, IssuerUrl, OidcState, Provider, Result, rejected,
    secure_url, unavailable,
};

const METADATA_TTL: Duration = Duration::from_mins(5);
const REFRESH_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(super) struct MetadataCache {
    pub(super) value: Option<(Instant, CoreProviderMetadata)>,
    pub(super) attempted_at: Option<Instant>,
}

impl OidcState {
    pub(super) async fn metadata(
        &self,
        provider: &Provider,
        refresh: bool,
    ) -> Result<CoreProviderMetadata> {
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
}

fn cached_metadata(cache: &MetadataCache, refresh: bool) -> Option<CoreProviderMetadata> {
    let recently_attempted = cache.attempted_at.is_some_and(|at| at.elapsed() < REFRESH_INTERVAL);
    cache.value
        .as_ref()
        .filter(|(fetched, _)| fetched.elapsed() < METADATA_TTL && (!refresh || recently_attempted))
        .map(|(_, metadata)| metadata.clone())
}
