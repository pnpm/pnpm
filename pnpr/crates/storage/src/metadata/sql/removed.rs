use super::{PackageKey, adapter::Connection, transactions::Freshness};
use crate::{PackageTombstone, metadata::PackageMetadata};
use pnpr_error::Result;
use serde_json::Value;

pub(super) fn record_package(
    connection: &dyn Connection,
    key: &PackageKey,
    state: &[u8],
    principal: Option<&str>,
) -> Result<()> {
    let maintainers = if key.ecosystem == pnpr_package_name::Ecosystem::Npm {
        let state: Value = serde_json::from_slice(state)?;
        maintainer_ids(&state)
    } else {
        Vec::new()
    };
    let maintainers = serde_json::to_string(&maintainers)?;
    let (catalog, ecosystem, name) = key.params();
    connection.execute(
        "INSERT INTO removed_packages (catalog, ecosystem, name, maintainers, unpublished_at, unpublished_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (catalog, ecosystem, name) DO UPDATE SET
           maintainers = CASE WHEN excluded.maintainers = '[]' THEN removed_packages.maintainers ELSE excluded.maintainers END,
           unpublished_at = excluded.unpublished_at, unpublished_by = excluded.unpublished_by",
        params![catalog, ecosystem, name, maintainers, crate::publish::now_millis(), principal],
    )?;
    Ok(())
}

fn maintainer_ids(state: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(entries) = state.get("maintainers").and_then(Value::as_array) {
        for entry in entries {
            if let Some(id) = entry.get("userId").and_then(Value::as_str)
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

impl PackageMetadata {
    pub(crate) async fn read_tombstone(
        &self,
        name: &pnpr_package_name::CanonicalPackageName,
    ) -> Result<Option<PackageTombstone>> {
        let key = self.key(name);
        self.database.read(self.catalog.clone(), Freshness::Current, move |connection, _| {
            read_package(connection, &key)
        })
        .await
    }
}

fn read_package(connection: &dyn Connection, key: &PackageKey) -> Result<Option<PackageTombstone>> {
    let (catalog, ecosystem, name) = key.params();
    let Some(mut row) = connection.query_optional(
        "SELECT maintainers, unpublished_at, unpublished_by FROM removed_packages
         WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3",
        params![catalog, ecosystem, name],
    )?
    else {
        return Ok(None);
    };
    let maintainers: String = row.get(0)?;
    let unpublished_versions = connection
        .query(
            "SELECT DISTINCT version FROM removed_versions
         WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3 ORDER BY version",
            params![catalog, ecosystem, name],
        )?
        .into_iter()
        .map(|mut row| row.get(0))
        .collect::<Result<_>>()?;
    Ok(Some(PackageTombstone {
        package: name.to_string(),
        maintainers: serde_json::from_str(&maintainers)?,
        unpublished_at: crate::publish::iso_from_unix_millis(row.get(1)?),
        unpublished_by: row.get(2)?,
        unpublished_versions,
    }))
}
