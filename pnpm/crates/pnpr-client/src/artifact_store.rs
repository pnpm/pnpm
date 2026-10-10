//! Where shared artifacts are looked up, downloaded from, and published to:
//! a pnpr server, or a server that speaks the Turborepo Remote Cache API.
//!
//! Both transports carry the same signed envelopes, and every answer goes
//! through the same client-side verification, so neither server is trusted
//! for what it serves.

pub use turborepo::TurborepoArtifactStore;

use super::{
    ArtifactBlobRequest, ArtifactPublication, BTreeMap, PnprClient, PnprClientError,
    ResolveArtifactsOptions, VerifiedArtifact,
    artifacts::{retain_permitted_candidates, select_verified_artifacts},
};
use pnpm_config::{Config, RemoteCacheSettings};
use std::path::Path;

pub enum ArtifactStore {
    Pnpr { client: PnprClient, authorization: Option<String> },
    Turborepo(TurborepoArtifactStore),
}

impl ArtifactStore {
    /// The server `remoteCache.url` names, or `pnprServer` without one.
    /// `Ok(None)` when neither is set. Credentials are refused for a server
    /// that is neither HTTPS nor loopback.
    pub fn from_config(
        config: &Config,
        settings: &RemoteCacheSettings,
    ) -> Result<Option<ArtifactStore>, PnprClientError> {
        if let Some(url) = settings.url.as_deref() {
            let authorization = config.auth_headers.for_secure_url(url);
            return TurborepoArtifactStore::new(url, settings.team.as_deref(), authorization)
                .map(|store| Some(ArtifactStore::Turborepo(store)))
                .map_err(remote_cache_error);
        }
        let Some(server) = config.pnpr_server.as_deref() else {
            return Ok(None);
        };
        let authorization = config.auth_headers.for_url(server);
        if authorization.is_some() && !pnpm_network::is_url_secure_for_credentials(server) {
            return Err(PnprClientError::Protocol(
                "pnpr artifact credentials require HTTPS or a loopback server".to_string(),
            ));
        }
        Ok(Some(ArtifactStore::Pnpr { client: PnprClient::new(server), authorization }))
    }

    /// The server's URL. Quarantine records are kept per channel, so an
    /// artifact one server served badly is not held against another.
    #[must_use]
    pub fn channel(&self) -> &str {
        match self {
            Self::Pnpr { client, .. } => &client.base_url,
            Self::Turborepo(store) => store.channel(),
        }
    }

    /// Confirm the server can answer artifact requests. A Turborepo server
    /// has no capability handshake, so it is assumed to.
    pub async fn handshake(&self) -> Result<(), PnprClientError> {
        match self {
            Self::Pnpr { client, .. } => client.handshake_artifacts().await,
            Self::Turborepo(_) => Ok(()),
        }
    }

    /// The best verified, compatible artifact of each candidate the server
    /// holds one for.
    pub async fn resolve_artifacts(
        &self,
        mut opts: ResolveArtifactsOptions,
    ) -> Result<BTreeMap<String, VerifiedArtifact>, PnprClientError> {
        match self {
            Self::Pnpr { client, authorization } => {
                opts.authorization.clone_from(authorization);
                client.resolve_artifacts(opts).await
            }
            Self::Turborepo(store) => {
                if !retain_permitted_candidates(&mut opts)? {
                    return Ok(BTreeMap::new());
                }
                let response = store.fetch_artifacts(&opts).await.map_err(remote_cache_error)?;
                select_verified_artifacts(&opts, response)
            }
        }
    }

    /// One blob of an artifact [`Self::resolve_artifacts`] selected, held
    /// to the `size` its manifest declares and checked against its
    /// integrity.
    pub async fn download_artifact_blob(
        &self,
        request: &ArtifactBlobRequest,
        size: u64,
    ) -> Result<Vec<u8>, PnprClientError> {
        match self {
            Self::Pnpr { client, authorization } => {
                client.download_artifact_blob(request, size, authorization.as_deref()).await
            }
            Self::Turborepo(store) => {
                store.download_blob(request, size).await.map_err(remote_cache_error)
            }
        }
    }

    /// [`Self::download_artifact_blob`] into a new file at `destination`,
    /// for a blob too large to hold in memory.
    pub async fn download_artifact_blob_to(
        &self,
        request: &ArtifactBlobRequest,
        size: u64,
        destination: &Path,
    ) -> Result<(), PnprClientError> {
        match self {
            Self::Pnpr { client, authorization } => {
                client.download_artifact_blob_to(
                    request,
                    size,
                    destination,
                    authorization.as_deref(),
                )
                .await
            }
            Self::Turborepo(store) => {
                store.download_blob_to(request, size, destination).await.map_err(remote_cache_error)
            }
        }
    }

    /// Store a signed artifact's blobs, each read from its file, then its
    /// envelope.
    pub async fn publish_artifact(
        &self,
        publication: &ArtifactPublication,
    ) -> Result<(), PnprClientError> {
        match self {
            Self::Pnpr { client, authorization } => {
                client.publish_artifact(publication, authorization.as_deref()).await
            }
            Self::Turborepo(store) => store.publish(publication).await.map_err(remote_cache_error),
        }
    }
}

mod turborepo;

/// Reword a server or network failure of the Turborepo transport, which
/// shares the pnpr error type, so it does not name pnpr. `Protocol` passes
/// through: it marks content the server got wrong, which callers quarantine.
fn remote_cache_error(error: PnprClientError) -> PnprClientError {
    match error {
        PnprClientError::Server(message) => PnprClientError::RemoteCache(message),
        PnprClientError::Http(error) => PnprClientError::RemoteCache(with_causes(&error)),
        other => other,
    }
}

/// `error` followed by each error that caused it, so a failure inside a
/// request body, such as a file that changed, is not reported as a bare
/// send failure.
fn with_causes(error: &(dyn std::error::Error + 'static)) -> String {
    let mut message = error.to_string();
    let mut cause = error.source();
    while let Some(error) = cause {
        message = format!("{message}: {error}");
        cause = error.source();
    }
    message
}
