use super::{MysqlDatabase, Result, Row};

impl MysqlDatabase {
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
            for row in query.build().fetch_all(&self.pool).await? {
                accounts.push(crate::UserAccount {
                    id: row.try_get(0)?,
                    username: row.try_get(1)?,
                });
            }
        }
        Ok(accounts)
    }
}
