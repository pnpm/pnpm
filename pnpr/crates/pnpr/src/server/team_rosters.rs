use super::{AppState, Config, IndexMap, RegistryError};
use futures_util::future::join_all;
use pnpr_config::{Teams, TeamsManagement};
use pnpr_storage::DocumentWrite;
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use tokio::sync::OwnedMutexGuard;

/// How long a replica serves the team rosters it last read before it reads
/// them from the hosted store again. Bounds how long a roster change made on
/// another replica takes to reach this one.
const ROSTER_TTL: Duration = Duration::from_secs(10);

/// How soon a replica tries again after a roster it could not read.
const ROSTER_RETRY: Duration = Duration::from_secs(1);

/// How many times a roster edit rereads the stored roster after another
/// writer replaced it first.
const ROSTER_WRITE_ATTEMPTS: usize = 5;

/// The rosters of the hosted registries whose teams the team API manages.
///
/// The hosted store holds each such roster, so every replica sees the same
/// one. A registry with no stored roster yet serves its `teams:` map, and its
/// first edit stores the edited map. A roster this replica cannot read is
/// served empty, so its `team:` grants admit nobody until a read succeeds.
pub(super) struct TeamRosters {
    /// Each managed registry's `teams:` map, keyed by registry name.
    seeds: IndexMap<String, Teams>,
    /// When the rosters are next due for a reload. `None` before the first.
    due_at: Mutex<Option<Instant>>,
    reload: Arc<tokio::sync::Mutex<()>>,
}

impl TeamRosters {
    pub(super) fn new(config: &Config) -> Self {
        let seeds = config.routing.hosted
            .iter()
            .filter(|(_, hosted)| hosted.teams_managed_by == TeamsManagement::Api)
            .map(|(name, hosted)| (name.clone(), Teams::clone(&hosted.teams.snapshot())))
            .collect();
        Self { seeds, due_at: Mutex::new(None), reload: Arc::default() }
    }

    fn due_at(&self) -> Option<Instant> {
        *self.due_at.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn schedule(&self, after: Duration) {
        *self.due_at.lock().unwrap_or_else(PoisonError::into_inner) = Some(Instant::now() + after);
    }
}

/// Reload the managed rosters once they are due. The first load after
/// startup holds up the request, so no request is authorized against a
/// roster this replica has not read. Later reloads run in the background
/// while requests keep the current rosters.
pub(super) async fn refresh_team_rosters(state: &AppState) {
    let rosters = &state.inner.identity.teams;
    if rosters.seeds.is_empty() {
        return;
    }
    match rosters.due_at() {
        Some(due_at) if Instant::now() < due_at => {}
        Some(_) => {
            if let Ok(guard) = Arc::clone(&rosters.reload).try_lock_owned() {
                tokio::spawn(reload_rosters(state.clone(), guard));
            }
        }
        None => {
            let guard = Arc::clone(&rosters.reload).lock_owned().await;
            if rosters.due_at().is_none() {
                reload_rosters(state.clone(), guard).await;
            }
        }
    }
}

/// Read every managed roster concurrently, publishing each as soon as its own
/// read finishes, so a slow registry does not hold back another's change.
async fn reload_rosters(state: AppState, _reload: OwnedMutexGuard<()>) {
    let rosters = &state.inner.identity.teams;
    let reloads =
        rosters.seeds.iter().map(|(registry, seed)| reload_roster(&state, registry, seed));
    let all_read = join_all(reloads).await.into_iter().all(|read| read);
    rosters.schedule(if all_read { ROSTER_TTL } else { ROSTER_RETRY });
}

/// Publish one registry's stored roster, or an empty one when it cannot be
/// read. Reports whether the read succeeded.
async fn reload_roster(state: &AppState, registry: &str, seed: &Teams) -> bool {
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
    match state.inner.storage.read_team_roster(registry).await? {
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
    let rosters = &state.inner.identity.teams;
    let seed = rosters.seeds.get(registry).ok_or(RegistryError::NotFound)?;
    let _reload = rosters.reload.lock().await;
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
