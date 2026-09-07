use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OidcProvider {
    pub name: String,
    pub issuer: String,
    pub audience: String,
    pub login: Option<OidcLogin>,
    #[serde(default)]
    pub workloads: Vec<OidcWorkload>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OidcLogin {
    pub client_secret: Option<String>,
    pub users: Vec<OidcBinding>,
}

impl std::fmt::Debug for OidcLogin {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("OidcLogin").field("users", &self.users).finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OidcBinding {
    pub subject: String,
    pub username: String,
    #[serde(default)]
    pub claims: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OidcWorkload {
    pub identity: OidcBinding,
    pub registry: String,
    pub packages: Vec<String>,
}
