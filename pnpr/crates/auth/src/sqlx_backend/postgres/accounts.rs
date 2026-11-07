use super::{PostgresDatabase, Result, Row};

impl PostgresDatabase {
    pub(super) async fn accounts_by_id_impl(
        &self,
        user_ids: &[String],
    ) -> Result<Vec<crate::UserAccount>> {
        let rows =
            sqlx::query("SELECT id, username FROM users WHERE id = ANY($1) AND bcrypt_hash <> '!'")
                .bind(user_ids)
                .fetch_all(&self.pool)
                .await?;
        rows.into_iter()
            .map(|row| Ok(crate::UserAccount { id: row.try_get(0)?, username: row.try_get(1)? }))
            .collect()
    }
}
