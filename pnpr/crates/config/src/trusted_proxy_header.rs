use pnpr_error::RegistryError;

/// Request header trusted to convey the client's original IP address.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TrustedProxyHeader {
    /// RFC 7239 `Forwarded` header (`for=...`).
    Forwarded,
    /// `X-Forwarded-For` header (default).
    #[default]
    XForwardedFor,
    /// `X-Real-IP` header.
    XRealIp,
}

impl std::str::FromStr for TrustedProxyHeader {
    type Err = RegistryError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let normalized = raw
            .trim()
            .to_ascii_lowercase()
            .replace('_', "-");
        match normalized.as_str() {
            "forwarded" => Ok(Self::Forwarded),
            "x-forwarded-for" => Ok(Self::XForwardedFor),
            "x-real-ip" => Ok(Self::XRealIp),
            _ => Err(RegistryError::InvalidConfig {
                reason: format!(
                    "unrecognized trusted proxy header {raw:?}; expected 'x-forwarded-for', 'forwarded', or 'x-real-ip'",
                ),
            }),
        }
    }
}

impl std::fmt::Display for TrustedProxyHeader {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Forwarded => write!(formatter, "forwarded"),
            Self::XForwardedFor => write!(formatter, "x-forwarded-for"),
            Self::XRealIp => write!(formatter, "x-real-ip"),
        }
    }
}

impl<'de> serde::Deserialize<'de> for TrustedProxyHeader {
    fn deserialize<Deserializer>(deserializer: Deserializer) -> Result<Self, Deserializer::Error>
    where
        Deserializer: serde::Deserializer<'de>,
    {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

impl serde::Serialize for TrustedProxyHeader {
    fn serialize<Serializer>(
        &self,
        serializer: Serializer,
    ) -> Result<Serializer::Ok, Serializer::Error>
    where
        Serializer: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(test)]
mod tests;
