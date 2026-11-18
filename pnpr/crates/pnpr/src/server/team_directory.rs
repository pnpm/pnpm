//! This replica's copy of the instance's teams, which the database holds.

use super::{AppState, Identity, RegistryError, managed_state::read_with_deadline};
use indexmap::IndexMap;
use pnpr_auth::oidc::SessionUser;
use pnpr_storage::database::StoredTeams;
use std::{
    collections::{BTreeSet, HashMap},
    sync::{
        Arc, PoisonError, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Team {
    pub(super) name: String,
    /// Account IDs.
    pub(super) members: BTreeSet<String>,
}

/// The teams as of one stored generation.
#[derive(Debug, Default)]
pub(super) struct Teams {
    /// `None` before the first read.
    generation: Option<u64>,
    /// By team ID, sorted by name.
    by_id: IndexMap<String, Team>,
    /// The IDs of each account's teams.
    by_member: HashMap<String, Vec<String>>,
}

impl Teams {
    fn new(stored: StoredTeams) -> Self {
        let mut by_member: HashMap<String, Vec<String>> = HashMap::new();
        for team in &stored.teams {
            for member in &team.members {
                by_member
                    .entry(member.clone())
                    .or_default()
                    .push(team.id.clone());
            }
        }
        let by_id = stored.teams
            .into_iter()
            .map(|team| {
                (team.id, Team { name: team.name, members: team.members.into_iter().collect() })
            })
            .collect();
        Self { generation: Some(stored.generation), by_id, by_member }
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (&str, &Team)> {
        self.by_id
            .iter()
            .map(|(id, team)| (id.as_str(), team))
    }

    pub(super) fn contains(&self, id: &str) -> bool {
        self.by_id.contains_key(id)
    }

    pub(super) fn get(&self, id: &str) -> Option<(&str, &Team)> {
        self.by_id
            .get_key_value(id)
            .map(|(id, team)| (id.as_str(), team))
    }

    /// The ID and record of the team named `name`.
    pub(super) fn named(&self, name: &str) -> Option<(&str, &Team)> {
        self.iter()
            .find(|(_, team)| team.name == name)
    }

    fn of_member(&self, user_id: &str) -> &[String] {
        self.by_member
            .get(user_id)
            .map_or(&[], Vec::as_slice)
    }
}

/// The teams this replica last read. A failed periodic read keeps them.
#[derive(Debug, Default)]
pub(super) struct TeamState {
    teams: RwLock<Arc<Teams>>,
    /// Set while this replica could not read the teams after writing them, so
    /// its copy may still list a member the write removed. Cleared by the
    /// next read.
    behind_a_write: AtomicBool,
}

impl TeamState {
    pub(super) fn snapshot(&self) -> Arc<Teams> {
        Arc::clone(&self.teams.read().unwrap_or_else(PoisonError::into_inner))
    }

    fn publish(&self, teams: Teams) {
        *self.teams.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(teams);
    }

    /// Trust this replica's copy again, which the caller found or made
    /// current.
    fn caught_up(&self) -> bool {
        self.behind_a_write.store(false, Ordering::Release);
        true
    }

    /// The teams a request's identity is built from: none while this replica
    /// is behind its own write, so a removed member is not admitted.
    fn for_identities(&self) -> Arc<Teams> {
        if self.behind_a_write.load(Ordering::Acquire) {
            return Arc::default();
        }
        self.snapshot()
    }
}

fn teams(state: &AppState) -> Arc<Teams> {
    state.inner.identity.managed.teams.snapshot()
}

/// The identity of the account `user_id`, in the teams that list it.
pub(super) fn account_identity(state: &AppState, user_id: String, username: String) -> Identity {
    let directory = state.inner.identity.managed.teams.for_identities();
    let teams = directory.of_member(&user_id).to_vec();
    Identity::account_member(user_id, username, teams)
}

/// The identity of a browser session: its account, in the teams that list it
/// and the teams its groups claim earned.
pub(super) fn session_identity(state: &AppState, user: SessionUser, username: String) -> Identity {
    let directory = state.inner.identity.managed.teams.for_identities();
    let mut teams = directory.of_member(&user.user_id).to_vec();
    for grant in user.teams {
        if directory.contains(&grant.team_id) && !teams.contains(&grant.team_id) {
            teams.push(grant.team_id);
        }
    }
    Identity::account_member(user.user_id, username, teams)
}

/// Read the teams when their stored generation moved. Reports whether this
/// replica now holds the stored teams.
pub(super) async fn reload_teams(state: &AppState) -> bool {
    let directory = &state.inner.identity.managed.teams;
    let known = directory.snapshot().generation;
    let read = read_with_deadline(state.inner.storage.read_teams(known)).await;
    if let Ok(Some(stored)) = read {
        let teams = Teams::new(stored);
        warn_of_unknown_group_teams(state, &teams);
        directory.publish(teams);
        return directory.caught_up();
    }
    match read {
        Ok(_) => directory.caught_up(),
        Err(err) => {
            tracing::error!(error = %err, "could not read the teams; serving the last ones read");
            false
        }
    }
}

/// An OIDC group grant in the config file that names no team gives nobody
/// anything, which a typo would cause.
fn warn_of_unknown_group_teams(state: &AppState, teams: &Teams) {
    let grants = state.inner.config.identity.auth.oidc
        .iter()
        .filter_map(|provider| provider.login.as_ref()?.groups.as_ref())
        .flat_map(|groups| &groups.teams);
    for grant in grants.filter(|grant| !teams.contains(&grant.team_id)) {
        tracing::warn!(
            group = %grant.group,
            team_id = %grant.team_id,
            "an OIDC group grant names a team that does not exist",
        );
    }
}

/// Publish a team write this replica just made. Waits for a reload in
/// progress, which may have read the teams before the write.
pub(super) async fn reload_written_teams(state: &AppState) {
    let _reload = state.inner.identity.managed.reload.lock().await;
    follow_write(state).await;
}

/// [`reload_written_teams`] for a caller that holds the reload lock. When the
/// read fails, requests get no team until one succeeds.
pub(super) async fn follow_write(state: &AppState) {
    if reload_teams(state).await {
        return;
    }
    let managed = &state.inner.identity.managed;
    managed.teams.behind_a_write.store(true, Ordering::Release);
    managed.schedule_retry();
}

/// The ID of the team named `name` in the database now, for a write to that
/// team. This replica's copy may miss a team another replica just created, or
/// hold the ID of one it removed and created again.
pub(super) async fn team_id(state: &AppState, name: &str) -> Result<String, RegistryError> {
    let read = {
        let _reload = state.inner.identity.managed.reload.lock().await;
        reload_teams(state).await
    };
    if !read {
        return Err(RegistryError::StorageUnavailable {
            reason: "the teams could not be read".to_string(),
        });
    }
    teams(state)
        .named(name)
        .map(|(id, _)| id.to_string())
        .ok_or(RegistryError::NotFound)
}

/// Remove a deleted account from every team, for a caller that holds the
/// reload lock. A failure is logged and does not fail the deletion: the
/// account can no longer sign in, and an admin can remove its ID from a team.
pub(super) async fn remove_from_teams(state: &AppState, user_id: &str) {
    match state.inner.storage.remove_team_memberships(user_id).await {
        Ok(()) => follow_write(state).await,
        Err(err) => {
            tracing::error!(error = %err, user_id, "could not remove a deleted account from its teams");
        }
    }
}
