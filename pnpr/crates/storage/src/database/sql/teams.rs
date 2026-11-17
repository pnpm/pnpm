//! The teams of the instance and the accounts in them.

use super::{
    adapter::Connection, batch::WriteTransaction, records::advance_record_generation,
    to_generation, transactions::Freshness,
};
use crate::Storage;
use pnpr_error::{RegistryError, Result};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredTeam {
    pub id: String,
    pub name: String,
    /// Account IDs, sorted.
    pub members: Vec<String>,
}

/// Every team, sorted by name, as of `generation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredTeams {
    pub generation: u64,
    pub teams: Vec<StoredTeam>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamCreation {
    Created,
    NameTaken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamRemoval {
    Removed,
    NotFound,
    /// An access list of the stored registry graph names the team, so nothing
    /// was removed.
    Named,
}

impl Storage {
    /// The teams, or `None` while their generation is still `known`.
    pub async fn read_teams(&self, known: Option<u64>) -> Result<Option<StoredTeams>> {
        self.database.read(String::new(), Freshness::Current, move |connection, _| {
            let generation = directory_generation(connection)?;
            if known == Some(generation) {
                return Ok(None);
            }
            read_teams(connection).map(|teams| Some(StoredTeams { generation, teams }))
        })
        .await
    }

    pub async fn create_team(&self, id: &str, name: &str) -> Result<TeamCreation> {
        let (id, name) = (id.to_string(), name.to_string());
        self.database.write(move |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            if transaction
                .query_optional("SELECT 1 FROM teams WHERE name = ?1", params![&name])?
                .is_some()
            {
                return Ok(TeamCreation::NameTaken);
            }
            transaction.execute(
                "INSERT INTO teams (team_id, name) VALUES (?1, ?2)",
                params![&id, &name],
            )?;
            advance_directory(&*transaction)?;
            transaction.commit()?;
            Ok(TeamCreation::Created)
        })
        .await
    }

    /// Remove the team and its memberships, unless the stored registry graph
    /// names it. A graph write that starts to name a team checks for it in
    /// its own transaction, so no stored graph names a removed team.
    pub async fn remove_team(&self, id: &str) -> Result<TeamRemoval> {
        let id = id.to_string();
        self.database.write(move |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            if !team_exists(&*transaction, &id)? {
                return Ok(TeamRemoval::NotFound);
            }
            if graph_names_team(&*transaction, &id)? {
                return Ok(TeamRemoval::Named);
            }
            transaction.execute("DELETE FROM team_members WHERE team_id = ?1", params![&id])?;
            transaction.execute("DELETE FROM teams WHERE team_id = ?1", params![&id])?;
            advance_directory(&*transaction)?;
            transaction.commit()?;
            Ok(TeamRemoval::Removed)
        })
        .await
    }

    /// Add the account to the team; adding a member again changes nothing.
    /// Reports whether the team exists.
    pub async fn add_team_member(&self, team_id: &str, user_id: &str) -> Result<bool> {
        let (team_id, user_id) = (team_id.to_string(), user_id.to_string());
        self.database.write(move |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            if !team_exists(&*transaction, &team_id)? {
                return Ok(false);
            }
            if transaction
                .query_optional(
                    "SELECT 1 FROM team_members WHERE team_id = ?1 AND user_id = ?2",
                    params![&team_id, &user_id],
                )?
                .is_none()
            {
                transaction.execute(
                    "INSERT INTO team_members (team_id, user_id) VALUES (?1, ?2)",
                    params![&team_id, &user_id],
                )?;
                advance_directory(&*transaction)?;
            }
            transaction.commit()?;
            Ok(true)
        })
        .await
    }

    /// Remove the account from the team; removing a non-member changes
    /// nothing. Reports whether the team exists.
    pub async fn remove_team_member(&self, team_id: &str, user_id: &str) -> Result<bool> {
        let (team_id, user_id) = (team_id.to_string(), user_id.to_string());
        self.database.write(move |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            if !team_exists(&*transaction, &team_id)? {
                return Ok(false);
            }
            remove_members(
                &transaction,
                "SELECT 1 FROM team_members WHERE team_id = ?1 AND user_id = ?2",
                "DELETE FROM team_members WHERE team_id = ?1 AND user_id = ?2",
                &[&team_id, &user_id],
            )?;
            transaction.commit()?;
            Ok(true)
        })
        .await
    }

    /// Remove the account from every team.
    pub async fn remove_team_memberships(&self, user_id: &str) -> Result<()> {
        let user_id = user_id.to_string();
        self.database.write(move |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            remove_members(
                &transaction,
                "SELECT 1 FROM team_members WHERE user_id = ?1 LIMIT 1",
                "DELETE FROM team_members WHERE user_id = ?1",
                &[&user_id],
            )?;
            transaction.commit()
        })
        .await
    }
}

/// Run `delete` and advance the directory when `select` finds a row to remove.
fn remove_members(
    transaction: &WriteTransaction<'_>,
    select: &str,
    delete: &str,
    keys: &[&String],
) -> Result<()> {
    let parameters: Vec<_> = keys
        .iter()
        .map(|key| super::adapter::Param::Text(key))
        .collect();
    if transaction.query_optional(select, &parameters)?.is_none() {
        return Ok(());
    }
    transaction.execute(delete, &parameters)?;
    advance_directory(&**transaction)
}

/// The first of `ids` that no team has.
pub(super) fn unknown_team(connection: &dyn Connection, ids: &[String]) -> Result<Option<String>> {
    for id in ids {
        if !team_exists(connection, id)? {
            return Ok(Some(id.clone()));
        }
    }
    Ok(None)
}

/// Whether the stored registry graph holds the access list entry `team:<id>`.
/// The document is JSON, so the entry is in it as that quoted string.
fn graph_names_team(connection: &dyn Connection, id: &str) -> Result<bool> {
    let entry = serde_json::to_string(&format!("team:{id}"))?;
    let document = connection.query_optional(
        "SELECT document FROM registry_graph_revisions ORDER BY generation DESC LIMIT 1",
        params![],
    )?;
    match document {
        Some(mut row) => Ok(row.get::<String>(0)?.contains(&entry)),
        None => Ok(false),
    }
}

fn team_exists(connection: &dyn Connection, id: &str) -> Result<bool> {
    Ok(connection
        .query_optional("SELECT 1 FROM teams WHERE team_id = ?1", params![id])?
        .is_some())
}

fn directory_generation(connection: &dyn Connection) -> Result<u64> {
    let mut row = connection
        .query_optional("SELECT generation FROM team_directory WHERE singleton = 1", params![])?
        .ok_or_else(|| RegistryError::Internal {
            reason: "the team directory generation is missing".to_string(),
        })?;
    Ok(to_generation(row.get(0)?))
}

fn advance_directory(connection: &dyn Connection) -> Result<()> {
    let generation = advance_record_generation(connection)?;
    connection.execute(
        "UPDATE team_directory SET generation = ?1 WHERE singleton = 1",
        params![generation],
    )
}

fn read_teams(connection: &dyn Connection) -> Result<Vec<StoredTeam>> {
    let mut teams = connection
        .query("SELECT team_id, name FROM teams ORDER BY name", params![])?
        .into_iter()
        .map(|mut row| Ok(StoredTeam { id: row.get(0)?, name: row.get(1)?, members: Vec::new() }))
        .collect::<Result<Vec<_>>>()?;
    let positions: HashMap<String, usize> = teams
        .iter()
        .enumerate()
        .map(|(position, team)| (team.id.clone(), position))
        .collect();
    let rows = connection.query(
        "SELECT team_id, user_id FROM team_members ORDER BY team_id, user_id",
        params![],
    )?;
    for mut row in rows {
        let (team_id, user_id): (String, String) = (row.get(0)?, row.get(1)?);
        if let Some(position) = positions.get(&team_id) {
            teams[*position].members.push(user_id);
        }
    }
    Ok(teams)
}
