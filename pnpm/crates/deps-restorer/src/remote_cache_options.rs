pub use pnpm_pnpr_client::{ArtifactSignerSettings, ArtifactStoreOptions};

use pnpm_config::{Config, RemoteCacheSettings};

/// The [`ArtifactStoreOptions`] that `remoteCache` and `pnprServer` configure.
#[must_use]
pub fn artifact_store_options<'a>(
    config: &'a Config,
    settings: &'a RemoteCacheSettings,
) -> ArtifactStoreOptions<'a> {
    ArtifactStoreOptions {
        remote_cache_url: settings.url.as_deref(),
        team: settings.team.as_deref(),
        pnpr_server: config.pnpr_server.as_deref(),
        auth_headers: &config.auth_headers,
    }
}

/// The [`ArtifactSignerSettings`] that `remoteCache` configures.
#[must_use]
pub fn artifact_signer_settings(settings: &RemoteCacheSettings) -> ArtifactSignerSettings<'_> {
    ArtifactSignerSettings {
        private_key: settings.private_key.as_deref(),
        key_id: settings.key_id.as_deref(),
        builder_id: settings.builder_id.as_deref(),
        image_digest: settings.image_digest.as_deref(),
        architecture_baseline: settings.architecture_baseline.as_deref(),
        build_env: settings.build_env.as_ref(),
    }
}
