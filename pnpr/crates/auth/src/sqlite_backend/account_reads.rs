use crate::{AccountLogin, AccountStatus, AccountStatusRevision, UserAccount};
use pnpr_error::{RegistryError, Result};
use pnpr_sql::adapter::{Connection, Param, Row};
use std::collections::HashMap;

pub(super) fn list_accounts(connection: &dyn Connection) -> Result<Vec<UserAccount>> {
    collect_accounts(connection.query(
        "SELECT id, username FROM users WHERE status <> 'deleted' ORDER BY username",
        &[],
    )?)
}

pub(super) fn accounts_by_id(connection: &dyn Connection, ids: &str) -> Result<Vec<UserAccount>> {
    collect_accounts(connection.query(
        "SELECT id, username FROM users
         WHERE id IN (SELECT value FROM json_each(?1)) AND status <> 'deleted'",
        &[Param::Text(ids)],
    )?)
}

fn collect_accounts(rows: Vec<Row>) -> Result<Vec<UserAccount>> {
    rows.into_iter()
        .map(|mut row| Ok(UserAccount { id: row.get(0)?, username: row.get(1)? }))
        .collect()
}

pub(super) fn active_account_name(
    connection: &dyn Connection,
    user_id: &str,
) -> Result<Option<String>> {
    connection
        .query_optional(
            "SELECT username FROM users WHERE id = ?1 AND status = 'active'",
            &[Param::Text(user_id)],
        )?
        .map(|mut row| row.get(0))
        .transpose()
}

pub(super) fn account_status(
    connection: &dyn Connection,
    user_id: &str,
) -> Result<Option<AccountStatus>> {
    connection
        .query_optional("SELECT status FROM users WHERE id = ?1", &[Param::Text(user_id)])?
        .map(|mut row| parse_status(user_id, &row.get::<String>(0)?))
        .transpose()
}

pub(super) fn status_revisions(
    connection: &dyn Connection,
    ids: &str,
) -> Result<HashMap<String, AccountStatusRevision>> {
    let rows = connection.query(
        "SELECT id, status, status_revision FROM users
         WHERE id IN (SELECT value FROM json_each(?1))",
        &[Param::Text(ids)],
    )?;
    rows.into_iter()
        .map(|mut row| {
            let id: String = row.get(0)?;
            let status = parse_status(&id, &row.get::<String>(1)?)?;
            let revision = u64::try_from(row.get::<i64>(2)?).unwrap_or(0);
            Ok((id, AccountStatusRevision::reconciled(status, revision)))
        })
        .collect()
}

pub(super) fn account_login(
    connection: &dyn Connection,
    user_id: &str,
) -> Result<Option<AccountLogin>> {
    connection
        .query_optional(
            "SELECT username, name_revision FROM users WHERE id = ?1 AND status <> 'deleted'",
            &[Param::Text(user_id)],
        )?
        .map(|mut row| {
            let username = row.get(0)?;
            let revision = u64::try_from(row.get::<i64>(1)?).unwrap_or(0);
            Ok(AccountLogin { username, revision })
        })
        .transpose()
}

pub(super) fn parse_status(user_id: &str, raw: &str) -> Result<AccountStatus> {
    AccountStatus::parse(raw)
        .ok_or_else(|| RegistryError::Internal {
            reason: format!("user {user_id} has an unrecognized status {raw:?}"),
        })
}
