use super::{ArtifactPayload, BuilderProfile, SignedArtifactEnvelope};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use pnpm_shared_artifact_protocol::ArtifactProtocolError;
use std::collections::BTreeMap;

/// What signs the artifacts this machine publishes: `remoteCache.privateKey`
/// and the provenance recorded with it.
pub struct ArtifactSigner {
    key_id: String,
    private_key: Vec<u8>,
    pub builder_id: String,
    pub builder_profile: BuilderProfile,
}

/// The `remoteCache` fields that describe an [`ArtifactSigner`], named after
/// them.
#[derive(Default, Clone, Copy)]
pub struct ArtifactSignerSettings<'a> {
    pub private_key: Option<&'a str>,
    pub key_id: Option<&'a str>,
    pub builder_id: Option<&'a str>,
    pub image_digest: Option<&'a str>,
    pub architecture_baseline: Option<&'a str>,
    pub build_env: Option<&'a BTreeMap<String, String>>,
}

impl ArtifactSigner {
    /// The signer `settings` describe. `Err` names the field that is missing
    /// or malformed.
    pub fn from_settings(settings: ArtifactSignerSettings<'_>) -> Result<ArtifactSigner, String> {
        let required = |value: Option<&str>, field: &str| {
            value
                .map(str::to_owned)
                .ok_or_else(|| format!("remoteCache.publish needs remoteCache.{field}"))
        };
        let private_key = BASE64
            .decode(required(settings.private_key, "privateKey")?)
            .map_err(|_| "remoteCache.privateKey is not valid base64".to_string())?;
        Ok(ArtifactSigner {
            key_id: required(settings.key_id, "keyId")?,
            private_key,
            builder_id: required(settings.builder_id, "builderId")?,
            builder_profile: BuilderProfile {
                image_digest: settings.image_digest.map(str::to_owned),
                architecture_baseline: settings.architecture_baseline
                    .map(str::to_owned)
                    .unwrap_or_else(|| pnpm_graph_hasher::host_arch().to_string()),
                environment: settings.build_env.cloned().unwrap_or_default(),
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
