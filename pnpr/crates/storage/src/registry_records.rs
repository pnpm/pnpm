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
        self.hosted.read_record(kind.namespace(), &registry_record_key(registry)?).await
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
        self.hosted.create_record(kind.namespace(), &key, bytes).await
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
        self.hosted.replace_record_if_current(kind.namespace(), &key, expected, bytes).await
    }

    pub async fn remove_registry_record(
        &self,
        kind: RegistryRecord,
        registry: &str,
    ) -> Result<bool> {
        self.hosted.remove_record(kind.namespace(), &registry_record_key(registry)?).await
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
