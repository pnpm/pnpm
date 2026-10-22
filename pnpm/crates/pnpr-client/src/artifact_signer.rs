use super::{ArtifactPayload, BuilderProfile, SignedArtifactEnvelope};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use pnpm_config::RemoteCacheSettings;
use pnpm_shared_artifact_protocol::ArtifactProtocolError;

/// What signs the artifacts this machine publishes: `remoteCache.privateKey`
/// and the provenance recorded with it.
pub struct ArtifactSigner {
    key_id: String,
    private_key: Vec<u8>,
    pub builder_id: String,
    pub builder_profile: BuilderProfile,
}

impl ArtifactSigner {
    /// The signer `settings` describe. `Err` names the field that is missing
    /// or malformed.
    pub fn from_settings(settings: &RemoteCacheSettings) -> Result<ArtifactSigner, String> {
        let required = |value: Option<&String>, field: &str| {
            value
                .cloned()
                .ok_or_else(|| format!("remoteCache.publish needs remoteCache.{field}"))
        };
        let private_key = BASE64
            .decode(required(settings.private_key.as_ref(), "privateKey")?)
            .map_err(|_| "remoteCache.privateKey is not valid base64".to_string())?;
        Ok(ArtifactSigner {
            key_id: required(settings.key_id.as_ref(), "keyId")?,
            private_key,
            builder_id: required(settings.builder_id.as_ref(), "builderId")?,
            builder_profile: BuilderProfile {
                image_digest: settings.image_digest.clone(),
                architecture_baseline: settings.architecture_baseline
                    .clone()
                    .unwrap_or_else(|| pnpm_graph_hasher::host_arch().to_string()),
                environment: settings.build_env.clone().unwrap_or_default(),
            },
        })
    }

    pub fn sign(
        &self,
        payload: &ArtifactPayload,
    ) -> Result<SignedArtifactEnvelope, ArtifactProtocolError> {
        SignedArtifactEnvelope::sign(payload, self.key_id.clone(), &self.private_key)
    }
}
