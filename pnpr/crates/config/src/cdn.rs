use serde::Deserialize;
use std::fmt;

/// Credentials for global purges of the Cloudflare zone serving the registry.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudflareCdnConfig {
    pub zone_id: String,
    pub api_token: String,
    /// A tag unique to this registry deployment within the zone.
    pub cache_tag: String,
}

impl fmt::Debug for CloudflareCdnConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CloudflareCdnConfig")
            .field("zone_id", &self.zone_id)
            .field("api_token", &"[redacted]")
            .field("cache_tag", &self.cache_tag)
            .finish()
    }
}

pub(super) fn build(
    file: Option<CloudflareCdnConfig>,
) -> pnpr_error::Result<Option<CloudflareCdnConfig>> {
    let Some(config) = file else { return Ok(None) };
    if config.zone_id.len() != 32 || !config.zone_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("cloudflareCdn.zoneId must be a 32-character hexadecimal zone ID"));
    }
    if config.api_token.is_empty() || !config.api_token.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(invalid("cloudflareCdn.apiToken must be a nonempty token without whitespace"));
    }
    if config.cache_tag.is_empty()
        || config.cache_tag.len() > 100
        || !config.cache_tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(invalid(
            "cloudflareCdn.cacheTag must contain 1 to 100 ASCII letters, digits, hyphens, or underscores",
        ));
    }
    Ok(Some(config))
}

fn invalid(reason: &str) -> pnpr_error::RegistryError {
    pnpr_error::RegistryError::InvalidConfig { reason: reason.to_string() }
}
