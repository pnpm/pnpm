use super::{DocumentWrite, Result, Storage, validated_record_name};

/// A kind of record the hosted store keeps per registry, one record per
/// registry under a reserved namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryRecord {
    /// The team roster of a registry whose teams the admin API manages.
    TeamRoster,
    /// The rule changes of a registry whose rules the admin API manages.
    RuleOverrides,
    /// The accounts a SCIM client provisioned, under the key
    /// [`SCIM_DIRECTORY`] rather than a registry name.
    ScimUsers,
}

/// The key of the one [`RegistryRecord::ScimUsers`] record.
pub const SCIM_DIRECTORY: &str = "directory";

impl RegistryRecord {
    const fn namespace(self) -> &'static str {
        match self {
            RegistryRecord::TeamRoster => ".team-rosters/v0",
            RegistryRecord::RuleOverrides => ".rule-overrides/v0",
            RegistryRecord::ScimUsers => ".scim-users/v0",
        }
    }
}

impl Storage {
    pub async fn read_registry_record(
        &self,
        kind: RegistryRecord,
        registry: &str,
    ) -> Result<Option<Vec<u8>>> {
        let key = registry_record_key(registry)?;
        if let Some(bytes) = self.root.read_record(kind.namespace(), &key).await? {
            return Ok(Some(bytes));
        }
        let local_name = registry.split_once('/').map_or(registry, |(_, name)| name);
        if local_name != registry {
            let legacy_key = registry_record_key(local_name)?;
            return self.root.read_record(kind.namespace(), &legacy_key).await;
        }
        Ok(None)
    }

    /// Write the first record of `kind` for `registry`, reporting `false`
    /// when another writer stored one first.
    pub async fn create_registry_record(
        &self,
        kind: RegistryRecord,
        registry: &str,
        bytes: &[u8],
    ) -> Result<bool> {
        let key = registry_record_key(registry)?;
        let local_name = registry.split_once('/').map_or(registry, |(_, name)| name);
        if local_name != registry {
            let legacy_key = registry_record_key(local_name)?;
            if self.root.read_record(kind.namespace(), &legacy_key).await?.is_some() {
                return Ok(false);
            }
        }
        if !self.root.create_record(kind.namespace(), &key, bytes).await? {
            return Ok(false);
        }
        if local_name != registry {
            let legacy_key = registry_record_key(local_name)?;
            if self.root.read_record(kind.namespace(), &legacy_key).await?.is_some() {
                self.root.remove_record_if_current(kind.namespace(), &key, bytes).await?;
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Replace the record of `kind` for `registry` only while it still holds
    /// `expected`.
    pub async fn replace_registry_record_if_current(
        &self,
        kind: RegistryRecord,
        registry: &str,
        expected: &[u8],
        bytes: &[u8],
    ) -> Result<DocumentWrite> {
        let key = registry_record_key(registry)?;
        if self.root.read_record(kind.namespace(), &key).await?.is_some() {
            return self.root.replace_record_if_current(kind.namespace(), &key, expected, bytes)
                .await;
        }
        let local_name = registry.split_once('/').map_or(registry, |(_, name)| name);
        if local_name == registry {
            return self.root.replace_record_if_current(kind.namespace(), &key, expected, bytes)
                .await;
        }
        let legacy_key = registry_record_key(local_name)?;
        if self.root.read_record(kind.namespace(), &legacy_key).await?.as_deref() != Some(expected)
        {
            return Ok(DocumentWrite::Conflict);
        }
        if !self.root.create_record(kind.namespace(), &key, bytes).await? {
            return self.root.replace_record_if_current(kind.namespace(), &key, expected, bytes)
                .await;
        }
        if self.root.read_record(kind.namespace(), &legacy_key).await?.as_deref() != Some(expected)
        {
            self.root.remove_record_if_current(kind.namespace(), &key, bytes).await?;
            return Ok(DocumentWrite::Conflict);
        }
        Ok(DocumentWrite::Written)
    }

    pub async fn remove_registry_record(
        &self,
        kind: RegistryRecord,
        registry: &str,
    ) -> Result<bool> {
        let key = registry_record_key(registry)?;
        let removed = self.root.remove_record(kind.namespace(), &key).await?;
        let local_name = registry.split_once('/').map_or(registry, |(_, name)| name);
        if local_name != registry {
            let legacy_key = registry_record_key(local_name)?;
            let legacy_removed = self.root.remove_record(kind.namespace(), &legacy_key).await?;
            return Ok(removed || legacy_removed);
        }
        Ok(removed)
    }
}

/// A registry's record key. A registry declared under an ecosystem group is
/// named `<ecosystem>/<name>`, and each segment becomes one path segment.
fn registry_record_key(registry: &str) -> Result<String> {
    let segments = registry
        .split('/')
        .map(validated_record_name)
        .collect::<Result<Vec<_>>>()?;
    Ok(format!("{}.json", segments.join("/")))
}
