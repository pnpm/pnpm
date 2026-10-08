use super::{AppState, Config, IndexMap, RegistryError};
use pnpr_config::{Teams, TeamsManagement};
use pnpr_storage::DocumentWrite;
use serde::{Deserialize, Serialize};
use std::{
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

/// How long a replica serves the team rosters it last read before it reads
/// them from the hosted store again. Bounds how long a roster change made on
/// another replica takes to reach this one.
const ROSTER_TTL: Duration = Duration::from_secs(10);

/// How many times a roster edit rereads the stored roster after another
/// writer replaced it first.
const ROSTER_WRITE_ATTEMPTS: usize = 5;

/// The rosters of the hosted registries whose teams the team API manages.
///
/// The hosted store holds each such roster, so every replica sees the same
/// one. A registry with no stored roster yet serves its `teams:` map, and its
/// first edit stores the edited map.
pub(super) struct TeamRosters {
    /// Each managed registry's `teams:` map, keyed by registry name.
    seeds: IndexMap<String, Teams>,
    loaded_at: Mutex<Option<Instant>>,
    reload: tokio::sync::Mutex<()>,
}

impl TeamRosters {
    pub(super) fn new(config: &Config) -> Self {
        let seeds = config.routing.hosted
            .iter()
            .filter(|(_, hosted)| hosted.teams_managed_by == TeamsManagement::Api)
            .map(|(name, hosted)| (name.clone(), Teams::clone(&hosted.teams.snapshot())))
            .collect();
        Self { seeds, loaded_at: Mutex::new(None), reload: tokio::sync::Mutex::new(()) }
    }

    fn is_fresh(&self) -> bool {
        self.loaded_at
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some_and(|loaded_at| loaded_at.elapsed() < ROSTER_TTL)
    }

    fn mark_loaded(&self) {
        *self.loaded_at.lock().unwrap_or_else(PoisonError::into_inner) = Some(Instant::now());
    }
}

/// Reload every managed roster from the hosted store once the loaded copies
/// are older than [`ROSTER_TTL`]. Requests that arrive during a reload wait
/// for it, so no request is authorized against a roster this replica has not
/// read yet.
pub(super) async fn refresh_team_rosters(state: &AppState) -> Result<(), RegistryError> {
    let rosters = &state.inner.identity.teams;
    if rosters.seeds.is_empty() || rosters.is_fresh() {
        return Ok(());
    }
    let _reload = rosters.reload.lock().await;
    if rosters.is_fresh() {
        return Ok(());
    }
    for (registry, seed) in &rosters.seeds {
        let teams = match state.inner.storage.read_team_roster(registry).await? {
            Some(bytes) => parse_roster(registry, &bytes)?,
            None => seed.clone(),
        };
        publish_roster(state, registry, teams);
    }
    rosters.mark_loaded();
    Ok(())
}

/// Apply `edit` to the stored roster of `registry` and store the result,
/// rereading and reapplying it when another writer replaced the roster
/// first. The edited roster takes effect on this replica at once.
pub(super) async fn update_team_roster<Edit>(
    state: &AppState,
    registry: &str,
    edit: Edit,
) -> Result<(), RegistryError>
where
    Edit: Fn(&mut Teams) -> Result<(), RegistryError>,
{
    let seed = state.inner.identity.teams.seeds.get(registry).ok_or(RegistryError::NotFound)?;
    for _ in 0..ROSTER_WRITE_ATTEMPTS {
        let stored = state.inner.storage.read_team_roster(registry).await?;
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
    Err(RegistryError::TeamConflict {
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
    let storage = &state.inner.storage;
    match expected {
        None => storage.create_team_roster(registry, &bytes).await,
        Some(expected) => {
            let written = storage.replace_team_roster_if_current(registry, expected, &bytes).await?;
            Ok(written == DocumentWrite::Written)
        }
    }
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
