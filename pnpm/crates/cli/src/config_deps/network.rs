use pnpm_config::Config;
use pnpm_network::{RetryOpts, ThrottledClient};
use pnpm_resolving_npm_resolver::{
    InMemoryPackageMetaCache, NpmResolver, shared_packument_fetch_locker,
    shared_picked_manifest_cache,
};
use std::{collections::HashMap, sync::Arc};

pub(super) struct EnvironmentNetwork {
    pub(super) http_client: Arc<ThrottledClient>,
    pub(super) auth_headers: Arc<pnpm_network::AuthHeaders>,
    pub(super) registries: HashMap<String, String>,
    pub(super) retry_opts: RetryOpts,
    pub(super) offline: bool,
}

impl EnvironmentNetwork {
    pub(super) fn resolver(&self, config: &Config) -> NpmResolver<InMemoryPackageMetaCache> {
        NpmResolver {
            registries: self.registries.clone(),
            registries_by_prefix: HashMap::new(),
            metadata: pnpm_resolving_npm_resolver::RegistryMetadataClient {
                http_client: Arc::clone(&self.http_client),
                auth_headers: Arc::clone(&self.auth_headers),
                meta_cache: Arc::new(InMemoryPackageMetaCache::default()),
                fetch_locker: shared_packument_fetch_locker(),
                picked_manifest_cache: shared_picked_manifest_cache(),
                cache_dir: Some(config.cache_dir.clone()),
                retry_opts: self.retry_opts,
            },
            format: pnpm_resolving_npm_resolver::RegistryMetadataFormat {
                // Match the install resolver's metadata policy so release-age and
                // trust checks receive full packument metadata when required.
                full_metadata: config.requires_full_metadata_for_resolution(),
                needs_full_metadata_for: None,
                filter_metadata: config.requires_full_metadata_for_resolution(),
            },
            cache_policy: pnpm_resolving_npm_resolver::MetadataCachePolicy {
                offline: self.offline,
                prefer_offline: config.prefer_offline,
                ignore_missing_time_field: config.minimum_release_age_ignore_missing_time,
            },
            store_view: pnpm_resolving_npm_resolver::OfflineStoreView::open_for_offline(
                self.offline,
                &config.store_dir,
                config.frozen_store,
            ),
        }
    }
}
