use super::{
    AccountStatus, LibsqlAuth, RegistryError, Result, TransactionBehavior, first_row, params,
    retry_database_conflicts,
};
use crate::{
    AccountLifecycle, AccountStatusOutcome, AccountStatusRevision, account::should_apply_lifecycle,
    validation::status_revision_to_sql,
};
use std::collections::HashMap;

impl LibsqlAuth {
    pub(super) async fn status_revisions_by_id(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, AccountStatusRevision>> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut revisions = HashMap::with_capacity(user_ids.len());
        for chunk in user_ids.chunks(512) {
            let ids = serde_json::to_string(chunk)?;
            let ids = ids.as_str();
            let batch = self.pool.read(|conn| async move {
                let rows = conn.query(
                    "SELECT id, status, status_revision
                             FROM users
                             WHERE id IN (SELECT value FROM json_each(?1))",
                    params![ids],
                )
                .await?;
                collect_status_revisions(rows).await
            })
            .await?;
            revisions.extend(batch);
        }
        Ok(revisions)
    }

    pub(super) async fn apply_account_status_by_id(
        &self,
        user_id: &str,
        revision: u64,
        lifecycle: AccountLifecycle,
    ) -> Result<AccountStatusOutcome> {
        let sql_revision = status_revision_to_sql(revision)?;
        let outcome = retry_database_conflicts(|| {
            self.pool.write(|conn| async move {
                execute_lifecycle_transition(&conn, user_id, revision, sql_revision, lifecycle)
                    .await
            })
        })
        .await?;
        self.sync_replica().await?;
        Ok(outcome)
    }
}

async fn collect_status_revisions(
    mut rows: libsql::Rows,
) -> Result<HashMap<String, AccountStatusRevision>> {
    let mut revisions = HashMap::new();
    while let Some(row) = rows.next().await? {
        let id: String = row.get(0)?;
        let raw_status: String = row.get(1)?;
        let status = AccountStatus::parse(&raw_status)
            .ok_or_else(|| RegistryError::Internal {
                reason: format!("user {id} has an unrecognized status {raw_status:?}"),
            })?;
        let revision = u64::try_from(row.get::<i64>(2)?).unwrap_or(0);
        revisions.insert(id, AccountStatusRevision::reconciled(status, revision));
    }
    Ok(revisions)
}

async fn execute_lifecycle_transition(
    conn: &libsql::Connection,
    user_id: &str,
    revision: u64,
    sql_revision: i64,
    lifecycle: AccountLifecycle,
) -> Result<AccountStatusOutcome> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).await?;
    let rows = tx.query(
        "SELECT status, status_revision FROM users WHERE id = ?1 AND status <> 'deleted'",
        params![user_id],
    )
    .await?;
    let Some((raw_status, raw_rev)) =
        first_row(rows, |row| Ok((row.get::<String>(0)?, row.get::<i64>(1)?))).await?
    else {
        tx.rollback().await?;
        return Ok(AccountStatusOutcome::Missing);
    };
    let actual_status = AccountStatus::parse(&raw_status)
        .ok_or_else(|| RegistryError::Internal {
            reason: format!("user {user_id} has an unrecognized status {raw_status:?}"),
        })?;
    let actual_rev = u64::try_from(raw_rev).unwrap_or(0);
    if !should_apply_lifecycle(actual_status, actual_rev, revision, lifecycle) {
        tx.rollback().await?;
        return Ok(AccountStatusOutcome::Superseded);
    }
    apply_primary_lifecycle(tx, user_id, sql_revision, lifecycle).await
}

async fn apply_primary_lifecycle(
    tx: libsql::Transaction,
    user_id: &str,
    sql_revision: i64,
    lifecycle: AccountLifecycle,
) -> Result<AccountStatusOutcome> {
    match lifecycle {
        AccountLifecycle::Active => {
            tx.execute(
                "UPDATE users SET status = 'active', status_revision = ?2
                 WHERE id = ?1 AND status <> 'deleted'",
                params![user_id, sql_revision],
            )
            .await?;
        }
        AccountLifecycle::Deactivated => {
            tx.execute(
                "UPDATE users
                 SET status = 'deactivated',
                     credential_kind = 'none',
                     password_hash = NULL,
                     status_revision = ?2
                 WHERE id = ?1 AND status <> 'deleted'",
                params![user_id, sql_revision],
            )
            .await?;
            tx.execute("DELETE FROM tokens WHERE user_id = ?1", params![user_id]).await?;
        }
    }
    tx.commit().await?;
    Ok(AccountStatusOutcome::Applied)
}
