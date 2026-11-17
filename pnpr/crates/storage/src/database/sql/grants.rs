//! The grants of the instance: which subjects may do which actions on which
//! packages of which registries.

use super::{
    adapter::{Connection, Row},
    batch::WriteTransaction,
    records::advance_record_generation,
    to_generation,
    transactions::Freshness,
};
use crate::Storage;
use pnpr_error::{RegistryError, Result};
use std::collections::HashMap;

/// What a grant holds, apart from its ID.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct GrantContent {
    pub name: String,
    /// Registry keys; `*` is every registry.
    pub registries: Vec<String>,
    /// The patterns of the packages the grant covers; none is every package.
    pub include: Vec<String>,
    /// The patterns of the packages taken out of it.
    pub exclude: Vec<String>,
    /// Each `(subject, action)` the grant gives.
    pub subjects: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredGrant {
    pub id: String,
    pub content: GrantContent,
}

/// Every grant, sorted by name, as of `generation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredGrants {
    pub generation: u64,
    pub grants: Vec<StoredGrant>,
}

/// The outcome of a write to the grants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantWrite {
    /// Written, as this generation of the grants.
    Written(u64),
    /// The grants changed since the generation the caller expected.
    Conflict { current: u64 },
    /// Another grant has the name.
    NameTaken,
    /// No grant has the ID.
    NotFound,
    /// A subject names this team ID, which no team has.
    UnknownTeam(String),
}

impl Storage {
    /// The grants, or `None` while their generation is still `known`.
    pub async fn read_grants(&self, known: Option<u64>) -> Result<Option<StoredGrants>> {
        self.database.read(String::new(), Freshness::Current, move |connection, _| {
            let generation = directory_generation(connection)?;
            if known == Some(generation) {
                return Ok(None);
            }
            read_grants(connection).map(|grants| Some(StoredGrants { generation, grants }))
        })
        .await
    }

    /// Store `content` as the grant `id`, creating it or replacing what it
    /// held, if the grants are still at `expected` when one is given.
    pub async fn put_grant(
        &self,
        id: &str,
        content: GrantContent,
        expected: Option<u64>,
    ) -> Result<GrantWrite> {
        let id = id.to_string();
        self.database.write(move |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            if let Some(refused) = refused_write(&*transaction, &id, &content, expected)? {
                return Ok(refused);
            }
            let was_public = gives_public_read(&*transaction, Some(&id))?;
            delete_grant(&*transaction, &id)?;
            insert_grant(&*transaction, &id, &content)?;
            purge_public_reads(&*transaction, was_public || is_public(&content.subjects))?;
            let generation = advance_directory(&*transaction)?;
            transaction.commit()?;
            Ok(GrantWrite::Written(generation))
        })
        .await
    }

    /// Make `grants` the grants of the instance, in place of the ones it has,
    /// if they are still at `expected` when one is given.
    pub async fn replace_grants(
        &self,
        grants: Vec<StoredGrant>,
        expected: Option<u64>,
    ) -> Result<GrantWrite> {
        self.database.write(move |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            if let Some(refused) = refused_replacement(&*transaction, &grants, expected)? {
                return Ok(refused);
            }
            let public = gives_public_read(&*transaction, None)?
                || grants.iter().any(|grant| is_public(&grant.content.subjects));
            replace_all(&*transaction, &grants)?;
            purge_public_reads(&*transaction, public)?;
            let generation = advance_directory(&*transaction)?;
            transaction.commit()?;
            Ok(GrantWrite::Written(generation))
        })
        .await
    }

    /// Replace the subjects of the grant `id`, leaving the rest of it.
    pub async fn set_grant_subjects(
        &self,
        id: &str,
        subjects: Vec<(String, String)>,
        expected: Option<u64>,
    ) -> Result<GrantWrite> {
        let id = id.to_string();
        self.database.write(move |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            if let Some(conflict) = conflict(&*transaction, expected)? {
                return Ok(conflict);
            }
            if !grant_exists(&*transaction, &id)? {
                return Ok(GrantWrite::NotFound);
            }
            if let Some(team) = unknown_subject_team(&*transaction, &subjects)? {
                return Ok(GrantWrite::UnknownTeam(team));
            }
            let was_public = gives_public_read(&*transaction, Some(&id))?;
            transaction.execute("DELETE FROM grant_subjects WHERE grant_id = ?1", params![&id])?;
            insert_subjects(&*transaction, &id, &subjects)?;
            purge_public_reads(&*transaction, was_public || is_public(&subjects))?;
            let generation = advance_directory(&*transaction)?;
            transaction.commit()?;
            Ok(GrantWrite::Written(generation))
        })
        .await
    }

    /// Remove the grant `id`, if the grants are still at `expected` when one
    /// is given.
    pub async fn remove_grant(&self, id: &str, expected: Option<u64>) -> Result<GrantWrite> {
        let id = id.to_string();
        self.database.write(move |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            if let Some(conflict) = conflict(&*transaction, expected)? {
                return Ok(conflict);
            }
            if !grant_exists(&*transaction, &id)? {
                return Ok(GrantWrite::NotFound);
            }
            let was_public = gives_public_read(&*transaction, Some(&id))?;
            delete_grant(&*transaction, &id)?;
            purge_public_reads(&*transaction, was_public)?;
            let generation = advance_directory(&*transaction)?;
            transaction.commit()?;
            Ok(GrantWrite::Written(generation))
        })
        .await
    }
}

fn refused_replacement(
    connection: &dyn Connection,
    grants: &[StoredGrant],
    expected: Option<u64>,
) -> Result<Option<GrantWrite>> {
    if let Some(conflict) = conflict(connection, expected)? {
        return Ok(Some(conflict));
    }
    for grant in grants {
        if let Some(team) = unknown_subject_team(connection, &grant.content.subjects)? {
            return Ok(Some(GrantWrite::UnknownTeam(team)));
        }
    }
    Ok(None)
}

fn replace_all(connection: &dyn Connection, grants: &[StoredGrant]) -> Result<()> {
    for table in ["grant_subjects", "grant_patterns", "grant_registries", "grants"] {
        connection.execute(&format!("DELETE FROM {table}"), params![])?;
    }
    for grant in grants {
        insert_grant(connection, &grant.id, &grant.content)?;
    }
    Ok(())
}

/// What a graph write changes in the grants: the registries it removes leave
/// every grant, and the registries it adds get their default grants.
pub(super) fn follow_graph(
    connection: &dyn Connection,
    removed: &[String],
    seeded: &[StoredGrant],
) -> Result<()> {
    forget_registries(connection, removed)?;
    seed_grants(connection, seeded)
}

/// Create each of `grants` whose name no grant has yet. A graph write seeds a
/// registry it adds this way, in its transaction.
fn seed_grants(connection: &dyn Connection, grants: &[StoredGrant]) -> Result<()> {
    let mut seeded = false;
    for grant in grants {
        if name_owner(connection, &grant.content.name)?.is_some() {
            continue;
        }
        insert_grant(connection, &grant.id, &grant.content)?;
        seeded = true;
    }
    if seeded {
        advance_directory(connection)?;
    }
    Ok(())
}

/// Take each of `registries` out of every grant, and remove the grants left
/// without a registry. A graph write does this for the registries it removes,
/// so a registry added later under the same key starts without their grants.
fn forget_registries(connection: &dyn Connection, registries: &[String]) -> Result<()> {
    let mut changed = false;
    for registry in registries {
        let named = connection.query_optional(
            "SELECT 1 FROM grant_registries WHERE registry = ?1 LIMIT 1",
            params![registry],
        )?;
        if named.is_none() {
            continue;
        }
        connection.execute("DELETE FROM grant_registries WHERE registry = ?1", params![registry])?;
        changed = true;
    }
    if !changed {
        return Ok(());
    }
    let orphans = connection.query(
        "SELECT grant_id FROM grants
         WHERE grant_id NOT IN (SELECT grant_id FROM grant_registries)",
        params![],
    )?;
    for mut orphan in orphans {
        delete_grant(connection, &orphan.get::<String>(0)?)?;
    }
    advance_directory(connection).map(drop)
}

/// Whether a grant gives an action to the team `id`.
pub(super) fn grants_name_team(connection: &dyn Connection, id: &str) -> Result<bool> {
    let subject = format!("team:{id}");
    Ok(connection
        .query_optional(
            "SELECT 1 FROM grant_subjects WHERE subject = ?1 LIMIT 1",
            params![&subject],
        )?
        .is_some())
}

/// Whether `subjects` let a caller read without signing in.
fn is_public(subjects: &[(String, String)]) -> bool {
    subjects
        .iter()
        .any(|(subject, action)| {
            action == "read" && matches!(subject.as_str(), "$all" | "$anonymous")
        })
}

/// Whether the grant `id`, or any grant when `None`, lets a caller read
/// without signing in.
fn gives_public_read(connection: &dyn Connection, id: Option<&str>) -> Result<bool> {
    const PUBLIC: &str = "action = 'read' AND subject IN ('$all', '$anonymous')";
    let found = match id {
        Some(id) => connection.query_optional(
            &format!("SELECT 1 FROM grant_subjects WHERE grant_id = ?1 AND {PUBLIC} LIMIT 1"),
            params![id],
        )?,
        None => connection.query_optional(
            &format!("SELECT 1 FROM grant_subjects WHERE {PUBLIC} LIMIT 1"),
            params![],
        )?,
    };
    Ok(found.is_some())
}

/// Invalidate every CDN-cached response when a write changed a grant that
/// lets callers read without signing in, the only reads a shared cache holds.
/// A write to any other grant leaves the cache alone.
fn purge_public_reads(connection: &dyn Connection, public: bool) -> Result<()> {
    if public {
        super::cdn::enqueue(connection, "*")?;
    }
    Ok(())
}

fn refused_write(
    connection: &dyn Connection,
    id: &str,
    content: &GrantContent,
    expected: Option<u64>,
) -> Result<Option<GrantWrite>> {
    if let Some(conflict) = conflict(connection, expected)? {
        return Ok(Some(conflict));
    }
    if name_owner(connection, &content.name)?.is_some_and(|owner| owner != id) {
        return Ok(Some(GrantWrite::NameTaken));
    }
    Ok(unknown_subject_team(connection, &content.subjects)?.map(GrantWrite::UnknownTeam))
}

fn conflict(connection: &dyn Connection, expected: Option<u64>) -> Result<Option<GrantWrite>> {
    let Some(expected) = expected else {
        return Ok(None);
    };
    let current = directory_generation(connection)?;
    Ok((current != expected).then_some(GrantWrite::Conflict { current }))
}

/// The first team a subject of `subjects` names that does not exist.
fn unknown_subject_team(
    connection: &dyn Connection,
    subjects: &[(String, String)],
) -> Result<Option<String>> {
    let teams: Vec<String> = subjects
        .iter()
        .filter_map(|(subject, _)| subject.strip_prefix("team:"))
        .map(str::to_string)
        .collect();
    super::teams::unknown_team(connection, &teams)
}

/// The ID of the grant named `name`.
fn name_owner(connection: &dyn Connection, name: &str) -> Result<Option<String>> {
    connection
        .query_optional("SELECT grant_id FROM grants WHERE name = ?1", params![name])?
        .map(|mut row| row.get(0))
        .transpose()
}

fn grant_exists(connection: &dyn Connection, id: &str) -> Result<bool> {
    Ok(connection
        .query_optional("SELECT 1 FROM grants WHERE grant_id = ?1", params![id])?
        .is_some())
}

fn delete_grant(connection: &dyn Connection, id: &str) -> Result<()> {
    for table in ["grant_subjects", "grant_patterns", "grant_registries", "grants"] {
        connection.execute(&format!("DELETE FROM {table} WHERE grant_id = ?1"), params![id])?;
    }
    Ok(())
}

fn insert_grant(connection: &dyn Connection, id: &str, content: &GrantContent) -> Result<()> {
    connection.execute(
        "INSERT INTO grants (grant_id, name) VALUES (?1, ?2)",
        params![id, &content.name],
    )?;
    for registry in &content.registries {
        connection.execute(
            "INSERT INTO grant_registries (grant_id, registry) VALUES (?1, ?2)",
            params![id, registry],
        )?;
    }
    for (exclude, patterns) in [(0_i64, &content.include), (1, &content.exclude)] {
        for pattern in patterns {
            connection.execute(
                "INSERT INTO grant_patterns (grant_id, exclude, pattern) VALUES (?1, ?2, ?3)",
                params![id, exclude, pattern],
            )?;
        }
    }
    insert_subjects(connection, id, &content.subjects)
}

fn insert_subjects(
    connection: &dyn Connection,
    id: &str,
    subjects: &[(String, String)],
) -> Result<()> {
    for (subject, action) in subjects {
        connection.execute(
            "INSERT INTO grant_subjects (grant_id, subject, action) VALUES (?1, ?2, ?3)",
            params![id, subject, action],
        )?;
    }
    Ok(())
}

fn directory_generation(connection: &dyn Connection) -> Result<u64> {
    let mut row = connection
        .query_optional("SELECT generation FROM grant_directory WHERE singleton = 1", params![])?
        .ok_or_else(|| RegistryError::Internal {
            reason: "the grant directory generation is missing".to_string(),
        })?;
    Ok(to_generation(row.get(0)?))
}

fn advance_directory(connection: &dyn Connection) -> Result<u64> {
    let generation = advance_record_generation(connection)?;
    connection.execute(
        "UPDATE grant_directory SET generation = ?1 WHERE singleton = 1",
        params![generation],
    )?;
    Ok(to_generation(generation))
}

fn read_grants(connection: &dyn Connection) -> Result<Vec<StoredGrant>> {
    let mut grants = connection
        .query("SELECT grant_id, name FROM grants ORDER BY name", params![])?
        .into_iter()
        .map(|mut row| {
            let content = GrantContent { name: row.get(1)?, ..GrantContent::default() };
            Ok(StoredGrant { id: row.get(0)?, content })
        })
        .collect::<Result<Vec<_>>>()?;
    let positions: HashMap<String, usize> = grants
        .iter()
        .enumerate()
        .map(|(position, grant)| (grant.id.clone(), position))
        .collect();
    let mut fill = |sql: &str, add: &dyn Fn(&mut GrantContent, &mut Row) -> Result<()>| {
        for mut row in connection.query(sql, params![])? {
            if let Some(position) = positions.get(&row.get::<String>(0)?) {
                add(&mut grants[*position].content, &mut row)?;
            }
        }
        Ok::<_, RegistryError>(())
    };
    fill(
        "SELECT grant_id, registry FROM grant_registries ORDER BY grant_id, registry",
        &add_registry,
    )?;
    fill(
        "SELECT grant_id, exclude, pattern FROM grant_patterns ORDER BY grant_id, exclude, pattern",
        &add_pattern,
    )?;
    fill(
        "SELECT grant_id, subject, action FROM grant_subjects ORDER BY grant_id, subject, action",
        &add_subject,
    )?;
    Ok(grants)
}

fn add_registry(content: &mut GrantContent, row: &mut Row) -> Result<()> {
    content.registries.push(row.get(1)?);
    Ok(())
}

fn add_subject(content: &mut GrantContent, row: &mut Row) -> Result<()> {
    content.subjects.push((row.get(1)?, row.get(2)?));
    Ok(())
}

fn add_pattern(content: &mut GrantContent, row: &mut Row) -> Result<()> {
    let list = if row.get::<i64>(1)? == 0 { &mut content.include } else { &mut content.exclude };
    list.push(row.get(2)?);
    Ok(())
}
