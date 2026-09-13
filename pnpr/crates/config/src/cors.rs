use super::{CorsConfig, CorsFile, RegistryError};
pub(super) fn build_cors_config(file: CorsFile) -> Result<CorsConfig, RegistryError> {
    CorsConfig::from_allowed_origins(file.allowed_origins)
}

pub(super) fn normalize_cors_origin(raw: &str) -> Result<String, RegistryError> {
    let parsed = url::Url::parse(raw)
        .map_err(|_| RegistryError::InvalidConfig {
            reason: format!("CORS allowed origin {raw:?} is not an absolute URL"),
        })?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/"
    {
        return Err(RegistryError::InvalidConfig {
            reason: format!(
                "CORS allowed origin {raw:?} must contain only an http(s) scheme, host, and optional port",
            ),
        });
    }
    Ok(parsed.origin().ascii_serialization())
}
