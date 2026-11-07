use super::{StoredAccount, UserStore, UserStoreState, Users, persist_users_to_disk};
use crate::{RegistryError, Result};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};

/// How an import handles a login name with different credentials.
#[derive(Debug, Clone, Copy)]
pub enum PasswordCollisions {
    Keep,
    Overwrite,
}

/// Whether imported records are written or only checked.
#[derive(Debug, Clone, Copy)]
pub enum ImportMode {
    DryRun,
    Persist,
}

#[derive(Debug, Clone, Copy)]
pub struct PasswordImport {
    pub collisions: PasswordCollisions,
    pub mode: ImportMode,
}

impl UserStore {
    /// Import password hashes into native account records, preserving IDs on overwrite.
    pub fn import_password_hashes(
        path: &Path,
        hashes: HashMap<String, String>,
        options: PasswordImport,
    ) -> Result<(usize, Vec<String>)> {
        let _lock = match options.mode {
            ImportMode::DryRun => None,
            ImportMode::Persist => Some(crate::lock_htpasswd(path)?),
        };
        let state = Arc::new(Mutex::new(UserStoreState::default()));
        let mut users = Self::load_users_for_update(Some(path), &state)?;
        let result = merge_hashes(&mut users, hashes, options.collisions)?;
        match options.mode {
            ImportMode::DryRun => {}
            ImportMode::Persist => {
                persist_users_to_disk(Some(path), &users)?;
            }
        }
        Ok(result)
    }
}

fn merge_hashes(
    users: &mut Users,
    hashes: HashMap<String, String>,
    policy: PasswordCollisions,
) -> Result<(usize, Vec<String>)> {
    let mut merged = 0;
    let mut collisions = Vec::new();
    for (name, hash) in hashes {
        crate::validation::validate_username(&name)?;
        if !crate::htpasswd::is_supported_hash(&hash) {
            return Err(RegistryError::BadRequest {
                reason: format!("unsupported password hash for {name:?}"),
            });
        }
        match users.get_mut(&name) {
            Some(account) if account.password_hash == hash => {}
            Some(account) => match policy {
                PasswordCollisions::Keep => collisions.push(name),
                PasswordCollisions::Overwrite => {
                    account.password_hash = hash;
                    merged += 1;
                }
            },
            None => {
                users.insert(name, StoredAccount::new(hash));
                merged += 1;
            }
        }
    }
    Ok((merged, collisions))
}
