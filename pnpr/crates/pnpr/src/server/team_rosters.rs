use super::{
    AppState, RegistryError,
    managed_state::{WRITE_ATTEMPTS, store_registry_record},
};
use pnpr_config::Teams;
use pnpr_storage::RegistryRecord;
use serde::{Deserialize, Serialize};

/// Publish one registry's stored roster, or an empty one when it cannot be
/// read. Reports whether the read succeeded.
pub(super) async fn reload_roster(state: &AppState, registry: &str, seed: &Teams) -> bool {
    let (teams, read) = match read_roster(state, registry, seed).await {
        Ok(teams) => (teams, true),
        Err(err) => {
            tracing::error!(registry, error = %err, "could not read a team roster; its teams admit nobody");
            (Teams::default(), false)
        }
    };
    publish_roster(state, registry, teams);
    read
}

async fn read_roster(
    state: &AppState,
    registry: &str,
    seed: &Teams,
) -> Result<Teams, RegistryError> {
    match state.inner.storage.read_registry_record(RegistryRecord::TeamRoster, registry).await? {
        Some(bytes) => parse_roster(registry, &bytes),
        None => Ok(seed.clone()),
    }
}

/// Apply `edit` to the stored roster of `registry` and store the result,
/// rereading and reapplying it when another writer replaced the roster
/// first. The edited roster takes effect on this replica at once. The edit
/// holds the reload lock, so a reload that read the store before the edit
/// cannot publish its older copy after it.
pub(super) async fn update_team_roster<Edit>(
    state: &AppState,
    registry: &str,
    edit: Edit,
) -> Result<(), RegistryError>
where
    Edit: Fn(&mut Teams) -> Result<(), RegistryError>,
{
    let managed = &state.inner.identity.managed;
    let seed = managed.roster_seeds.get(registry).ok_or(RegistryError::NotFound)?;
    let _reload = managed.reload.lock().await;
    for _ in 0..WRITE_ATTEMPTS {
        let stored =
            state.inner.storage.read_registry_record(RegistryRecord::TeamRoster, registry).await?;
        let mut teams = match &stored {
            Some(bytes) => parse_roster(registry, bytes)?,
            None => seed.clone(),
        };
        edit(&mut teams)?;
        if store_roster(state, registry, stored.as_deref(), &teams).await? {
            publish_roster(state, registry, teams);
            return Ok(());
        }
    }
    Err(RegistryError::AdminConflict {
        reason: format!("the teams of registry {registry:?} kept changing; retry the request"),
    })
}

async fn store_roster(
    state: &AppState,
    registry: &str,
    expected: Option<&[u8]>,
    teams: &Teams,
) -> Result<bool, RegistryError> {
    let bytes = serde_json::to_vec(&RosterRecord::from(teams))?;
    store_registry_record(state, RegistryRecord::TeamRoster, registry, expected, &bytes).await
}

fn publish_roster(state: &AppState, registry: &str, teams: Teams) {
    if let Some(hosted) = state.inner.config.routing.hosted.get(registry) {
        hosted.teams.replace(teams);
    }
}

fn parse_roster(registry: &str, bytes: &[u8]) -> Result<Teams, RegistryError> {
    let record: RosterRecord = serde_json::from_slice(bytes)
        .map_err(|err| RegistryError::Internal {
            reason: format!("the stored teams of registry {registry:?} are unreadable: {err}"),
        })?;
    Ok(record.teams
        .into_iter()
        .map(|team| (team.name, team.members.into_iter().collect()))
        .collect())
}

/// The stored shape of a roster. Teams are a list, not a map, so their order
/// survives a round trip.
#[derive(Serialize, Deserialize)]
struct RosterRecord {
    teams: Vec<RosterTeam>,
}

#[derive(Serialize, Deserialize)]
struct RosterTeam {
    name: String,
    members: Vec<String>,
}

impl From<&Teams> for RosterRecord {
    fn from(teams: &Teams) -> Self {
        let teams = teams
            .iter()
            .map(|(name, members)| RosterTeam {
                name: name.clone(),
                members: members.iter().cloned().collect(),
            })
            .collect();
        Self { teams }
    }
}
