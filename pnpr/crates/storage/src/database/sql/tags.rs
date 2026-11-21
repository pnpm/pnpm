//! Dist-tags and their insert-only history.

use super::{
    Expected,
    Freshness,
    PackageKey,
    adapter::{
        Connection,
        Query,
        Row,
    },
    batch::{
        Batch,
        Retired,
    },
    commit::{
        Commit,
        PackageRow,
        package_row_from,
        package_row_query,
        parse_optional_json,
    },
    packument::{
        PackumentRows,
        Readme,
        TIME,
    },
    to_column,
};
use crate::{
    Storage,
    database::{
        TagChange,
        TagUpdate,
        validate_dist_tag,
        validate_dist_tag_target,
    },
    publish::{
        iso_from_unix_millis,
        now_iso,
        now_millis,
    },
};
use pnpr_error::Result;
use pnpr_package_name::{
    CanonicalPackageName,
    Ecosystem,
};
use pnpr_sql::DialectSql;
use serde_json::{
    Map,
    Value,
};
use std::collections::BTreeMap;

/// A [`TagChange`] the blocking pool can own.
#[derive(Debug)]
pub(super) struct OwnedTagChange {
    tag: String,
    target: Option<String>,
    principal: Option<String>,
    expected: Expected,
}

impl TryFrom<TagChange<'_>> for OwnedTagChange {
    type Error = pnpr_error::RegistryError;

    fn try_from(change: TagChange<'_>) -> Result<Self> {
        let expected = match change.expected {
            Some(version) => Expected::from_version(Some(version))?,
            None => Expected::Any,
        };
        Ok(Self {
            tag: change.tag.to_string(),
            target: change.target.map(str::to_string),
            principal: change.principal.map(str::to_string),
            expected,
        })
    }
}

/// One move of a tag, as its history records it.
pub(super) struct TagMove<'change> {
    pub(super) tag: &'change str,
    pub(super) previous: Option<&'change str>,
    pub(super) target: Option<&'change str>,
    pub(super) principal: Option<&'change str>,
}

pub(super) fn stored_tags_query(key: &PackageKey) -> Query<'_> {
    let (namespace, name) = key.params_in(Ecosystem::Npm);
    query!("SELECT tag, version FROM npm_tags WHERE namespace = ?1 AND name = ?2", namespace, name)
}

/// The tags in the rows of [`stored_tags_query`].
pub(super) fn tags_from_rows(rows: Vec<Row>) -> Result<BTreeMap<String, String>> {
    rows.into_iter()
        .map(|mut row| Ok((row.get(0)?, row.get(1)?)))
        .collect()
}

const WRITE_TAG: DialectSql = DialectSql::with_mysql(
    "INSERT INTO npm_tags (namespace, name, tag, version, generation)
     VALUES (?1, ?2, ?3, ?4, ?5)
     ON CONFLICT (namespace, name, tag)
     DO UPDATE SET version = excluded.version, generation = excluded.generation",
    "INSERT INTO npm_tags (namespace, name, tag, version, generation)
     VALUES (?1, ?2, ?3, ?4, ?5)
     AS excluded ON DUPLICATE KEY UPDATE version = excluded.version, generation = excluded.generation",
);

/// Point the tag where `change` says and append the move to its history.
pub(super) fn record_tag_move(commit: &Commit<'_>, change: &TagMove<'_>) -> Result<()> {
    let (namespace, name) = commit.key.params_in(Ecosystem::Npm);
    let generation = to_column(commit.generation);
    match change.target {
        Some(target) => commit.transaction.execute(
            WRITE_TAG.get(commit.transaction.dialect()),
            params![namespace, name, change.tag, target, generation],
        )?,
        None => commit.transaction.execute(
            "DELETE FROM npm_tags WHERE namespace = ?1 AND name = ?2 AND tag = ?3",
            params![namespace, name, change.tag],
        )?,
    }
    commit.transaction.execute(
        "INSERT INTO npm_tag_history (namespace, name, tag, generation, previous, target,
                                      principal, changed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            namespace,
            name,
            change.tag,
            generation,
            change.previous,
            change.target,
            change.principal,
            now_millis(),
        ],
    )?;
    Ok(())
}

/// Move one tag of an npm package in its own commit, stamping
/// `time.modified`. A change that leaves the tag where it is commits nothing.
pub(super) fn change_tag(
    connection: &dyn Connection,
    key: &PackageKey,
    change: &OwnedTagChange,
) -> Result<TagUpdate> {
    if key.ecosystem != Ecosystem::Npm {
        return Ok(TagUpdate::NotFound);
    }
    validate_dist_tag(&change.tag)?;
    let reads = [package_row_query(key), stored_tags_query(key), tag_target_query(key, change)];
    // A tag moves only within a stored package, and a retired namespace holds none.
    let (mut batch, results) =
        Batch::begin_with(connection, &key.namespace, Retired::Allowed, &reads)?;
    let mut results = results.into_iter();
    let Some(package) = package_row_from(key, results.next().unwrap_or_default())? else {
        return Ok(TagUpdate::NotFound);
    };
    if !change.expected.admits(Some(package.generation), None) {
        return Ok(TagUpdate::Conflict);
    }
    let tags = tags_from_rows(results.next().unwrap_or_default())?;
    let target = results
        .next()
        .unwrap_or_default()
        .into_iter()
        .next();
    validate_dist_tag_target(&change.tag, change.target.as_deref(), target.is_some())?;
    if tags.get(&change.tag).map(String::as_str) == change.target.as_deref() {
        let generation = Some(key.generation(package.generation));
        return Ok(TagUpdate::Committed { generation, moved: false });
    }
    move_tag(&batch.package(key), &package, &tags, change, target)?;
    batch.touch(key);
    let generation = batch
        .finish()?
        .map(|value| key.generation(value));
    Ok(TagUpdate::Committed { generation, moved: true })
}

fn tag_target_query<'query>(
    key: &'query PackageKey,
    change: &'query OwnedTagChange,
) -> Query<'query> {
    let (namespace, name) = key.params_in(Ecosystem::Npm);
    match change.target.as_deref() {
        Some(target) if change.tag == "latest" => query!(
            "SELECT r.readme, r.readme_filename FROM npm_versions v
             LEFT JOIN npm_readmes r USING (namespace, name, version)
             WHERE v.namespace = ?1 AND v.name = ?2 AND v.version = ?3",
            namespace,
            name,
            target,
        ),
        Some(target) => query!(
            "SELECT version FROM npm_versions
             WHERE namespace = ?1 AND name = ?2 AND version = ?3 LIMIT 1",
            namespace,
            name,
            target,
        ),
        None => query!(
            "SELECT version FROM npm_versions WHERE namespace = ?1 AND name = ?2 LIMIT 1",
            namespace,
            name,
        ),
    }
}

pub(super) fn validate_packument_tags(
    stored: &BTreeMap<String, String>,
    rows: &PackumentRows,
) -> Result<()> {
    for (tag, target) in &rows.tags {
        validate_dist_tag(tag)?;
        validate_dist_tag_target(tag, Some(target), rows.versions.contains_key(target))?;
    }
    if stored.contains_key("latest") && !rows.tags.contains_key("latest") {
        validate_dist_tag_target("latest", None, !rows.versions.is_empty())?;
    }
    Ok(())
}

/// Commit the tag move and its package-level metadata together.
fn move_tag(
    commit: &Commit<'_>,
    package: &PackageRow,
    tags: &BTreeMap<String, String>,
    change: &OwnedTagChange,
    target: Option<Row>,
) -> Result<()> {
    let previous = tags.get(&change.tag).map(String::as_str);
    let tag_move = TagMove {
        tag: &change.tag,
        previous,
        target: change.target.as_deref(),
        principal: change.principal.as_deref(),
    };
    record_tag_move(commit, &tag_move)?;
    let mut state = serde_json::from_slice(&package.state)?;
    if change.tag == "latest" {
        let readme = match target {
            Some(mut row) => Readme {
                text: parse_optional_json(row.get::<Option<String>>(0)?.as_deref())?,
                filename: parse_optional_json(row.get::<Option<String>>(1)?.as_deref())?,
            },
            None => Readme::default(),
        };
        readme.replace_in(&mut state);
    }
    stamp_modified(&mut state);
    commit.write_state(package.state_version, &serde_json::to_vec(&state)?)?;
    Ok(())
}

/// The package-level fields with `time.modified` set to now.
fn stamp_modified(state: &mut Map<String, Value>) {
    let time = state
        .entry(TIME)
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(time) = time.as_object_mut() {
        time.insert("modified".to_string(), Value::String(now_iso()));
    }
}

impl Storage {
    /// The most recent former npm `latest` that will survive removal of `version`.
    pub async fn previous_published_latest(
        &self,
        name: &CanonicalPackageName,
        version: &str,
    ) -> Result<Option<String>> {
        let key = self.metadata.key(name);
        if key.ecosystem != Ecosystem::Npm {
            return Ok(None);
        }
        let version = version.to_owned();
        self.database.read(key.namespace.clone(), Freshness::Current, move |connection, _| {
            let (namespace, name) = key.params_in(Ecosystem::Npm);
            connection
                .query_optional(
                    "SELECT h.previous FROM npm_tag_history h
                     JOIN npm_versions v ON v.namespace = h.namespace AND v.name = h.name
                                        AND v.version = h.previous
                     WHERE h.namespace = ?1 AND h.name = ?2 AND h.tag = 'latest' AND h.previous <> ?3
                     ORDER BY h.generation DESC LIMIT 1",
                    params![namespace, name, version],
                )?
                .map(|mut row| row.get(0))
                .transpose()
        })
        .await
    }
}

/// One recorded move of a tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagHistoryEntry {
    /// The namespace generation that moved the tag.
    pub generation: u64,
    pub previous: Option<String>,
    /// `None` when the move removed the tag.
    pub target: Option<String>,
    pub principal: Option<String>,
    pub changed_at: String,
}

pub(super) fn tag_history(
    connection: &dyn Connection,
    key: &PackageKey,
    tag: &str,
) -> Result<Vec<TagHistoryEntry>> {
    if key.ecosystem != Ecosystem::Npm {
        return Ok(Vec::new());
    }
    let (namespace, name) = key.params_in(Ecosystem::Npm);
    connection
        .query(
            "SELECT generation, previous, target, principal, changed_at FROM npm_tag_history
             WHERE namespace = ?1 AND name = ?2 AND tag = ?3
             ORDER BY generation",
            params![namespace, name, tag],
        )?
        .into_iter()
        .map(|mut row| {
            Ok(TagHistoryEntry {
                generation: super::to_generation(row.get(0)?),
                previous: row.get(1)?,
                target: row.get(2)?,
                principal: row.get(3)?,
                changed_at: iso_from_unix_millis(row.get(4)?),
            })
        })
        .collect()
}
