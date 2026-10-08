use pnpr_config::oidc::{OidcLogin, OidcTeamGrant};
use serde_json::Value;

/// The grants whose group the login's groups claim lists in `payload`. A
/// claim that is missing, or neither a string nor a list of strings, lists
/// no group; a list with any other element in it is not a list of strings.
pub(super) fn granted_teams(login: &OidcLogin, payload: &Value) -> Vec<OidcTeamGrant> {
    let Some(groups) = &login.groups else {
        return Vec::new();
    };
    let listed: Vec<&str> = match payload.get(&groups.claim) {
        Some(Value::String(group)) => vec![group.as_str()],
        Some(Value::Array(values)) => values
            .iter()
            .map(Value::as_str)
            .collect::<Option<_>>()
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    groups.teams
        .iter()
        .filter(|grant| listed.contains(&grant.group.as_str()))
        .cloned()
        .collect()
}
