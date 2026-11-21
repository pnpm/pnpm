use super::{
    SqlDatabase,
    batch::WriteTransaction,
    transactions::Freshness,
};
use pnpr_error::{
    RegistryError,
    Result,
};
use pnpr_sql::DialectSql;
use std::sync::Arc;

const ADD: DialectSql = DialectSql::with_mysql(
    "INSERT INTO npm_stars (namespace, name, user_id) VALUES (?1, ?2, ?3) ON CONFLICT DO NOTHING",
    "INSERT INTO npm_stars (namespace, name, user_id) VALUES (?1, ?2, ?3) ON DUPLICATE KEY UPDATE name = npm_stars.name",
);

impl SqlDatabase {
    /// Set the caller's star on an existing hosted npm package.
    pub async fn set_package_star(
        self: &Arc<Self>,
        namespace: String,
        name: String,
        user_id: String,
        starred: bool,
    ) -> Result<()> {
        self.write(move |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            let exists = transaction.query_optional(
                "SELECT 1 FROM packages WHERE namespace = ?1 AND ecosystem = 'npm' AND name = ?2",
                params![namespace, name],
            )?;
            if exists.is_none() {
                return Err(RegistryError::NotFound);
            }
            let sql = if starred {
                ADD.get(transaction.dialect())
            } else {
                "DELETE FROM npm_stars WHERE namespace = ?1 AND name = ?2 AND user_id = ?3"
            };
            transaction.execute(sql, params![namespace, name, user_id])?;
            transaction.commit()
        })
        .await
    }

    /// Hosted namespace and package name of each star recorded for the account.
    pub async fn account_stars(self: &Arc<Self>, user_id: String) -> Result<Vec<(String, String)>> {
        self.read(String::new(), Freshness::Current, move |connection, _| {
            connection
                .query(
                    "SELECT namespace, name FROM npm_stars WHERE user_id = ?1 ORDER BY name",
                    params![user_id],
                )?
                .into_iter()
                .map(|mut row| Ok((row.get(0)?, row.get(1)?)))
                .collect()
        })
        .await
    }
}
