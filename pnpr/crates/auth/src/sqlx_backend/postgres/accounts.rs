use super::{AuthSqlBackend, PostgresDatabase, RegistryError, Result, Row, is_unique_violation};
use crate::validation::name_revision_bounds;

impl PostgresDatabase {
    pub(super) async fn account_login_impl(
        &self,
        user_id: &str,
    ) -> Result<Option<crate::AccountLogin>> {
        let row = sqlx::query(
            "SELECT username, name_revision FROM users WHERE id = $1 AND bcrypt_hash <> '!'",
        )
        .bind(user_id)
        .fetch_optional(&mut *self.pool.acquire().await?)
        .await?;
        row.map(|row| -> std::result::Result<crate::AccountLogin, sqlx::Error> {
            let revision: i64 = row.try_get(1)?;
            Ok(crate::AccountLogin {
                username: row.try_get(0)?,
                revision: u64::try_from(revision).unwrap_or(0),
            })
        })
        .transpose()
        .map_err(RegistryError::from)
    }

    pub(super) async fn accounts_by_id_impl(
        &self,
        user_ids: &[String],
    ) -> Result<Vec<crate::UserAccount>> {
        let rows =
            sqlx::query("SELECT id, username FROM users WHERE id = ANY($1) AND bcrypt_hash <> '!'")
                .bind(user_ids)
                .fetch_all(&mut *self.pool.acquire().await?)
                .await?;
        rows.into_iter()
            .map(|row| Ok(crate::UserAccount { id: row.try_get(0)?, username: row.try_get(1)? }))
            .collect()
    }

    pub(super) async fn rename_account(&self, user_id: &str, username: &str) -> Result<()> {
        let result = sqlx::query(
            "UPDATE users
             SET username = $2, username_skeleton_hash = $3, name_revision = name_revision + 1
             WHERE id = $1 AND bcrypt_hash <> '!'",
        )
        .bind(user_id)
        .bind(username)
        .bind(crate::validation::username_skeleton_hash(username))
        .execute(&mut *self.pool.acquire().await?)
        .await;
        match result {
            Ok(result) if result.rows_affected() > 0 => Ok(()),
            Ok(_) => Err(RegistryError::NotFound),
            Err(err) if is_unique_violation(&err) => Err(crate::validation::username_conflict(
                username,
                self.stored_user(username).await?.is_some(),
            )),
            Err(err) => Err(err.into()),
        }
    }

    pub(super) async fn rename_account_if_current(
        &self,
        user_id: &str,
        expected_revision: u64,
        username: &str,
    ) -> Result<crate::UserRename> {
        let Some((expected, next, next_revision)) = name_revision_bounds(expected_revision)? else {
            self.account_name(user_id).await?.ok_or(RegistryError::NotFound)?;
            return Ok(crate::UserRename::CurrentChanged);
        };
        let result = sqlx::query(
            "UPDATE users
             SET username = $2, username_skeleton_hash = $3, name_revision = $5
             WHERE id = $1 AND name_revision = $4 AND bcrypt_hash <> '!'",
        )
        .bind(user_id)
        .bind(username)
        .bind(crate::validation::username_skeleton_hash(username))
        .bind(expected)
        .bind(next)
        .execute(&mut *self.pool.acquire().await?)
        .await;
        match result {
            Ok(result) if result.rows_affected() > 0 => {
                Ok(crate::UserRename::Renamed(next_revision))
            }
            Ok(_) => match self.account_name(user_id).await? {
                Some(_) => Ok(crate::UserRename::CurrentChanged),
                None => Err(RegistryError::NotFound),
            },
            Err(err) if is_unique_violation(&err) => Err(crate::validation::username_conflict(
                username,
                self.stored_user(username).await?.is_some(),
            )),
            Err(err) => Err(err.into()),
        }
    }
}
