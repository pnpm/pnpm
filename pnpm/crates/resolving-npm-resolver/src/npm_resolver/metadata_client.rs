use super::{PackageMetaCache, PickPackageContext, RegistryMetadataClient, RegistryMetadataFormat};

impl<Cache: PackageMetaCache> RegistryMetadataClient<Cache> {
    pub(crate) fn pick_context<'a>(
        &'a self,
        format: &'a RegistryMetadataFormat,
        cache_policy: crate::MetadataCachePolicy,
    ) -> PickPackageContext<'a, Cache> {
        PickPackageContext {
            full_metadata: format.full_metadata,
            needs_full_metadata_for: format.needs_full_metadata_for.as_deref(),
            filter_metadata: format.filter_metadata,
            cache_policy,
            metadata: crate::MetadataRequestContext {
                meta_cache: self.meta_cache.as_ref(),
                fetch_locker: &self.fetch_locker,
                cache_dir: self.cache_dir.as_deref(),
                http: crate::MetadataHttpClient {
                    http_client: &self.http_client,
                    auth_headers: &self.auth_headers,
                    retry_opts: self.retry_opts,
                },
            },
        }
    }
}
