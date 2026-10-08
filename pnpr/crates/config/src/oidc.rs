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
    /// Team memberships a signed-in session gets from the groups its ID token
    /// lists.
    #[serde(default)]
    pub groups: Option<OidcGroups>,
}

/// Which ID-token claim lists a user's groups, and which teams each group
/// grants. A membership lasts as long as the session that proved it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OidcGroups {
    /// A claim holding a list of strings, or one string. Omitted ⇒ `groups`.
    #[serde(default = "default_groups_claim")]
    pub claim: String,
    pub teams: Vec<OidcTeamGrant>,
}

/// A session whose groups claim lists `group` is a member of `team` on the
/// hosted registry `registry`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OidcTeamGrant {
    pub group: String,
    pub registry: String,
    pub team: String,
}

fn default_groups_claim() -> String {
    "groups".to_string()
}

impl std::fmt::Debug for OidcLogin {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OidcLogin")
            .field("users", &self.users)
            .field("groups", &self.groups)
            .finish_non_exhaustive()
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
