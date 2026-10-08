use pnpr_error::RegistryError;
use serde::Deserialize;
use std::fmt;

/// The YAML `auth.scim:` block, which turns on the SCIM 2.0 endpoints an
/// identity provider uses to deprovision accounts.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScimConfig {
    /// The bearer secret the identity provider sends. It authenticates only
    /// the SCIM endpoints.
    pub token: String,
}

impl ScimConfig {
    /// The shortest `token` pnpr accepts, so a guessable secret cannot
    /// deprovision every account.
    pub const MIN_TOKEN_LEN: usize = 32;

    pub(super) fn validate(&self) -> Result<(), RegistryError> {
        if self.token.len() < Self::MIN_TOKEN_LEN {
            return Err(RegistryError::InvalidConfig {
                reason: format!(
                    "auth.scim.token must be at least {} characters long",
                    Self::MIN_TOKEN_LEN,
                ),
            });
        }
        Ok(())
    }
}

impl fmt::Debug for ScimConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScimConfig")
            .field("token", &"<redacted>")
            .finish()
    }
}
