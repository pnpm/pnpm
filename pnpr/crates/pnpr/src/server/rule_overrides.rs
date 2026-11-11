use super::{
    AppState, RegistryError,
    managed_state::{WRITE_ATTEMPTS, canonical_registry_key, read_with_deadline},
};
use pnpr_config::{TeamDirectory, compile_access_list};
use pnpr_policy::{AccessList, AccessToken, RuleOverride, RuleTable};
use pnpr_registry::{Ecosystem, PackagePattern};
use pnpr_storage::metadata::{RuleOverridesWrite, StoredPackageOverride, StoredRuleOverrides};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Arc, PoisonError},
};

/// What an admin changed in a registry's rules, as the admin API takes it.
/// Each list replaces the YAML's list of the same place; an omitted list keeps
/// it.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub(super) struct RuleChanges {
    /// The registry-level `access:`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) access: Option<Vec<String>>,
    /// Keyed by a pattern the registry's `packages:` map declares.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) packages: BTreeMap<String, PackageChanges>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub(super) struct PackageChanges {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) access: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) publish: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) unpublish: Option<Vec<String>>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) extra: BTreeMap<String, serde_json::Value>,
}

/// How a list's tokens become an [`AccessList`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum Compile {
    /// As an admin writes them: every token well formed, every `team:`
    /// reference naming a team the roster holds.
    Strict,
    /// As they are read back from the store: a `team:` reference to a team the
    /// roster no longer holds admits nobody, as a removed team does.
    Stored,
}

/// Publish one registry's stored rule changes over its YAML rules. Changes
/// that cannot be read or applied fail closed: every list of the registry
/// admits nobody until a read succeeds. Reports whether the read succeeded.
pub(super) async fn reload_rules(state: &AppState, registry: &str, base: &RuleTable) -> bool {
    let loaded = loaded_rule_generation(state, registry);
    match read_with_deadline(poll_rules(state, registry, base, loaded)).await {
        Ok(None) => true,
        Ok(Some((generation, table))) => {
            publish_rules(state, registry, Some(generation), table);
            true
        }
        Err(err) => {
            tracing::error!(registry, error = %err, "could not apply a registry's stored rules; it admits nobody");
            publish_rules(state, registry, None, deny_all(base));
            false
        }
    }
}

async fn poll_rules(
    state: &AppState,
    registry: &str,
    base: &RuleTable,
    loaded: Option<Option<u64>>,
) -> Result<Option<(Option<u64>, RuleTable)>, RegistryError> {
    let canonical = canonical_registry_key(state, registry);
    let current = state.inner.storage.read_rule_overrides_generation(&canonical).await?;
    if loaded == Some(current) {
        return Ok(None);
    }
    let (table, generation) = read_rules(state, registry, base).await?;
    Ok(Some((generation, table)))
}

/// `registry`'s rules and their generation, both from one read of the stored
/// overrides, so the version always names the rules it comes with.
async fn read_rules(
    state: &AppState,
    registry: &str,
    base: &RuleTable,
) -> Result<(RuleTable, Option<u64>), RegistryError> {
    let canonical = canonical_registry_key(state, registry);
    let Some(stored) = state.inner.storage.read_rule_overrides(&canonical).await? else {
        return Ok((base.clone(), None));
    };
    let changes = changes_from_stored(stored.overrides);
    let table = apply_changes(state, registry, base, changes, Compile::Stored)
        .map_err(|reason| RegistryError::Internal { reason })?;
    Ok((table, Some(stored.generation)))
}

/// The version of a registry's stored rule changes, for `ETag` and
/// `If-Match`. Absent or empty overrides have the version `config`.
pub(super) fn rules_version(generation: Option<u64>) -> String {
    generation.map(|generation| generation.to_string()).unwrap_or_else(|| "config".to_string())
}

/// `registry`'s rules and their version. See [`read_rules`].
pub(super) async fn current_rules(
    state: &AppState,
    registry: &str,
) -> Result<(Arc<RuleTable>, String), RegistryError> {
    let Some(base) = state.inner.identity.managed.rule_bases.get(registry) else {
        let hosted = &state.inner.config.routing.hosted[registry];
        return Ok((hosted.rules.snapshot(), rules_version(None)));
    };
    let (table, generation) = read_rules(state, registry, base).await?;
    Ok((Arc::new(table), rules_version(generation)))
}

/// Store `changes` as the rules of `registry`, replacing what was stored, and
/// apply them on this replica at once. Changes are checked against the
/// registry's YAML rules and current roster before anything is written.
///
/// With `expected`, the write lands only while the stored changes still have
/// one of those versions; otherwise the last writer wins.
pub(super) async fn replace_rules(
    state: &AppState,
    registry: &str,
    changes: RuleChanges,
    expected: Option<&[String]>,
) -> Result<(RuleTable, String), RegistryError> {
    let managed = &state.inner.identity.managed;
    let base = managed.rule_bases.get(registry).ok_or(RegistryError::NotFound)?;
    let canonical = canonical_registry_key(state, registry);
    let stored = stored_from_changes(&changes);
    let table = apply_changes(state, registry, base, changes, Compile::Strict)
        .map_err(|reason| RegistryError::BadRequest { reason })?;
    let _reload = managed.reload.lock().await;
    for _ in 0..WRITE_ATTEMPTS {
        let expected_gen = match expected {
            Some(versions) => {
                let current = state.inner.storage.read_rule_overrides_generation(&canonical).await?;
                check_version(registry, current, versions)?;
                Some(current)
            }
            None => None,
        };
        if let RuleOverridesWrite::Written(generation) =
            state.inner.storage.replace_rule_overrides(&canonical, expected_gen, &stored).await?
        {
            publish_rules(state, registry, Some(generation), table.clone());
            return Ok((table, rules_version(generation)));
        }
    }
    Err(RegistryError::AdminConflict {
        reason: format!("the rules of registry {registry:?} kept changing; retry the request"),
    })
}

/// Apply `edit` to the stored rule changes of `registry` and store the
/// result, rereading and reapplying it when another writer replaced them
/// first.
pub(super) async fn update_rules<Edit>(
    state: &AppState,
    registry: &str,
    edit: Edit,
) -> Result<(), RegistryError>
where
    Edit: Fn(&mut RuleChanges, &RuleTable) -> Result<(), RegistryError>,
{
    let managed = &state.inner.identity.managed;
    let base = managed.rule_bases.get(registry).ok_or(RegistryError::NotFound)?;
    let canonical = canonical_registry_key(state, registry);
    let _reload = managed.reload.lock().await;
    for _ in 0..WRITE_ATTEMPTS {
        let stored_set = state.inner.storage.read_rule_overrides(&canonical).await?;
        let current_gen = stored_set.as_ref().map(|set| set.generation);
        let mut changes =
            stored_set.map(|set| changes_from_stored(set.overrides)).unwrap_or_default();
        strip_unsupported_unpublish(state, registry, &mut changes);
        edit(&mut changes, base)?;
        let stored = stored_from_changes(&changes);
        let table = apply_changes(state, registry, base, changes, Compile::Strict)
            .map_err(|reason| RegistryError::BadRequest { reason })?;
        if let RuleOverridesWrite::Written(generation) =
            state.inner.storage.replace_rule_overrides(&canonical, Some(current_gen), &stored)
                .await?
        {
            publish_rules(state, registry, Some(generation), table);
            return Ok(());
        }
    }
    Err(RegistryError::AdminConflict {
        reason: format!("the rules of registry {registry:?} kept changing; retry the request"),
    })
}

fn strip_unsupported_unpublish(state: &AppState, registry: &str, changes: &mut RuleChanges) {
    let ecosystem = state.inner.config.routing.registries.ecosystem(registry).unwrap_or_default();
    if ecosystem == Ecosystem::Cargo || ecosystem == Ecosystem::Pypi {
        for pkg in changes.packages.values_mut() {
            pkg.unpublish = None;
        }
    }
}

fn check_version(
    registry: &str,
    current: Option<u64>,
    expected: &[String],
) -> Result<(), RegistryError> {
    if expected.contains(&rules_version(current)) {
        return Ok(());
    }
    Err(RegistryError::PreconditionFailed {
        resource: format!("the rules of registry {registry:?}"),
        expected: expected.join(", "),
    })
}

fn loaded_rule_generation(state: &AppState, registry: &str) -> Option<Option<u64>> {
    state.inner.identity.managed.rule_generations
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(registry)
        .copied()
}

fn publish_rules(
    state: &AppState,
    registry: &str,
    generation: Option<Option<u64>>,
    table: RuleTable,
) {
    if let Some(hosted) = state.inner.config.routing.hosted.get(registry) {
        hosted.rules.replace(table);
    }
    let mut loaded = state.inner.identity.managed.rule_generations
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

fn stored_from_changes(changes: &RuleChanges) -> StoredRuleOverrides {
    StoredRuleOverrides {
        access: changes.access.clone(),
        packages: changes.packages
            .iter()
            .map(|(pattern, pkg)| {
                (
                    pattern.clone(),
                    StoredPackageOverride {
                        access: pkg.access.clone(),
                        publish: pkg.publish.clone(),
                        unpublish: pkg.unpublish.clone(),
                    },
                )
            })
            .collect(),
    }
}

fn changes_from_stored(stored: StoredRuleOverrides) -> RuleChanges {
    RuleChanges {
        access: stored.access,
        packages: stored.packages
            .into_iter()
            .map(|(pattern, pkg)| {
                (
                    pattern,
                    PackageChanges {
                        access: pkg.access,
                        publish: pkg.publish,
                        unpublish: pkg.unpublish,
                        extra: BTreeMap::new(),
                    },
                )
            })
            .collect(),
        extra: BTreeMap::new(),
    }
}

fn validate_and_compile_override(
    registry: &str,
    ecosystem: Ecosystem,
    compile: Compile,
    key: &str,
    mut lists: PackageChanges,
    teams: &TeamDirectory,
) -> Result<(PackagePattern, RuleOverride), String> {
    if ecosystem == Ecosystem::Cargo || ecosystem == Ecosystem::Pypi {
        if compile == Compile::Strict && lists.unpublish.is_some() {
            return Err(format!(
                "registry {registry:?} serves {ecosystem}, which does not support unpublishing; `unpublish` rule on {key:?} is invalid",
            ));
        }
        if compile == Compile::Stored {
            lists.unpublish = None;
        }
    }
    let pattern = PackagePattern::parse(key, ecosystem)
        .map_err(|err| format!("{key:?} is not a package pattern: {err}"))?;
    let list = |entries: Option<Vec<String>>| {
        entries.map(|entries| compile_list(entries, teams, compile)).transpose()
    };
    let override_rules = RuleOverride {
        access: list(lists.access)?,
        publish: list(lists.publish)?,
        unpublish: list(lists.unpublish)?,
    };
    Ok((pattern, override_rules))
}

fn apply_changes(
    state: &AppState,
    registry: &str,
    base: &RuleTable,
    changes: RuleChanges,
    compile: Compile,
) -> Result<RuleTable, String> {
    let hosted = &state.inner.config.routing.hosted[registry];
    let ecosystem = state.inner.config.routing.registries.ecosystem(registry).unwrap_or_default();
    let default_access =
        changes.access.map(|entries| compile_list(entries, &hosted.teams, compile)).transpose()?;
    let mut overrides = Vec::with_capacity(changes.packages.len());
    for (key, lists) in changes.packages {
        let entry = validate_and_compile_override(
            registry,
            ecosystem,
            compile,
            &key,
            lists,
            &hosted.teams,
        )?;
        overrides.push(entry);
    }
    base.with_overrides(default_access, overrides)
        .map_err(|pattern| format!("registry {registry:?} does not declare the pattern {pattern}"))
}

fn compile_list(
    entries: Vec<String>,
    teams: &TeamDirectory,
    compile: Compile,
) -> Result<AccessList, String> {
    match compile {
        Compile::Strict => compile_access_list(entries, teams),
        Compile::Stored => Ok(AccessList::new(
            entries
                .iter()
                .map(|entry| match entry.strip_prefix("team:") {
                    Some(team) => {
                        AccessToken::Team { id: team.to_string(), directory: teams.clone() }
                    }
                    None => AccessToken::from(entry.as_str()),
                })
                .collect(),
        )),
    }
}

/// `base` with every list, the registry-level default included, admitting
/// nobody.
fn deny_all(base: &RuleTable) -> RuleTable {
    let nobody = || Some(AccessList::default());
    let overrides = base
        .rules()
        .iter()
        .map(|rule| {
            let lists = RuleOverride { access: nobody(), publish: nobody(), unpublish: nobody() };
            (rule.pattern.clone(), lists)
        })
        .collect();
    base.with_overrides(nobody(), overrides)
        .expect("the patterns come from the table itself")
        .with_default_publish(AccessList::default())
        .with_default_unpublish(AccessList::default())
}

/// The registry's current rules in the shape the admin API returns.
pub(super) fn rules_view(table: &RuleTable) -> serde_json::Value {
    let lists = |list: Option<&AccessList>| list.map(AccessList::token_strings);
    let packages: serde_json::Map<String, serde_json::Value> = table
        .rules()
        .iter()
        .map(|rule| {
            let value = serde_json::json!({
                "access": lists(rule.access.as_ref()),
                "publish": lists(rule.publish.as_ref()),
                "unpublish": lists(rule.unpublish.as_ref()),
            });
            (rule.pattern.to_string(), value)
        })
        .collect();
    serde_json::json!({ "access": table.default_access().token_strings(), "packages": packages })
}
