//! Provisioned SCIM user resources and deprovision tombstones.

use super::{
    adapter::{Connection, Row},
    batch::WriteTransaction,
    records::advance_record_generation,
    to_column, to_generation,
    transactions::Freshness,
};
use crate::Storage;
use pnpr_error::{RegistryError, Result};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredScimUser {
    pub user_id: String,
    pub active: bool,
    pub deprovisions: u64,
    pub attributes: Map<String, Value>,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScimStatus {
    pub external_id: Option<String>,
    pub user_id: String,
    pub active: bool,
    pub deprovisions: u64,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewScimUser {
    pub user_id: String,
    pub active: bool,
    pub attributes: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScimUserWrite {
    Written(StoredScimUser),
    NotFound,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScimUserRemoval {
    Removed(ScimStatus),
    NotFound,
}

impl Storage {
    pub async fn read_scim_user(&self, user_id: &str) -> Result<Option<StoredScimUser>> {
        let user_id = user_id.to_string();
        self.database.read(String::new(), Freshness::Current, move |connection, _| {
            connection
                .query_optional(
                    "SELECT user_id, active, removed, deprovisions, attributes, generation
                         FROM scim_users WHERE user_id = ?1 AND removed = 0",
                    params![user_id],
                )?
                .map(user_from_row)
                .transpose()
        })
        .await
    }

    pub async fn list_active_scim_users(&self) -> Result<Vec<StoredScimUser>> {
        self.database.read(String::new(), Freshness::Current, |connection, _| {
            connection
                .query(
                    "SELECT user_id, active, removed, deprovisions, attributes, generation
                         FROM scim_users WHERE removed = 0 ORDER BY user_id",
                    params![],
                )?
                .into_iter()
                .map(user_from_row)
                .collect()
        })
        .await
    }

    pub async fn read_scim_statuses_since(&self, since_generation: u64) -> Result<Vec<ScimStatus>> {
        let since = to_column(since_generation);
        self.database.read(String::new(), Freshness::Current, move |connection, _| {
            connection
                .query(
                    "SELECT user_id, active, removed, deprovisions, generation, attributes
                         FROM scim_users WHERE generation > ?1 ORDER BY generation",
                    params![since],
                )?
                .into_iter()
                .map(status_from_row)
                .collect()
        })
        .await
    }

    pub async fn insert_scim_user(&self, user: NewScimUser) -> Result<Option<StoredScimUser>> {
        if user.user_id.is_empty() {
            return Err(RegistryError::BadRequest {
                reason: "SCIM user_id must not be empty".to_string(),
            });
        }
        let attributes_json = serde_json::to_string(&user.attributes)?;
        self.database.write(move |connection| insert_user(connection, &user, &attributes_json))
            .await
    }

    pub async fn replace_scim_user(
        &self,
        expected_generation: Option<u64>,
        user: NewScimUser,
    ) -> Result<ScimUserWrite> {
        let attributes_json = serde_json::to_string(&user.attributes)?;
        self.database.write(move |connection| {
            replace_user(connection, expected_generation, &user, &attributes_json)
        })
        .await
    }

    pub async fn remove_scim_user(&self, user_id: &str) -> Result<ScimUserRemoval> {
        let user_id = user_id.to_string();
        self.database.write(move |connection| remove_user(connection, &user_id)).await
    }
}

fn insert_user(
    connection: &dyn Connection,
    user: &NewScimUser,
    attributes_json: &str,
) -> Result<Option<StoredScimUser>> {
    let transaction = WriteTransaction::begin(connection)?;
    if transaction
        .query_optional("SELECT 1 FROM scim_users WHERE user_id = ?1", params![&user.user_id])?
        .is_some()
    {
        return Ok(None);
    }
    let generation = advance_record_generation(&*transaction)?;
    let deprovisions = u64::from(!user.active);
    transaction.execute(
        "INSERT INTO scim_users (user_id, active, removed, deprovisions, attributes, generation)
         VALUES (?1, ?2, 0, ?3, ?4, ?5)",
        params![
            &user.user_id,
            i64::from(user.active),
            to_column(deprovisions),
            attributes_json,
            generation,
        ],
    )?;
    transaction.commit()?;
    Ok(Some(StoredScimUser {
        user_id: user.user_id.clone(),
        active: user.active,
        deprovisions,
        attributes: user.attributes.clone(),
        generation: to_generation(generation),
    }))
}

fn replace_user(
    connection: &dyn Connection,
    expected_generation: Option<u64>,
    user: &NewScimUser,
    attributes_json: &str,
) -> Result<ScimUserWrite> {
    let transaction = WriteTransaction::begin(connection)?;
    let Some(existing) = existing_row(&*transaction, &user.user_id)? else {
        return Ok(ScimUserWrite::NotFound);
    };
    let deprovisions = match existing.lifecycle {
        ScimLifecycle::Removed => return Ok(ScimUserWrite::NotFound),
        ScimLifecycle::Active if !user.active => existing.deprovisions + 1,
        ScimLifecycle::Active | ScimLifecycle::Inactive => existing.deprovisions,
    };
    if expected_generation.is_some_and(|expected| expected != existing.generation) {
        return Ok(ScimUserWrite::Conflict);
    }
    let generation = advance_record_generation(&*transaction)?;
    transaction.execute(
        "UPDATE scim_users SET active = ?2, deprovisions = ?3, attributes = ?4, generation = ?5
         WHERE user_id = ?1",
        params![
            &user.user_id,
            i64::from(user.active),
            to_column(deprovisions),
            attributes_json,
            generation,
        ],
    )?;
    transaction.commit()?;
    Ok(ScimUserWrite::Written(StoredScimUser {
        user_id: user.user_id.clone(),
        active: user.active,
        deprovisions,
        attributes: user.attributes.clone(),
        generation: to_generation(generation),
    }))
}

fn remove_user(connection: &dyn Connection, user_id: &str) -> Result<ScimUserRemoval> {
    let transaction = WriteTransaction::begin(connection)?;
    let Some(existing) = existing_row(&*transaction, user_id)? else {
        return Ok(ScimUserRemoval::NotFound);
    };
    let deprovisions = match existing.lifecycle {
        ScimLifecycle::Removed => {
            return Ok(ScimUserRemoval::Removed(ScimStatus {
                external_id: None,
                user_id: user_id.to_string(),
                active: false,
                deprovisions: existing.deprovisions,
                generation: existing.generation,
            }));
        }
        ScimLifecycle::Active => existing.deprovisions + 1,
        ScimLifecycle::Inactive => existing.deprovisions,
    };
    let generation = advance_record_generation(&*transaction)?;
    transaction.execute(
        "UPDATE scim_users
         SET active = 0, removed = 1, deprovisions = ?2, attributes = '{}', generation = ?3
         WHERE user_id = ?1",
        params![user_id, to_column(deprovisions), generation],
    )?;
    transaction.commit()?;
    Ok(ScimUserRemoval::Removed(ScimStatus {
        external_id: None,
        user_id: user_id.to_string(),
        active: false,
        deprovisions,
        generation: to_generation(generation),
    }))
}

#[derive(Clone, Copy)]
enum ScimLifecycle {
    Active,
    Inactive,
    Removed,
}

struct DecodedStatusRow {
    user_id: String,
    lifecycle: ScimLifecycle,
    deprovisions: u64,
    generation: u64,
}

fn existing_row(connection: &dyn Connection, user_id: &str) -> Result<Option<DecodedStatusRow>> {
    connection
        .query_optional(
            "SELECT user_id, active, removed, deprovisions, generation
             FROM scim_users WHERE user_id = ?1",
            params![user_id],
        )?
        .map(|mut row| decode_status_columns(&mut row))
        .transpose()
}

fn user_from_row(mut row: Row) -> Result<StoredScimUser> {
    let user_id: String = row.get(0)?;
    let active = match decode_lifecycle(row.get(1)?, row.get(2)?)? {
        ScimLifecycle::Active => true,
        ScimLifecycle::Inactive => false,
        ScimLifecycle::Removed => return Err(corrupt("invalid SCIM user flags")),
    };
    if user_id.is_empty() {
        return Err(corrupt("invalid SCIM user flags"));
    }
    let deprovisions = decode_non_negative("deprovisions", row.get(3)?)?;
    let attributes_raw: String = row.get(4)?;
    let generation = decode_positive("generation", row.get(5)?)?;
    let attributes = decode_attributes(&user_id, &attributes_raw)?;
    Ok(StoredScimUser { user_id, active, deprovisions, attributes, generation })
}

fn status_from_row(mut row: Row) -> Result<ScimStatus> {
    let DecodedStatusRow {
        user_id,
        lifecycle,
        deprovisions,
        generation,
    } = decode_status_columns(&mut row)?;
    let active = match lifecycle {
        ScimLifecycle::Active => true,
        ScimLifecycle::Inactive | ScimLifecycle::Removed => false,
    };
    let attributes_raw: String = row.get(5)?;
    let attributes = decode_attributes(&user_id, &attributes_raw)?;
    let external_id = attributes
        .get("externalId")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok(ScimStatus { external_id, user_id, active, deprovisions, generation })
}

fn decode_status_columns(row: &mut Row) -> Result<DecodedStatusRow> {
    let user_id: String = row.get(0)?;
    if user_id.is_empty() {
        return Err(corrupt("invalid SCIM user row"));
    }
    let lifecycle = decode_lifecycle(row.get(1)?, row.get(2)?)?;
    let deprovisions = decode_non_negative("deprovisions", row.get(3)?)?;
    let generation = decode_positive("generation", row.get(4)?)?;
    Ok(DecodedStatusRow { user_id, lifecycle, deprovisions, generation })
}

fn decode_lifecycle(active_raw: i64, removed_raw: i64) -> Result<ScimLifecycle> {
    let active = decode_flag("active", active_raw)?;
    let removed = decode_flag("removed", removed_raw)?;
    match (active, removed) {
        (true, false) => Ok(ScimLifecycle::Active),
        (false, false) => Ok(ScimLifecycle::Inactive),
        (false, true) => Ok(ScimLifecycle::Removed),
        (true, true) => Err(corrupt("invalid SCIM user flags")),
    }
}

fn decode_flag(field: &str, value: i64) -> Result<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(corrupt(&format!("invalid SCIM {field} value {value}"))),
    }
}

fn decode_non_negative(field: &str, value: i64) -> Result<u64> {
    u64::try_from(value).map_err(|_| corrupt(&format!("invalid SCIM {field} value {value}")))
}

fn decode_positive(field: &str, value: i64) -> Result<u64> {
    if value < 1 {
        return Err(corrupt(&format!("invalid SCIM {field} value {value}")));
    }
    Ok(to_generation(value))
}

fn corrupt(detail: &str) -> RegistryError {
    RegistryError::Internal { reason: format!("the stored SCIM directory is unreadable: {detail}") }
}

fn decode_attributes(user_id: &str, raw: &str) -> Result<Map<String, Value>> {
    serde_json::from_str(raw)
        .map_err(|err| RegistryError::Internal {
            reason: format!("the stored SCIM user {user_id:?} is unreadable: {err}"),
        })
}
