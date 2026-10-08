use super::{
    AppState, RegistryError,
    ecosystem::sha256_hex,
    managed_state::{WRITE_ATTEMPTS, store_registry_record},
};
use pnpr_config::{TeamDirectory, compile_access_list};
use pnpr_policy::{AccessList, AccessToken, RuleOverride, RuleTable};
use pnpr_registry::PackagePattern;
use pnpr_storage::RegistryRecord;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

/// What an admin changed in a registry's rules, as stored and as the admin
/// API takes it. Each list replaces the YAML's list of the same place; an
/// omitted list keeps it.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RuleChanges {
    /// The registry-level `access:`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) access: Option<Vec<String>>,
    /// Keyed by a pattern the registry's `packages:` map declares.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) packages: BTreeMap<String, PackageChanges>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PackageChanges {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) access: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) publish: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) unpublish: Option<Vec<String>>,
}

/// How a list's tokens become an [`AccessList`].
#[derive(Clone, Copy)]
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
    let (table, read) = match read_rules(state, registry, base).await {
        Ok((table, _)) => (table, true),
        Err(err) => {
            tracing::error!(registry, error = %err, "could not apply a registry's stored rules; it admits nobody");
            (deny_all(base), false)
        }
    };
    publish_rules(state, registry, table);
    read
}

/// `registry`'s rules and their version, both from one read of the stored
/// changes, so the version always names the rules it comes with.
async fn read_rules(
    state: &AppState,
    registry: &str,
    base: &RuleTable,
) -> Result<(RuleTable, String), RegistryError> {
    let Some(bytes) =
        state.inner.storage.read_registry_record(RegistryRecord::RuleOverrides, registry).await?
    else {
        return Ok((base.clone(), rules_version(None)));
    };
    let changes: RuleChanges = serde_json::from_slice(&bytes)
        .map_err(|err| RegistryError::Internal {
            reason: format!("the stored rules of registry {registry:?} are unreadable: {err}"),
        })?;
    let table = apply_changes(state, registry, base, changes, Compile::Stored)
        .map_err(|reason| RegistryError::Internal { reason })?;
    Ok((table, rules_version(Some(&bytes))))
}

/// The version of a registry's stored rule changes, for `ETag` and
/// `If-Match`. A registry with no stored changes has the version `config`.
pub(super) fn rules_version(stored: Option<&[u8]>) -> String {
    stored.map_or_else(|| "config".to_string(), sha256_hex)
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
    let (table, version) = read_rules(state, registry, base).await?;
    Ok((Arc::new(table), version))
}

/// Store `changes` as the rules of `registry`, replacing what was stored, and
/// apply them on this replica at once. Changes are checked against the
/// registry's YAML rules and current roster before anything is written.
///
/// With `expected`, the write lands only while the stored changes still have
/// one of those versions; otherwise the last writer wins. Resetting the rules
/// stores the empty set of changes, so it is conditional the same way.
/// Returns the rules stored and their version.
pub(super) async fn replace_rules(
    state: &AppState,
    registry: &str,
    changes: RuleChanges,
    expected: Option<&[String]>,
) -> Result<(RuleTable, String), RegistryError> {
    let managed = &state.inner.identity.managed;
    let base = managed.rule_bases.get(registry).ok_or(RegistryError::NotFound)?;
    let bytes = serde_json::to_vec(&changes)?;
    let table = apply_changes(state, registry, base, changes, Compile::Strict)
        .map_err(|reason| RegistryError::BadRequest { reason })?;
    let _reload = managed.reload.lock().await;
    for _ in 0..WRITE_ATTEMPTS {
        let kind = RegistryRecord::RuleOverrides;
        let stored = state.inner.storage.read_registry_record(kind, registry).await?;
        check_version(registry, stored.as_deref(), expected)?;
        if store_registry_record(state, kind, registry, stored.as_deref(), &bytes).await? {
            publish_rules(state, registry, table.clone());
            return Ok((table, rules_version(Some(&bytes))));
        }
    }
    Err(RegistryError::AdminConflict {
        reason: format!("the rules of registry {registry:?} kept changing; retry the request"),
    })
}

fn check_version(
    registry: &str,
    stored: Option<&[u8]>,
    expected: Option<&[String]>,
) -> Result<(), RegistryError> {
    match expected {
        Some(expected) if !expected.contains(&rules_version(stored)) => {
            Err(RegistryError::PreconditionFailed {
                resource: format!("the rules of registry {registry:?}"),
                expected: expected.join(", "),
            })
        }
        _ => Ok(()),
    }
}

fn publish_rules(state: &AppState, registry: &str, table: RuleTable) {
    if let Some(hosted) = state.inner.config.routing.hosted.get(registry) {
        hosted.rules.replace(table);
    }
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
    let list = |entries: Option<Vec<String>>| {
        entries.map(|entries| compile_list(entries, &hosted.teams, compile)).transpose()
    };
    let default_access = list(changes.access)?;
    let mut overrides = Vec::with_capacity(changes.packages.len());
    for (key, lists) in changes.packages {
        let pattern = PackagePattern::parse(&key, ecosystem)
            .map_err(|err| format!("{key:?} is not a package pattern: {err}"))?;
        let lists = RuleOverride {
            access: list(lists.access)?,
            publish: list(lists.publish)?,
            unpublish: list(lists.unpublish)?,
        };
        overrides.push((pattern, lists));
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
                        AccessToken::Team { name: team.to_string(), directory: teams.clone() }
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
