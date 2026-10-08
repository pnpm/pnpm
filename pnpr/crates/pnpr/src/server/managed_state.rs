use super::{
    AppState, Config, IndexMap, RegistryError,
    rule_overrides::reload_rules,
    scim::{ScimState, reload_directory},
    team_rosters::reload_roster,
};
use futures_util::future::join_all;
use pnpr_config::{Management, Teams};
use pnpr_policy::RuleTable;
use pnpr_storage::{DocumentWrite, RegistryRecord};
use std::{
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use tokio::sync::OwnedMutexGuard;

/// How long a replica serves the managed state it last read before it reads
/// it from the hosted store again. Bounds how long a change made on another
/// replica takes to reach this one.
const STATE_TTL: Duration = Duration::from_secs(10);

/// How soon a replica tries again after a record it could not read.
const STATE_RETRY: Duration = Duration::from_secs(1);

/// How many times an edit rereads a stored record after another writer
/// replaced it first.
pub(super) const WRITE_ATTEMPTS: usize = 5;

/// The state admins and identity providers manage at runtime: team rosters
/// (`teamsManagedBy: api`), package rules (`rulesManagedBy: api`), and the
/// SCIM directory (`auth.scim`).
///
/// The hosted store holds each one as a [`RegistryRecord`], so every replica
/// sees the same state. A registry with no stored record yet serves its YAML.
pub(super) struct ManagedState {
    /// Each roster-managed registry's `teams:` map, keyed by registry name.
    pub(super) roster_seeds: IndexMap<String, Teams>,
    /// Each rules-managed registry's rules as the YAML declares them, keyed
    /// by registry name. Stored changes apply on top of these.
    pub(super) rule_bases: IndexMap<String, Arc<RuleTable>>,
    /// The usernames the SCIM directory marks inactive. `None` without
    /// `auth.scim`.
    pub(super) scim: Option<ScimState>,
    /// When the state is next due for a reload. `None` before the first.
    due_at: Mutex<Option<Instant>>,
    /// Held by a reload and by every edit, so a reload that read the store
    /// before an edit cannot publish its older copy after it.
    pub(super) reload: Arc<tokio::sync::Mutex<()>>,
}

impl ManagedState {
    pub(super) fn new(config: &Config) -> Self {
        let managed = |by: fn(&pnpr_config::HostedConfig) -> Management| {
            config.routing.hosted
                .iter()
                .filter(move |(_, hosted)| by(hosted) == Management::Api)
        };
        let roster_seeds = managed(|hosted| hosted.teams_managed_by)
            .map(|(name, hosted)| (name.clone(), Teams::clone(&hosted.teams.snapshot())))
            .collect();
        let rule_bases = managed(|hosted| hosted.rules_managed_by)
            .map(|(name, hosted)| (name.clone(), hosted.rules.snapshot()))
            .collect();
        let scim = config.identity.auth.scim.as_ref().map(|_| ScimState::default());
        Self { roster_seeds, rule_bases, scim, due_at: Mutex::new(None), reload: Arc::default() }
    }

    fn is_empty(&self) -> bool {
        self.roster_seeds.is_empty() && self.rule_bases.is_empty() && self.scim.is_none()
    }

    fn due_at(&self) -> Option<Instant> {
        *self.due_at.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn schedule(&self, after: Duration) {
        *self.due_at.lock().unwrap_or_else(PoisonError::into_inner) = Some(Instant::now() + after);
    }
}

/// Reload the managed state once it is due. The first load after startup
/// holds up the request, so no request is authorized against state this
/// replica has not read. Later reloads run in the background while requests
/// keep the current state.
pub(super) async fn refresh_managed_state(state: &AppState) {
    let managed = &state.inner.identity.managed;
    if managed.is_empty() {
        return;
    }
    match managed.due_at() {
        Some(due_at) if Instant::now() < due_at => {}
        Some(_) => {
            if let Ok(guard) = Arc::clone(&managed.reload).try_lock_owned() {
                tokio::spawn(reload_managed_state(state.clone(), guard));
            }
        }
        None => {
            let guard = Arc::clone(&managed.reload).lock_owned().await;
            if managed.due_at().is_none() {
                reload_managed_state(state.clone(), guard).await;
            }
        }
    }
}

/// Read every managed record concurrently, publishing each as soon as its own
/// read finishes, so a slow registry does not hold back another's change.
/// Rosters load before rules, because a rule can name a team.
async fn reload_managed_state(state: AppState, _reload: OwnedMutexGuard<()>) {
    let managed = &state.inner.identity.managed;
    let rosters =
        managed.roster_seeds.iter().map(|(registry, seed)| reload_roster(&state, registry, seed));
    let rosters_read = join_all(rosters).await.into_iter().all(|read| read);
    let rules =
        managed.rule_bases.iter().map(|(registry, base)| reload_rules(&state, registry, base));
    let rules_read = join_all(rules).await.into_iter().all(|read| read);
    let scim_read = match &managed.scim {
        Some(scim) => reload_directory(&state, scim).await,
        None => true,
    };
    let all_read = rosters_read && rules_read && scim_read;
    managed.schedule(if all_read { STATE_TTL } else { STATE_RETRY });
}

/// Store `bytes` as the record of `kind` for `registry`: create it when
/// `expected` is `None`, otherwise replace it only while it still holds
/// `expected`. Reports whether the write landed.
pub(super) async fn store_registry_record(
    state: &AppState,
    kind: RegistryRecord,
    registry: &str,
    expected: Option<&[u8]>,
    bytes: &[u8],
) -> Result<bool, RegistryError> {
    let storage = &state.inner.storage;
    match expected {
        None => storage.create_registry_record(kind, registry, bytes).await,
        Some(expected) => {
            let written =
                storage.replace_registry_record_if_current(kind, registry, expected, bytes).await?;
            Ok(written == DocumentWrite::Written)
        }
    }
}
