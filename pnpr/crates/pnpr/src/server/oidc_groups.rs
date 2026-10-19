//! Team memberships that OIDC sessions earn from their groups claim.

use indexmap::IndexMap;
use pnpr_auth::oidc::SessionUser;
use pnpr_config::{Config, HostedConfig, Management, validate_team_name};
use pnpr_error::RegistryError;
use pnpr_policy::{Identity, Membership};

fn find_hosted<'a>(
    hosted: &'a IndexMap<String, HostedConfig>,
    registry: &str,
) -> Option<&'a HostedConfig> {
    if let Some(exact) = hosted.get(registry) {
        return Some(exact);
    }
    let mut matches = hosted
        .iter()
        .filter(|(key, _)| pnpr_registry::Registries::local_name(key) == registry)
        .map(|(_, config)| config);
    let first = matches.next()?;
    matches
        .next()
        .is_none()
        .then_some(first)
}

/// The identity of a browser session: its username, and a membership for
/// every team its groups claim earned.
pub(super) fn session_identity(config: &Config, user: SessionUser) -> Identity {
    let memberships = user.teams
        .into_iter()
        .filter_map(|grant| {
            let hosted = find_hosted(&config.routing.hosted, &grant.registry)?;
            Some(Membership { directory: hosted.teams.clone(), team: grant.team })
        })
        .collect();
    Identity::member(user.username, memberships)
}

/// Every group grant names a hosted registry and a team it can hold: on a
/// registry whose roster is the YAML, a team the YAML declares.
pub(super) fn validate_group_grants(config: &Config) -> Result<(), RegistryError> {
    let grants = config.identity.auth.oidc
        .iter()
        .filter_map(|provider| provider.login.as_ref()?.groups.as_ref())
        .flat_map(|groups| &groups.teams);
    for grant in grants {
        let invalid = |reason: String| RegistryError::InvalidConfig {
            reason: format!("OIDC group {:?}: {reason}", grant.group),
        };
        let hosted = find_hosted(&config.routing.hosted, &grant.registry)
            .ok_or_else(|| invalid(format!("{:?} is not a hosted registry", grant.registry)))?;
        validate_team_name(&grant.team).map_err(invalid)?;
        if hosted.teams_managed_by == Management::Config
            && !hosted.teams.snapshot().contains_key(&grant.team)
        {
            return Err(invalid(format!(
                "registry {:?} does not declare the team {:?}",
                grant.registry, grant.team,
            )));
        }
    }
    Ok(())
}
