//! Package owner rows and queries.

use super::{
    PackageKey,
    adapter::Connection,
    commit::{Commit, PackageRow},
    transactions::Freshness,
};
use crate::database::PackageMetadata;
use pnpr_error::Result;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_sql::DialectSql;
use serde_json::Value;
use std::collections::BTreeMap;

const INSERT_PACKAGE_OWNERS: DialectSql = DialectSql {
    sqlite: "INSERT INTO package_owners (namespace, ecosystem, name, user_id)
             SELECT ?1, ?2, ?3, value
             FROM (SELECT value FROM json_each(?4)) AS owners
             WHERE value != ''
             ON CONFLICT DO NOTHING",
    postgres: "INSERT INTO package_owners (namespace, ecosystem, name, user_id)
               SELECT ?1, ?2, ?3, value
               FROM (SELECT value FROM json_array_elements_text(CAST(?4 AS JSON)) AS elements(value)) AS owners
               WHERE value != ''
               ON CONFLICT DO NOTHING",
    mysql: "INSERT INTO package_owners (namespace, ecosystem, name, user_id)
            SELECT ?1, ?2, ?3, value
            FROM (SELECT JSON_UNQUOTE(value) AS value FROM JSON_TABLE(?4, '$[*]' COLUMNS (value JSON PATH '$')) AS elements) AS owners
            WHERE value != ''
            ON DUPLICATE KEY UPDATE namespace = package_owners.namespace",
};

const PACKAGE_OWNERS: &str = "SELECT 1 AS live, o.user_id AS user_id, NULL AS maintainers
     FROM packages p
     LEFT JOIN package_owners o
       ON o.namespace = p.namespace
      AND o.ecosystem = p.ecosystem
      AND o.name = p.name
     WHERE p.namespace = ?1 AND p.ecosystem = ?2 AND p.name = ?3
     UNION ALL
     SELECT 0 AS live, NULL AS user_id, r.maintainers AS maintainers
     FROM removed_packages r
     WHERE r.namespace = ?1 AND r.ecosystem = ?2 AND r.name = ?3
       AND NOT EXISTS (
           SELECT 1 FROM packages p
           WHERE p.namespace = ?1 AND p.ecosystem = ?2 AND p.name = ?3
       )
     ORDER BY user_id";

const PACKAGES_OWNERS: DialectSql = DialectSql {
    sqlite: "SELECT name, user_id FROM package_owners
             WHERE namespace = ?1 AND ecosystem = ?2
               AND name IN (SELECT value FROM json_each(?3))
             ORDER BY name, user_id",
    postgres: "SELECT name, user_id FROM package_owners
               WHERE namespace = ?1 AND ecosystem = ?2
                 AND name IN (SELECT value FROM json_array_elements_text(CAST(?3 AS JSON)) AS elements(value))
               ORDER BY name, user_id",
    mysql: "SELECT name, user_id FROM package_owners
            WHERE namespace = ?1 AND ecosystem = ?2
              AND name IN (SELECT JSON_UNQUOTE(value) AS value FROM JSON_TABLE(?3, '$[*]' COLUMNS (value JSON PATH '$')) AS elements)
            ORDER BY name, user_id",
};

impl Commit<'_> {
    pub(super) fn write_owners(&self, current: Option<&PackageRow>, state: &[u8]) -> Result<()> {
        let owners = state_owners(self.key.ecosystem, state)?;
        let previous = match current {
            Some(row) => state_owners(self.key.ecosystem, &row.state)?,
            None => Vec::new(),
        };
        if current.is_some() && previous == owners {
            return Ok(());
        }
        let (namespace, ecosystem, name) = self.key.params();
        if !previous.is_empty() {
            self.transaction.execute(
                "DELETE FROM package_owners WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3",
                params![namespace, ecosystem, name],
            )?;
        }
        if !owners.is_empty() {
            let owners = super::blobs::json_array(&owners);
            self.transaction.execute(
                INSERT_PACKAGE_OWNERS.get(self.transaction.dialect()),
                params![namespace, ecosystem, name, owners.as_str()],
            )?;
        }
        Ok(())
    }
}

impl PackageMetadata {
    /// The account IDs of `name`'s recorded owners on the primary, falling
    /// back to its tombstone when the package was removed, or `None` when the
    /// name has neither a live package nor a tombstone.
    pub(crate) async fn read_package_owners(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<Vec<String>>> {
        let key = self.key(name);
        self.database.read(self.namespace.clone(), Freshness::Current, move |connection, _| {
            query_package_owners(connection, &key)
        })
        .await
    }

    /// The account IDs of the recorded owners of each live package in `names`,
    /// keyed by package name. A live package with no owners has no entry.
    pub(crate) async fn read_packages_owners(
        &self,
        names: &[String],
    ) -> Result<BTreeMap<String, Vec<String>>> {
        if names.is_empty() {
            return Ok(BTreeMap::new());
        }
        let (namespace, ecosystem) = (self.namespace.clone(), self.ecosystem);
        let names = super::blobs::json_array(names);
        self.database.read(namespace.clone(), Freshness::Current, move |connection, _| {
            query_packages_owners(connection, &namespace, ecosystem, &names)
        })
        .await
    }
}

/// The account IDs of the package's owners recorded in `state`.
pub(super) fn state_owners(ecosystem: Ecosystem, state: &[u8]) -> Result<Vec<String>> {
    if ecosystem == Ecosystem::Npm {
        Ok(maintainer_ids(&serde_json::from_slice(state)?))
    } else {
        // Other ecosystems' documents are opaque here; one that does not
        // parse records no owners.
        Ok(serde_json::from_slice(state).map_or_else(|_| Vec::new(), |state| owner_ids(&state)))
    }
}

/// The `owners` a crate, project, or image document records.
fn owner_ids(state: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(owners) = state.get("owners").and_then(Value::as_array) {
        for entry in owners {
            if let Some(id) = entry.as_str()
                && !id.is_empty()
                && !ids
                    .iter()
                    .any(|existing| existing == id)
            {
                ids.push(id.to_string());
            }
        }
    }
    ids
}

fn maintainer_ids(state: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(entries) = state.get("maintainers").and_then(Value::as_array) {
        for entry in entries {
            if let Some(id) = entry.get("userId").and_then(Value::as_str)
                && !id.is_empty()
                && !ids
                    .iter()
                    .any(|existing| existing == id)
            {
                ids.push(id.to_string());
            }
        }
    }
    ids
}

fn query_package_owners(
    connection: &dyn Connection,
    key: &PackageKey,
) -> Result<Option<Vec<String>>> {
    let (namespace, ecosystem, name) = key.params();
    let rows = connection.query(PACKAGE_OWNERS, params![namespace, ecosystem, name])?;
    if rows.is_empty() {
        return Ok(None);
    }
    let mut rows = rows.into_iter();
    let mut first = rows.next().expect("non-empty rows");
    if first.get::<i64>(0)? == 0 {
        let maintainers: String = first.get(2)?;
        return Ok(Some(serde_json::from_str(&maintainers)?));
    }
    let mut owners = Vec::new();
    if let Some(owner) = first.get::<Option<String>>(1)? {
        owners.push(owner);
    }
    for mut row in rows {
        if let Some(owner) = row.get::<Option<String>>(1)? {
            owners.push(owner);
        }
    }
    Ok(Some(owners))
}

fn query_packages_owners(
    connection: &dyn Connection,
    namespace: &str,
    ecosystem: Ecosystem,
    names: &str,
) -> Result<BTreeMap<String, Vec<String>>> {
    let mut owners = BTreeMap::<String, Vec<String>>::new();
    for mut row in connection.query(
        PACKAGES_OWNERS.get(connection.dialect()),
        params![namespace, ecosystem.as_str(), names],
    )? {
        let name: String = row.get(0)?;
        let user_id: String = row.get(1)?;
        owners
            .entry(name)
            .or_default()
            .push(user_id);
    }
    Ok(owners)
}
