use super::{
    Expected, PackageKey,
    adapter::{Connection, Query, Row},
    batch::{Batch, Retired},
    commit::{package_row_from, package_row_query, parse_json},
    to_column,
    transactions::Freshness,
};
use crate::{BlobId, PackageTombstone, database::PackageMetadata};
use pnpr_error::Result;
use pnpr_package_name::Ecosystem;
use pnpr_sql::DialectSql;

pub(crate) enum RemovedPackage {
    Removed(Vec<BlobId>),
    NotFound,
    Conflict,
}

/// The tables holding a package's rows other than its history, each before
/// the tables its rows reference.
const PACKAGE_TABLES: [&str; 13] = [
    "artifact_blobs",
    "release_records",
    "accepted_manifests",
    "readmes",
    "annotations",
    "versions",
    "tags",
    "cargo_versions",
    "pypi_files",
    "oci_tags",
    "oci_manifests",
    "package_owners",
    "packages",
];

/// Remove every row of the package except its tag history and the identities
/// of its versions, which stay removed, under a new generation. Returns the
/// blobs its files were mapped to.
pub(super) fn remove_package(
    connection: &dyn Connection,
    key: &PackageKey,
    expected: &Expected,
    principal: Option<&str>,
) -> Result<RemovedPackage> {
    let (namespace, ecosystem, name) = key.params();
    let mapped = query!(
        "SELECT DISTINCT blob_id FROM artifact_blobs
         WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3",
        namespace,
        ecosystem,
        name,
    );
    // Only a stored package is removed, and a retired namespace holds none.
    let reads = [package_row_query(key), mapped];
    let (mut batch, results) =
        Batch::begin_with(connection, &key.namespace, Retired::Allowed, &reads)?;
    let mut results = results.into_iter();
    let Some(row) = package_row_from(key, results.next().unwrap_or_default())? else {
        return Ok(RemovedPackage::NotFound);
    };
    if !expected.admits(Some(row.generation), None) {
        return Ok(RemovedPackage::Conflict);
    }
    let ids = super::blobs::ids_from_rows(results.next().unwrap_or_default())?;
    record_removed_package(batch.transaction(), key, batch.generation(), &row.state, principal)?;
    for table in PACKAGE_TABLES {
        batch
            .transaction()
            .execute(
                &format!(
                    "DELETE FROM {table} WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3",
                ),
                params![namespace, ecosystem, name],
            )?;
    }
    batch.touch(key);
    batch.finish()?;
    Ok(RemovedPackage::Removed(ids))
}

const RECORD_PACKAGE: DialectSql = DialectSql::with_mysql(
    "INSERT INTO removed_packages (namespace, ecosystem, name, maintainers, unpublished_at, unpublished_by)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
     ON CONFLICT (namespace, ecosystem, name) DO UPDATE SET
       maintainers = CASE WHEN excluded.maintainers = '[]' THEN removed_packages.maintainers ELSE excluded.maintainers END,
       unpublished_at = excluded.unpublished_at, unpublished_by = excluded.unpublished_by",
    "INSERT INTO removed_packages (namespace, ecosystem, name, maintainers, unpublished_at, unpublished_by)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
     AS excluded ON DUPLICATE KEY UPDATE maintainers = CASE WHEN excluded.maintainers = '[]' THEN removed_packages.maintainers ELSE excluded.maintainers END,
       unpublished_at = excluded.unpublished_at, unpublished_by = excluded.unpublished_by",
);

pub(super) fn record_package(
    connection: &dyn Connection,
    key: &PackageKey,
    state: &[u8],
    principal: Option<&str>,
) -> Result<()> {
    let maintainers = super::owners::state_owners(key.ecosystem, state)?;
    let maintainers = serde_json::to_string(&maintainers)?;
    let (namespace, ecosystem, name) = key.params();
    connection.execute(
        RECORD_PACKAGE.get(connection.dialect()),
        params![namespace, ecosystem, name, maintainers, crate::publish::now_millis(), principal],
    )?;
    Ok(())
}

pub(super) fn record_removed_package(
    connection: &dyn Connection,
    key: &PackageKey,
    generation: u64,
    state: &[u8],
    principal: Option<&str>,
) -> Result<()> {
    record_package(connection, key, state, principal)?;
    record_removed_entries(connection, key, generation)
}

pub(super) const RECORD_REMOVED_PYPI_FILES: DialectSql = DialectSql::with_mysql(
    "INSERT INTO removed_pypi_files (namespace, ecosystem, name, filename, generation)
     SELECT namespace, ecosystem, name, filename, ?4 FROM pypi_files
     WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3
     ON CONFLICT DO NOTHING",
    "INSERT INTO removed_pypi_files (namespace, ecosystem, name, filename, generation)
     SELECT namespace, ecosystem, name, filename, ?4 FROM pypi_files
     WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3
     ON DUPLICATE KEY UPDATE namespace = removed_pypi_files.namespace",
);

pub(super) const RECORD_REMOVED_CARGO_VERSION_LIST: DialectSql = DialectSql {
    sqlite: "INSERT INTO removed_versions (namespace, ecosystem, name, version, generation)
             SELECT ?1, ?2, ?3, value, ?5
             FROM (SELECT value FROM json_each(?4)) AS removed
             WHERE value != ''
             ON CONFLICT DO NOTHING",
    postgres: "INSERT INTO removed_versions (namespace, ecosystem, name, version, generation)
               SELECT ?1, ?2, ?3, value, ?5
               FROM (SELECT value FROM json_array_elements_text(CAST(?4 AS JSON)) AS elements(value)) AS removed
               WHERE value != ''
               ON CONFLICT DO NOTHING",
    mysql: "INSERT INTO removed_versions (namespace, ecosystem, name, version, generation)
            SELECT ?1, ?2, ?3, value, ?5
            FROM (SELECT JSON_UNQUOTE(value) AS value FROM JSON_TABLE(?4, '$[*]' COLUMNS (value JSON PATH '$')) AS elements) AS removed
            WHERE value != ''
            ON DUPLICATE KEY UPDATE namespace = removed_versions.namespace",
};

pub(super) const DELETE_CARGO_VERSION_LIST: DialectSql = DialectSql {
    sqlite: "DELETE FROM cargo_versions
             WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3
               AND version IN (SELECT value FROM json_each(?4))",
    postgres: "DELETE FROM cargo_versions
               WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3
                 AND version IN (SELECT value FROM json_array_elements_text(CAST(?4 AS JSON)) AS elements(value))",
    mysql: "DELETE FROM cargo_versions
            WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3
              AND version IN (SELECT JSON_UNQUOTE(value) AS value FROM JSON_TABLE(?4, '$[*]' COLUMNS (value JSON PATH '$')) AS elements)",
};

pub(super) const DELETE_PYPI_FILE_LIST: DialectSql = DialectSql {
    sqlite: "DELETE FROM pypi_files
             WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3
               AND filename IN (SELECT value FROM json_each(?4))",
    postgres: "DELETE FROM pypi_files
               WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3
                 AND filename IN (SELECT value FROM json_array_elements_text(CAST(?4 AS JSON)) AS elements(value))",
    mysql: "DELETE FROM pypi_files
            WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3
              AND filename IN (SELECT JSON_UNQUOTE(value) AS value FROM JSON_TABLE(?4, '$[*]' COLUMNS (value JSON PATH '$')) AS elements)",
};

pub(super) const RECORD_REMOVED_PYPI_FILE_LIST: DialectSql = DialectSql {
    sqlite: "INSERT INTO removed_pypi_files (namespace, ecosystem, name, filename, generation)
             SELECT ?1, ?2, ?3, value, ?5
             FROM (SELECT value FROM json_each(?4)) AS removed
             WHERE value != ''
             ON CONFLICT DO NOTHING",
    postgres: "INSERT INTO removed_pypi_files (namespace, ecosystem, name, filename, generation)
               SELECT ?1, ?2, ?3, value, ?5
               FROM (SELECT value FROM json_array_elements_text(CAST(?4 AS JSON)) AS elements(value)) AS removed
               WHERE value != ''
               ON CONFLICT DO NOTHING",
    mysql: "INSERT INTO removed_pypi_files (namespace, ecosystem, name, filename, generation)
            SELECT ?1, ?2, ?3, value, ?5
            FROM (SELECT JSON_UNQUOTE(value) AS value FROM JSON_TABLE(?4, '$[*]' COLUMNS (value JSON PATH '$')) AS elements) AS removed
            WHERE value != ''
            ON DUPLICATE KEY UPDATE namespace = removed_pypi_files.namespace",
};

pub(super) const RECORD_REMOVED_CARGO_VERSIONS: DialectSql = DialectSql::with_mysql(
    "INSERT INTO removed_versions (namespace, ecosystem, name, version, generation)
     SELECT namespace, ecosystem, name, version, ?4 FROM cargo_versions
     WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3
     ON CONFLICT DO NOTHING",
    "INSERT INTO removed_versions (namespace, ecosystem, name, version, generation)
     SELECT namespace, ecosystem, name, version, ?4 FROM cargo_versions
     WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3
     ON DUPLICATE KEY UPDATE namespace = removed_versions.namespace",
);

const RECORD_REMOVED_NPM_VERSIONS: DialectSql = DialectSql::with_mysql(
    "INSERT INTO removed_versions (namespace, ecosystem, name, version, generation)
     SELECT namespace, ecosystem, name, version, ?4 FROM versions
     WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3
     ON CONFLICT DO NOTHING",
    "INSERT INTO removed_versions (namespace, ecosystem, name, version, generation)
     SELECT namespace, ecosystem, name, version, ?4 FROM versions
     WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3
     ON DUPLICATE KEY UPDATE namespace = removed_versions.namespace",
);

fn record_removed_entries(
    connection: &dyn Connection,
    key: &PackageKey,
    generation: u64,
) -> Result<()> {
    let (namespace, ecosystem, name) = key.params();
    let column_gen = to_column(generation);
    match key.ecosystem {
        Ecosystem::Npm => connection.execute(
            RECORD_REMOVED_NPM_VERSIONS.get(connection.dialect()),
            params![namespace, ecosystem, name, column_gen],
        ),
        Ecosystem::Cargo => connection.execute(
            RECORD_REMOVED_CARGO_VERSIONS.get(connection.dialect()),
            params![namespace, ecosystem, name, column_gen],
        ),
        Ecosystem::Pypi => connection.execute(
            RECORD_REMOVED_PYPI_FILES.get(connection.dialect()),
            params![namespace, ecosystem, name, column_gen],
        ),
        Ecosystem::Oci => record_removed_all_oci_digests(connection, key, generation),
    }
}

fn record_removed_all_oci_digests(
    connection: &dyn Connection,
    key: &PackageKey,
    generation: u64,
) -> Result<()> {
    let (namespace, ecosystem, name) = key.params();
    let digests: Vec<String> = connection
        .query(
            "SELECT digest FROM oci_manifests
             WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3",
            params![namespace, ecosystem, name],
        )?
        .into_iter()
        .map(|mut row| row.get(0))
        .collect::<Result<_>>()?;
    record_removed_oci_digests(connection, key, generation, digests.iter().map(String::as_str))
}

const RECORD_REMOVED_MANIFESTS: DialectSql = DialectSql {
    sqlite: "INSERT INTO removed_oci_manifests (namespace, ecosystem, name, artifact, generation)
             SELECT ?1, ?2, ?3, value, ?5
             FROM (SELECT value FROM json_each(?4)) AS removed
             WHERE value != ''
             ON CONFLICT (namespace, ecosystem, name, artifact) DO UPDATE SET
               generation = excluded.generation",
    postgres: "INSERT INTO removed_oci_manifests (namespace, ecosystem, name, artifact, generation)
               SELECT ?1, ?2, ?3, value, ?5
               FROM (SELECT value FROM json_array_elements_text(CAST(?4 AS JSON)) AS elements(value)) AS removed
               WHERE value != ''
               ON CONFLICT (namespace, ecosystem, name, artifact) DO UPDATE SET
                 generation = excluded.generation",
    mysql: "INSERT INTO removed_oci_manifests (namespace, ecosystem, name, artifact, generation)
            SELECT ?1, ?2, ?3, value, ?5
            FROM (SELECT JSON_UNQUOTE(value) AS value FROM JSON_TABLE(?4, '$[*]' COLUMNS (value JSON PATH '$')) AS elements) AS removed
            WHERE value != ''
            ON DUPLICATE KEY UPDATE generation = VALUES(generation)",
};

pub(super) fn record_removed_oci_digests<'digests>(
    connection: &dyn Connection,
    key: &PackageKey,
    generation: u64,
    digests: impl IntoIterator<Item = &'digests str>,
) -> Result<()> {
    let removed: Vec<String> = digests
        .into_iter()
        .map(|digest| digest.replacen(':', "-", 1))
        .collect();
    if removed.is_empty() {
        return Ok(());
    }
    let (namespace, ecosystem, name) = key.params();
    let removed = super::blobs::json_array(&removed);
    connection.execute(
        RECORD_REMOVED_MANIFESTS.get(connection.dialect()),
        params![namespace, ecosystem, name, removed, to_column(generation)],
    )
}

impl PackageMetadata {
    pub(crate) async fn read_tombstone(
        &self,
        name: &pnpr_package_name::CanonicalPackageName,
    ) -> Result<Option<PackageTombstone>> {
        let key = self.key(name);
        self.database.read(self.namespace.clone(), Freshness::Current, move |connection, _| {
            read_package(connection, &key)
        })
        .await
    }
}

pub(super) fn tombstone_owners_query(key: &PackageKey) -> Query<'_> {
    let (namespace, ecosystem, name) = key.params();
    query!(
        "SELECT maintainers FROM removed_packages
         WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3",
        namespace,
        ecosystem,
        name,
    )
}

pub(super) fn tombstone_owners_from(rows: Vec<Row>) -> Result<Option<Vec<String>>> {
    rows.into_iter()
        .next()
        .map(|mut row| parse_json(&row.get::<String>(0)?))
        .transpose()
}

fn read_package(connection: &dyn Connection, key: &PackageKey) -> Result<Option<PackageTombstone>> {
    let (namespace, ecosystem, name) = key.params();
    let Some(mut row) = connection.query_optional(
        "SELECT maintainers, unpublished_at, unpublished_by FROM removed_packages
         WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3",
        params![namespace, ecosystem, name],
    )?
    else {
        return Ok(None);
    };
    let maintainers: String = row.get(0)?;
    let unpublished_versions = connection
        .query(
            "SELECT version FROM removed_versions
             WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3 ORDER BY version",
            params![namespace, ecosystem, name],
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
