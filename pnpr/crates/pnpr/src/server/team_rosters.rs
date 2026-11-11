use super::{
    AppState, RegistryError,
    managed_state::{canonical_registry_key, read_with_deadline},
};
use pnpr_config::Teams;
use pnpr_storage::metadata::{StoredTeam, StoredTeamRoster, TeamRosterWrite};
use std::sync::PoisonError;

/// Publish one registry's stored roster, or an empty one when it cannot be
/// read. Reports whether the read succeeded.
pub(super) async fn reload_roster(state: &AppState, registry: &str, seed: &Teams) -> bool {
    let loaded = loaded_roster_generation(state, registry);
    match read_with_deadline(poll_roster(state, registry, seed, loaded)).await {
        Ok(None) => true,
        Ok(Some((generation, teams))) => {
            publish_roster(state, registry, Some(generation), teams);
            true
        }
        Err(err) => {
            tracing::error!(registry, error = %err, "could not read a team roster; its teams admit nobody");
            publish_roster(state, registry, None, Teams::default());
            false
        }
    }
}

async fn poll_roster(
    state: &AppState,
    registry: &str,
    seed: &Teams,
    loaded: Option<Option<u64>>,
) -> Result<Option<(Option<u64>, Teams)>, RegistryError> {
    let canonical = canonical_registry_key(state, registry);
    let current = state.inner.storage.read_team_roster_generation(&canonical).await?;
    if loaded == Some(current) {
        return Ok(None);
    }
    match state.inner.storage.read_team_roster(&canonical).await? {
        Some(roster) => Ok(Some((Some(roster.generation), teams_from_stored(roster.teams)))),
        None => Ok(Some((None, seed.clone()))),
    }
}

pub(super) async fn create_team_in_roster(
    state: &AppState,
    registry: &str,
    team_id: &str,
    name: &str,
) -> Result<(), RegistryError> {
    let managed = &state.inner.identity.managed;
    let seed = stored_seed(managed.roster_seeds.get(registry).ok_or(RegistryError::NotFound)?);
    let canonical = canonical_registry_key(state, registry);
    let _reload = managed.reload.lock().await;
    match state.inner.storage.create_team_in_roster(&canonical, &seed, team_id, name).await? {
        TeamRosterWrite::Written(roster) => {
            publish_stored_roster(state, registry, roster);
            Ok(())
        }
        TeamRosterWrite::Conflict => {
            Err(RegistryError::AdminConflict { reason: format!("team {name:?} already exists") })
        }
        TeamRosterWrite::NotFound => Err(RegistryError::NotFound),
    }
}

pub(super) async fn destroy_team_in_roster(
    state: &AppState,
    registry: &str,
    name: &str,
) -> Result<(), RegistryError> {
    let managed = &state.inner.identity.managed;
    let seed = stored_seed(managed.roster_seeds.get(registry).ok_or(RegistryError::NotFound)?);
    let canonical = canonical_registry_key(state, registry);
    let _reload = managed.reload.lock().await;
    let rules = state.inner.config.routing.hosted[registry].rules.clone();
    let team_label = name.to_string();
    let result = state
        .inner
        .storage
        .destroy_team_in_roster(&canonical, &seed, name, move |team_id| {
            if rules.references_team(team_id) {
                return Err(RegistryError::AdminConflict {
                    reason: format!(
                        "team {team_label:?} is named by this registry's `packages:` rules; remove it from the configuration first",
                    ),
                });
            }
            Ok(())
        })
        .await?;
    finish_member_or_destroy(state, registry, result)
}

pub(super) async fn add_member_to_team(
    state: &AppState,
    registry: &str,
    name: &str,
    user_id: &str,
) -> Result<(), RegistryError> {
    let managed = &state.inner.identity.managed;
    let seed = stored_seed(managed.roster_seeds.get(registry).ok_or(RegistryError::NotFound)?);
    let canonical = canonical_registry_key(state, registry);
    let _reload = managed.reload.lock().await;
    let result = state.inner.storage.add_member_to_team(&canonical, &seed, name, user_id).await?;
    finish_member_or_destroy(state, registry, result)
}

pub(super) async fn remove_member_from_team(
    state: &AppState,
    registry: &str,
    name: &str,
    user_id: &str,
) -> Result<(), RegistryError> {
    let managed = &state.inner.identity.managed;
    let seed = stored_seed(managed.roster_seeds.get(registry).ok_or(RegistryError::NotFound)?);
    let canonical = canonical_registry_key(state, registry);
    let _reload = managed.reload.lock().await;
    let result =
        state.inner.storage.remove_member_from_team(&canonical, &seed, name, user_id).await?;
    finish_member_or_destroy(state, registry, result)
}

fn finish_member_or_destroy(
    state: &AppState,
    registry: &str,
    result: TeamRosterWrite,
) -> Result<(), RegistryError> {
    match result {
        TeamRosterWrite::Written(roster) => {
            publish_stored_roster(state, registry, roster);
            Ok(())
        }
        TeamRosterWrite::NotFound => Err(RegistryError::NotFound),
        TeamRosterWrite::Conflict => Err(RegistryError::AdminConflict {
            reason: format!("the teams of registry {registry:?} changed concurrently"),
        }),
    }
}

fn publish_stored_roster(state: &AppState, registry: &str, roster: StoredTeamRoster) {
    publish_roster(state, registry, Some(Some(roster.generation)), teams_from_stored(roster.teams));
}

fn loaded_roster_generation(state: &AppState, registry: &str) -> Option<Option<u64>> {
    state.inner.identity.managed.roster_generations
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(registry)
        .copied()
}

fn publish_roster(state: &AppState, registry: &str, generation: Option<Option<u64>>, teams: Teams) {
    if let Some(hosted) = state.inner.config.routing.hosted.get(registry) {
        hosted.teams.replace(teams);
    }
    let mut loaded = state.inner.identity.managed.roster_generations
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    match generation {
        Some(generation) => {
            loaded.insert(registry.to_string(), generation);
        }
        None => {
            loaded.remove(registry);
        }
    }
}

fn stored_seed(teams: &Teams) -> Vec<StoredTeam> {
    teams
        .iter()
        .map(|(id, team)| StoredTeam {
            id: id.clone(),
            name: team.name.clone(),
            members: team.members.clone(),
        })
        .collect()
}

fn teams_from_stored(teams: Vec<StoredTeam>) -> Teams {
    teams
        .into_iter()
        .map(|team| (team.id, pnpr_policy::Team::new(team.name, team.members)))
        .collect()
}
