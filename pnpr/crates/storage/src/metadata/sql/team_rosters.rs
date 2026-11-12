//! Normalized team rosters (`team_rosters`, `teams`, `team_members`).

use super::{
    adapter::{Connection, Row},
    batch::WriteTransaction,
    cdn,
    records::advance_record_generation,
    to_column, to_generation,
    transactions::Freshness,
};
use crate::{Storage, validated_record_name};
use pnpr_error::{RegistryError, Result};
use pnpr_sql::DialectSql;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredTeam {
    pub id: String,
    pub name: String,
    pub members: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredTeamRoster {
    pub generation: u64,
    pub teams: Vec<StoredTeam>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeamRosterWrite {
    Written(StoredTeamRoster),
    NotFound,
    Conflict,
}

pub(super) fn validated_registry_key(registry: &str) -> Result<String> {
    let segments = registry
        .split('/')
        .map(validated_record_name)
        .collect::<Result<Vec<_>>>()?;
    Ok(segments.join("/"))
}

impl Storage {
    pub async fn read_team_roster_generation(&self, registry: &str) -> Result<Option<u64>> {
        let registry = validated_registry_key(registry)?;
        self.database.read(String::new(), Freshness::Current, move |connection, _| {
            roster_generation(connection, &registry)
        })
        .await
    }

    pub async fn read_team_roster(&self, registry: &str) -> Result<Option<StoredTeamRoster>> {
        let registry = validated_registry_key(registry)?;
        self.database.read(String::new(), Freshness::Current, move |connection, _| {
            read_roster(connection, &registry)
        })
        .await
    }

    pub async fn replace_team_roster(
        &self,
        registry: &str,
        teams: &[StoredTeam],
    ) -> Result<StoredTeamRoster> {
        let registry = validated_registry_key(registry)?;
        let teams = teams.to_vec();
        self.database.write(move |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            let generation = write_all_teams(&*transaction, &registry, &teams)?;
            cdn::enqueue(&*transaction, "*")?;
            transaction.commit()?;
            Ok(StoredTeamRoster { generation, teams: teams.clone() })
        })
        .await
    }

    pub async fn remove_team_roster(&self, registry: &str) -> Result<bool> {
        let registry = validated_registry_key(registry)?;
        self.database.write(move |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            let existed = roster_generation(&*transaction, &registry)?.is_some();
            transaction.execute(
                "DELETE FROM team_members WHERE registry = ?1",
                params![&registry],
            )?;
            transaction.execute("DELETE FROM teams WHERE registry = ?1", params![&registry])?;
            transaction.execute(
                "DELETE FROM team_rosters WHERE registry = ?1",
                params![&registry],
            )?;
            if existed {
                cdn::enqueue(&*transaction, "*")?;
            }
            transaction.commit()?;
            Ok(existed)
        })
        .await
    }

    pub async fn create_team_in_roster(
        &self,
        registry: &str,
        seed: &[StoredTeam],
        team_id: &str,
        name: &str,
    ) -> Result<TeamRosterWrite> {
        let registry = validated_registry_key(registry)?;
        let (seed, team_id, name) = (seed.to_vec(), team_id.to_string(), name.to_string());
        self.database.write(move |connection| {
            create_team(connection, &registry, &seed, &team_id, &name)
        })
        .await
    }

    pub async fn destroy_team_in_roster<Check>(
        &self,
        registry: &str,
        seed: &[StoredTeam],
        name: &str,
        verify_removable: Check,
    ) -> Result<TeamRosterWrite>
    where
        Check: Fn(&str) -> Result<()> + Send + Sync + 'static,
    {
        let registry = validated_registry_key(registry)?;
        let (seed, name) = (seed.to_vec(), name.to_string());
        self.database.write(move |connection| {
            destroy_team(connection, &registry, &seed, &name, &verify_removable)
        })
        .await
    }

    pub async fn add_member_to_team(
        &self,
        registry: &str,
        seed: &[StoredTeam],
        name: &str,
        user_id: &str,
    ) -> Result<TeamRosterWrite> {
        let registry = validated_registry_key(registry)?;
        let (seed, name, user_id) = (seed.to_vec(), name.to_string(), user_id.to_string());
        self.database.write(move |connection| {
            add_member(connection, &registry, &seed, &name, &user_id)
        })
        .await
    }

    pub async fn remove_member_from_team(
        &self,
        registry: &str,
        seed: &[StoredTeam],
        name: &str,
        user_id: &str,
    ) -> Result<TeamRosterWrite> {
        let registry = validated_registry_key(registry)?;
        let (seed, name, user_id) = (seed.to_vec(), name.to_string(), user_id.to_string());
        self.database.write(move |connection| {
            remove_member(connection, &registry, &seed, &name, &user_id)
        })
        .await
    }
}

fn roster_generation(connection: &dyn Connection, registry: &str) -> Result<Option<u64>> {
    connection
        .query_optional(
            "SELECT generation FROM team_rosters WHERE registry = ?1",
            params![registry],
        )?
        .map(|mut row| decode_roster_generation(registry, row.get(0)?))
        .transpose()
}

fn read_roster(connection: &dyn Connection, registry: &str) -> Result<Option<StoredTeamRoster>> {
    let queries = [
        query!("SELECT generation FROM team_rosters WHERE registry = ?1", registry),
        query!(
            "SELECT team_id, name FROM teams WHERE registry = ?1 ORDER BY position, team_id",
            registry,
        ),
        query!(
            "SELECT team_id, user_id FROM team_members WHERE registry = ?1 ORDER BY team_id, user_id",
            registry,
        ),
    ];
    let mut results = connection.query_all(&queries)?.into_iter();
    let head = results.next().unwrap_or_default();
    let team_rows = results.next().unwrap_or_default();
    let member_rows = results.next().unwrap_or_default();
    let Some(mut head_row) = head.into_iter().next() else {
        if !team_rows.is_empty() || !member_rows.is_empty() {
            return Err(corrupt(registry, "orphaned team rows without team_rosters header"));
        }
        return Ok(None);
    };
    let generation = decode_roster_generation(registry, head_row.get(0)?)?;
    let teams = assemble_teams(registry, team_rows, member_rows)?;
    Ok(Some(StoredTeamRoster { generation, teams }))
}

fn assemble_teams(
    registry: &str,
    team_rows: Vec<Row>,
    member_rows: Vec<Row>,
) -> Result<Vec<StoredTeam>> {
    let mut teams = Vec::with_capacity(team_rows.len());
    let mut index_by_id = BTreeMap::new();
    for mut row in team_rows {
        let id: String = row.get(0)?;
        let name: String = row.get(1)?;
        if id.is_empty() || name.is_empty() {
            return Err(corrupt(registry, "empty team identifier or name"));
        }
        index_by_id.insert(id.clone(), teams.len());
        teams.push(StoredTeam { id, name, members: BTreeSet::new() });
    }
    for mut row in member_rows {
        let team_id: String = row.get(0)?;
        let user_id: String = row.get(1)?;
        if user_id.is_empty() {
            return Err(corrupt(registry, "empty team member user_id"));
        }
        let Some(&index) = index_by_id.get(&team_id) else {
            return Err(corrupt(registry, &format!("member references unknown team {team_id:?}")));
        };
        teams[index].members.insert(user_id);
    }
    Ok(teams)
}

fn ensure_seeded(connection: &dyn Connection, registry: &str, seed: &[StoredTeam]) -> Result<()> {
    if roster_generation(connection, registry)?.is_some() {
        return Ok(());
    }
    write_all_teams(connection, registry, seed)?;
    Ok(())
}

const WRITE_ROSTER: DialectSql = DialectSql::with_mysql(
    "INSERT INTO team_rosters (registry, generation) VALUES (?1, ?2)
     ON CONFLICT (registry) DO UPDATE SET generation = excluded.generation",
    "INSERT INTO team_rosters (registry, generation) VALUES (?1, ?2)
     AS excluded ON DUPLICATE KEY UPDATE generation = excluded.generation",
);

fn write_all_teams(
    connection: &dyn Connection,
    registry: &str,
    teams: &[StoredTeam],
) -> Result<u64> {
    connection.execute("DELETE FROM team_members WHERE registry = ?1", params![registry])?;
    connection.execute("DELETE FROM teams WHERE registry = ?1", params![registry])?;
    let generation = advance_record_generation(connection)?;
    connection.execute(WRITE_ROSTER.get(connection.dialect()), params![registry, generation])?;
    for (position, team) in teams.iter().enumerate() {
        connection.execute(
            "INSERT INTO teams (registry, team_id, name, position) VALUES (?1, ?2, ?3, ?4)",
            params![registry, &team.id, &team.name, to_column(position as u64)],
        )?;
        for member in &team.members {
            connection.execute(
                "INSERT INTO team_members (registry, team_id, user_id) VALUES (?1, ?2, ?3)",
                params![registry, &team.id, member],
            )?;
        }
    }
    Ok(to_generation(generation))
}

fn bump_roster_generation(connection: &dyn Connection, registry: &str) -> Result<u64> {
    let generation = advance_record_generation(connection)?;
    connection.execute(
        "UPDATE team_rosters SET generation = ?2 WHERE registry = ?1",
        params![registry, generation],
    )?;
    Ok(to_generation(generation))
}

fn create_team(
    connection: &dyn Connection,
    registry: &str,
    seed: &[StoredTeam],
    team_id: &str,
    name: &str,
) -> Result<TeamRosterWrite> {
    let transaction = WriteTransaction::begin(connection)?;
    ensure_seeded(&*transaction, registry, seed)?;
    if find_team_id(&*transaction, registry, name)?.is_some() {
        return Ok(TeamRosterWrite::Conflict);
    }
    let next_position: i64 = transaction
        .query_one(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM teams WHERE registry = ?1",
            params![registry],
        )?
        .get(0)?;
    transaction.execute(
        "INSERT INTO teams (registry, team_id, name, position) VALUES (?1, ?2, ?3, ?4)",
        params![registry, team_id, name, next_position],
    )?;
    bump_roster_generation(&*transaction, registry)?;
    cdn::enqueue(&*transaction, "*")?;
    let roster = committed_roster(&*transaction, registry)?;
    transaction.commit()?;
    Ok(TeamRosterWrite::Written(roster))
}

fn destroy_team<Check>(
    connection: &dyn Connection,
    registry: &str,
    seed: &[StoredTeam],
    name: &str,
    verify_removable: &Check,
) -> Result<TeamRosterWrite>
where
    Check: Fn(&str) -> Result<()>,
{
    let transaction = WriteTransaction::begin(connection)?;
    ensure_seeded(&*transaction, registry, seed)?;
    let Some(team_id) = find_team_id(&*transaction, registry, name)? else {
        return Ok(TeamRosterWrite::NotFound);
    };
    verify_removable(&team_id)?;
    transaction.execute(
        "DELETE FROM team_members WHERE registry = ?1 AND team_id = ?2",
        params![registry, &team_id],
    )?;
    transaction.execute(
        "DELETE FROM teams WHERE registry = ?1 AND team_id = ?2",
        params![registry, &team_id],
    )?;
    bump_roster_generation(&*transaction, registry)?;
    cdn::enqueue(&*transaction, "*")?;
    let roster = committed_roster(&*transaction, registry)?;
    transaction.commit()?;
    Ok(TeamRosterWrite::Written(roster))
}

fn add_member(
    connection: &dyn Connection,
    registry: &str,
    seed: &[StoredTeam],
    name: &str,
    user_id: &str,
) -> Result<TeamRosterWrite> {
    let transaction = WriteTransaction::begin(connection)?;
    ensure_seeded(&*transaction, registry, seed)?;
    let Some(team_id) = find_team_id(&*transaction, registry, name)? else {
        return Ok(TeamRosterWrite::NotFound);
    };
    if !member_exists(&*transaction, registry, &team_id, user_id)? {
        transaction.execute(
            "INSERT INTO team_members (registry, team_id, user_id) VALUES (?1, ?2, ?3)",
            params![registry, &team_id, user_id],
        )?;
        bump_roster_generation(&*transaction, registry)?;
        cdn::enqueue(&*transaction, "*")?;
    }
    let roster = committed_roster(&*transaction, registry)?;
    transaction.commit()?;
    Ok(TeamRosterWrite::Written(roster))
}

fn remove_member(
    connection: &dyn Connection,
    registry: &str,
    seed: &[StoredTeam],
    name: &str,
    user_id: &str,
) -> Result<TeamRosterWrite> {
    let transaction = WriteTransaction::begin(connection)?;
    ensure_seeded(&*transaction, registry, seed)?;
    let Some(team_id) = find_team_id(&*transaction, registry, name)? else {
        return Ok(TeamRosterWrite::NotFound);
    };
    if member_exists(&*transaction, registry, &team_id, user_id)? {
        transaction.execute(
            "DELETE FROM team_members WHERE registry = ?1 AND team_id = ?2 AND user_id = ?3",
            params![registry, &team_id, user_id],
        )?;
        bump_roster_generation(&*transaction, registry)?;
        cdn::enqueue(&*transaction, "*")?;
    }
    let roster = committed_roster(&*transaction, registry)?;
    transaction.commit()?;
    Ok(TeamRosterWrite::Written(roster))
}

fn member_exists(
    connection: &dyn Connection,
    registry: &str,
    team_id: &str,
    user_id: &str,
) -> Result<bool> {
    Ok(connection
        .query_optional(
            "SELECT 1 FROM team_members WHERE registry = ?1 AND team_id = ?2 AND user_id = ?3",
            params![registry, team_id, user_id],
        )?
        .is_some())
}

fn find_team_id(connection: &dyn Connection, registry: &str, name: &str) -> Result<Option<String>> {
    connection
        .query_optional(
            "SELECT team_id FROM teams WHERE registry = ?1 AND name = ?2",
            params![registry, name],
        )?
        .map(|mut row| row.get(0))
        .transpose()
}

fn committed_roster(connection: &dyn Connection, registry: &str) -> Result<StoredTeamRoster> {
    read_roster(connection, registry)?.ok_or_else(|| corrupt(registry, "missing committed roster"))
}

fn decode_roster_generation(registry: &str, value: i64) -> Result<u64> {
    if value < 1 {
        return Err(corrupt(registry, &format!("invalid generation {value}")));
    }
    Ok(to_generation(value))
}

fn corrupt(registry: &str, detail: &str) -> RegistryError {
    RegistryError::Internal {
        reason: format!("the stored teams of registry {registry:?} are unreadable: {detail}"),
    }
}
