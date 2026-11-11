use super::{
    AuthSqlBackend, MysqlDatabase, RegistryError, Result, Row, is_unique_violation,
    username_from_row,
};
use crate::validation::name_revision_bounds;

impl MysqlDatabase {
    pub(super) async fn account_login_impl(
        &self,
        user_id: &str,
    ) -> Result<Option<crate::AccountLogin>> {
        let row = sqlx::query(
            "SELECT username, name_revision FROM users WHERE id = ? AND bcrypt_hash <> '!'",
        )
        .bind(user_id)
        .fetch_optional(&mut *self.pool.acquire().await?)
        .await?;
        row.map(|row| -> std::result::Result<crate::AccountLogin, sqlx::Error> {
            let revision: i64 = row.try_get(1)?;
            Ok(crate::AccountLogin {
                username: username_from_row(&row, 0)?,
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
        let mut accounts = Vec::new();
        for ids in user_ids.chunks(512) {
            let mut query = sqlx::QueryBuilder::<sqlx::MySql>::new(
                "SELECT id, username FROM users WHERE id IN (",
            );
            let mut binds = query.separated(", ");
            for id in ids {
                binds.push_bind(id);
            }
            binds.push_unseparated(") AND bcrypt_hash <> '!'");
            for row in query
                .build()
                .fetch_all(&mut *self.pool.acquire().await?)
                .await?
            {
                accounts.push(crate::UserAccount {
                    id: row.try_get(0)?,
                    username: username_from_row(&row, 1)?,
                });
            }
        }
        Ok(accounts)
    }

    pub(super) async fn rename_account(&self, user_id: &str, username: &str) -> Result<()> {
        let result = sqlx::query(
            "UPDATE users
             SET username = ?, username_skeleton_hash = ?, name_revision = name_revision + 1
             WHERE id = ? AND bcrypt_hash <> '!'",
        )
        .bind(username)
        .bind(crate::validation::username_skeleton_hash(username))
        .bind(user_id)
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
             SET username = ?, username_skeleton_hash = ?, name_revision = ?
             WHERE id = ? AND name_revision = ? AND bcrypt_hash <> '!'",
        )
        .bind(username)
        .bind(crate::validation::username_skeleton_hash(username))
        .bind(next)
        .bind(user_id)
        .bind(expected)
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
