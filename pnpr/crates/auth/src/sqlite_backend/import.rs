use super::{
    SqliteAuth,
    accounts::{NameCheck, check_name, insert_account, set_password_hash, stored_account},
};
use pnpr_error::{RegistryError, Result};
use pnpr_sql::adapter::{Adapter as _, WriteTransaction};
use std::{collections::HashMap, sync::Arc};

/// How an import handles a login name with different credentials.
#[derive(Debug, Clone, Copy)]
pub enum PasswordCollisions {
    Keep,
    Overwrite,
}

#[derive(Debug, Clone, Copy)]
pub struct PasswordImport {
    pub collisions: PasswordCollisions,
    /// Check the import and report what it would do, writing nothing.
    pub dry_run: bool,
}

/// What an import did, or would do in a dry run.
#[derive(Debug, Default)]
pub struct ImportedPasswords {
    /// Accounts created or given the imported credential.
    pub merged: usize,
    /// Login names whose different credential was kept.
    pub collisions: Vec<String>,
}

impl SqliteAuth {
    /// Import `hashes`, keyed by login name, in one transaction. A new name
    /// becomes an account with a fresh ID. An existing account keeps its ID.
    pub async fn import_password_hashes(
        &self,
        hashes: HashMap<String, String>,
        options: PasswordImport,
    ) -> Result<ImportedPasswords> {
        for (name, hash) in &hashes {
            validate_imported_credentials(name, hash)?;
        }
        let database = Arc::clone(&self.database);
        tokio::task::spawn_blocking(move || {
            let mut imported = ImportedPasswords::default();
            database.write(&mut |connection| {
                imported = ImportedPasswords::default();
                let transaction = WriteTransaction::begin(connection)?;
                for (name, hash) in &hashes {
                    merge_hash(&*transaction, name, hash, options.collisions, &mut imported)?;
                }
                if !options.dry_run {
                    transaction.commit()?;
                }
                Ok(())
            })?;
            imported.collisions.sort();
            Ok(imported)
        })
        .await?
    }
}

fn merge_hash(
    connection: &dyn pnpr_sql::adapter::Connection,
    name: &str,
    hash: &str,
    collisions: PasswordCollisions,
    imported: &mut ImportedPasswords,
) -> Result<()> {
    if check_name(connection, name, None)? == NameCheck::Free {
        insert_account(connection, name, Some(hash))?;
        imported.merged += 1;
        return Ok(());
    }
    let Some(stored) = stored_account(connection, name)? else {
        return Err(RegistryError::Internal {
            reason: format!("account {name:?} vanished during an import"),
        });
    };
    if stored.credential.password_hash() == Some(hash) {
        return Ok(());
    }
    match collisions {
        PasswordCollisions::Keep => imported.collisions.push(name.to_string()),
        PasswordCollisions::Overwrite => {
            set_password_hash(connection, &stored.id, Some(hash))?;
            imported.merged += 1;
        }
    }
    Ok(())
}

fn validate_imported_credentials(name: &str, hash: &str) -> Result<()> {
    crate::validation::validate_username(name)?;
    if !crate::htpasswd::is_supported_hash(hash) {
        return Err(RegistryError::BadRequest {
            reason: format!("unsupported password hash for {name:?}"),
        });
    }
    Ok(())
}
