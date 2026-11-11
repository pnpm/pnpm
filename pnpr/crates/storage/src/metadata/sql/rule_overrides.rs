//! Normalized package rule overrides (`rule_override_sets`, `rule_overrides`,
//! `rule_override_grants`).

use super::{
    adapter::{Connection, Row},
    batch::WriteTransaction,
    cdn,
    records::advance_record_generation,
    team_rosters::validated_registry_key,
    to_column, to_generation,
    transactions::Freshness,
};
use crate::Storage;
use pnpr_error::{RegistryError, Result};
use std::collections::BTreeMap;

const REGISTRY_PATTERN: &str = "";
const ACCESS_ACTION: &str = "access";
const PUBLISH_ACTION: &str = "publish";
const UNPUBLISH_ACTION: &str = "unpublish";

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StoredPackageOverride {
    pub access: Option<Vec<String>>,
    pub publish: Option<Vec<String>>,
    pub unpublish: Option<Vec<String>>,
}

impl StoredPackageOverride {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.access.is_none() && self.publish.is_none() && self.unpublish.is_none()
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StoredRuleOverrides {
    pub access: Option<Vec<String>>,
    pub packages: BTreeMap<String, StoredPackageOverride>,
}

impl StoredRuleOverrides {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.access.is_none() && self.packages.values().all(StoredPackageOverride::is_empty)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRuleOverrideSet {
    pub generation: u64,
    pub overrides: StoredRuleOverrides,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleOverridesWrite {
    Written(Option<u64>),
    Conflict,
}

impl Storage {
    pub async fn read_rule_overrides_generation(&self, registry: &str) -> Result<Option<u64>> {
        let registry = validated_registry_key(registry)?;
        self.database.read(String::new(), Freshness::Current, move |connection, _| {
            override_set_generation(connection, &registry)
        })
        .await
    }

    pub async fn read_rule_overrides(
        &self,
        registry: &str,
    ) -> Result<Option<StoredRuleOverrideSet>> {
        let registry = validated_registry_key(registry)?;
        self.database.read(String::new(), Freshness::Current, move |connection, _| {
            read_override_set(connection, &registry)
        })
        .await
    }

    /// Replace the rule overrides of `registry`. When `expected` is `Some`,
    /// the write succeeds only while the current stored generation equals it.
    pub async fn replace_rule_overrides(
        &self,
        registry: &str,
        expected: Option<Option<u64>>,
        overrides: &StoredRuleOverrides,
    ) -> Result<RuleOverridesWrite> {
        let registry = validated_registry_key(registry)?;
        let overrides = overrides.clone();
        self.database.write(move |connection| {
            write_override_set(connection, &registry, expected, &overrides)
        })
        .await
    }
}

fn override_set_generation(connection: &dyn Connection, registry: &str) -> Result<Option<u64>> {
    connection
        .query_optional(
            "SELECT generation FROM rule_override_sets WHERE registry = ?1",
            params![registry],
        )?
        .map(|mut row| decode_override_generation(registry, row.get(0)?))
        .transpose()
}

enum UnknownActions {
    Absent,
    Present,
}

fn read_override_set(
    connection: &dyn Connection,
    registry: &str,
) -> Result<Option<StoredRuleOverrideSet>> {
    Ok(read_override_set_detail(connection, registry)?.map(|(set, _)| set))
}

fn read_override_set_detail(
    connection: &dyn Connection,
    registry: &str,
) -> Result<Option<(StoredRuleOverrideSet, UnknownActions)>> {
    let queries = [
        query!("SELECT generation FROM rule_override_sets WHERE registry = ?1", registry),
        query!(
            "SELECT pattern, action FROM rule_overrides
             WHERE registry = ?1 ORDER BY pattern, action",
            registry,
        ),
        query!(
            "SELECT pattern, action, position, principal FROM rule_override_grants
             WHERE registry = ?1 ORDER BY pattern, action, position",
            registry,
        ),
    ];
    let mut results = connection.query_all(&queries)?.into_iter();
    let head = results.next().unwrap_or_default();
    let override_rows = results.next().unwrap_or_default();
    let grant_rows = results.next().unwrap_or_default();
    let Some(mut head_row) = head.into_iter().next() else {
        if !override_rows.is_empty() || !grant_rows.is_empty() {
            return Err(corrupt(registry, "orphaned rule override rows without header"));
        }
        return Ok(None);
    };
    let generation = decode_override_generation(registry, head_row.get(0)?)?;
    let (overrides, unknown) = assemble_overrides(registry, override_rows, grant_rows)?;
    Ok(Some((StoredRuleOverrideSet { generation, overrides }, unknown)))
}

fn assemble_overrides(
    registry: &str,
    override_rows: Vec<Row>,
    grant_rows: Vec<Row>,
) -> Result<(StoredRuleOverrides, UnknownActions)> {
    let mut lists: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for mut row in override_rows {
        let key = (row.get::<String>(0)?, row.get::<String>(1)?);
        if key.1.is_empty() {
            return Err(corrupt(registry, "empty rule override action"));
        }
        lists.insert(key, Vec::new());
    }
    collect_grants(registry, &mut lists, grant_rows)?;
    Ok(build_stored_overrides(lists))
}

fn collect_grants(
    registry: &str,
    lists: &mut BTreeMap<(String, String), Vec<String>>,
    grant_rows: Vec<Row>,
) -> Result<()> {
    for mut row in grant_rows {
        let key = (row.get::<String>(0)?, row.get::<String>(1)?);
        let position: i64 = row.get(2)?;
        let principal: String = row.get(3)?;
        let Some(entries) = lists.get_mut(&key) else {
            return Err(corrupt(registry, "grant without rule override row"));
        };
        if position != to_column(entries.len() as u64) {
            return Err(corrupt(registry, "non-contiguous grant position"));
        }
        entries.push(principal);
    }
    Ok(())
}

fn build_stored_overrides(
    lists: BTreeMap<(String, String), Vec<String>>,
) -> (StoredRuleOverrides, UnknownActions) {
    let mut stored = StoredRuleOverrides::default();
    let mut unknown = UnknownActions::Absent;
    for ((pattern, action), principals) in lists {
        if pattern == REGISTRY_PATTERN {
            if action == ACCESS_ACTION {
                stored.access = Some(principals);
            } else {
                unknown = UnknownActions::Present;
            }
            continue;
        }
        match action.as_str() {
            ACCESS_ACTION => {
                stored.packages.entry(pattern).or_default().access = Some(principals);
            }
            PUBLISH_ACTION => {
                stored.packages.entry(pattern).or_default().publish = Some(principals);
            }
            UNPUBLISH_ACTION => {
                stored.packages.entry(pattern).or_default().unpublish = Some(principals);
            }
            // Unknown future actions written by a newer replica are ignored on read.
            _ => unknown = UnknownActions::Present,
        }
    }
    (stored, unknown)
}

fn write_override_set(
    connection: &dyn Connection,
    registry: &str,
    expected: Option<Option<u64>>,
    overrides: &StoredRuleOverrides,
) -> Result<RuleOverridesWrite> {
    if overrides.packages.keys().any(String::is_empty) {
        return Err(RegistryError::BadRequest {
            reason: "rule override package pattern must not be empty".to_string(),
        });
    }
    let transaction = WriteTransaction::begin(connection)?;
    let existing = read_override_set_detail(&*transaction, registry)?;
    let current = existing.as_ref().map(|(set, _)| set.generation);
    if expected.is_some_and(|expected_gen| current != expected_gen) {
        return Ok(RuleOverridesWrite::Conflict);
    }
    if overrides.is_empty() {
        clear_override_set(transaction, registry, current.is_some())?;
        return Ok(RuleOverridesWrite::Written(None));
    }
    if let Some((set, UnknownActions::Absent)) = &existing
        && set.overrides == *overrides
    {
        return Ok(RuleOverridesWrite::Written(Some(set.generation)));
    }
    transaction.execute("DELETE FROM rule_override_grants WHERE registry = ?1", params![registry])?;
    transaction.execute("DELETE FROM rule_overrides WHERE registry = ?1", params![registry])?;
    let generation = advance_record_generation(&*transaction)?;
    transaction.execute(
        "INSERT INTO rule_override_sets (registry, generation) VALUES (?1, ?2)
         ON CONFLICT (registry) DO UPDATE SET generation = excluded.generation",
        params![registry, generation],
    )?;
    insert_override_lists(&*transaction, registry, overrides)?;
    cdn::enqueue(&*transaction, "*")?;
    transaction.commit()?;
    Ok(RuleOverridesWrite::Written(Some(to_generation(generation))))
}

fn clear_override_set(
    transaction: WriteTransaction<'_>,
    registry: &str,
    existed: bool,
) -> Result<()> {
    if !existed {
        return Ok(());
    }
    transaction.execute("DELETE FROM rule_override_grants WHERE registry = ?1", params![registry])?;
    transaction.execute("DELETE FROM rule_overrides WHERE registry = ?1", params![registry])?;
    transaction.execute("DELETE FROM rule_override_sets WHERE registry = ?1", params![registry])?;
    cdn::enqueue(&*transaction, "*")?;
    transaction.commit()
}

fn insert_override_lists(
    connection: &dyn Connection,
    registry: &str,
    overrides: &StoredRuleOverrides,
) -> Result<()> {
    if let Some(access) = &overrides.access {
        insert_list(connection, registry, REGISTRY_PATTERN, ACCESS_ACTION, access)?;
    }
    for (pattern, pkg) in &overrides.packages {
        if pattern.is_empty() {
            return Err(RegistryError::BadRequest {
                reason: "rule override package pattern must not be empty".to_string(),
            });
        }
        if let Some(access) = &pkg.access {
            insert_list(connection, registry, pattern, ACCESS_ACTION, access)?;
        }
        if let Some(publish) = &pkg.publish {
            insert_list(connection, registry, pattern, PUBLISH_ACTION, publish)?;
        }
        if let Some(unpublish) = &pkg.unpublish {
            insert_list(connection, registry, pattern, UNPUBLISH_ACTION, unpublish)?;
        }
    }
    Ok(())
}

fn insert_list(
    connection: &dyn Connection,
    registry: &str,
    pattern: &str,
    action: &str,
    principals: &[String],
) -> Result<()> {
    connection.execute(
        "INSERT INTO rule_overrides (registry, pattern, action) VALUES (?1, ?2, ?3)",
        params![registry, pattern, action],
    )?;
    for (position, principal) in principals.iter().enumerate() {
        connection.execute(
            "INSERT INTO rule_override_grants (registry, pattern, action, position, principal)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![registry, pattern, action, to_column(position as u64), principal],
        )?;
    }
    Ok(())
}

fn decode_override_generation(registry: &str, value: i64) -> Result<u64> {
    if value < 1 {
        return Err(corrupt(registry, &format!("invalid generation {value}")));
    }
    Ok(to_generation(value))
}

fn corrupt(registry: &str, detail: &str) -> RegistryError {
    RegistryError::Internal {
        reason: format!("the stored rules of registry {registry:?} are unreadable: {detail}"),
    }
}
