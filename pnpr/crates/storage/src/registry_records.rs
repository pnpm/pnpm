use super::{MetadataRecord, Result, Storage, validated_record_name};

/// An authoritative registry record in the metadata database.
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
    const fn as_str(self) -> &'static str {
        match self {
            RegistryRecord::TeamRoster => "team-roster",
            RegistryRecord::RuleOverrides => "rule-overrides",
            RegistryRecord::ScimUsers => "scim-users",
        }
    }
}

impl Storage {
    pub async fn read_registry_record(
        &self,
        kind: RegistryRecord,
        registry: &str,
    ) -> Result<Option<MetadataRecord>> {
        self.database.read_record(record_key(kind, registry)?).await
    }

    /// Write the first record of `kind` for `registry`, reporting `false`
    /// when another writer stored one first.
    pub async fn create_registry_record(
        &self,
        kind: RegistryRecord,
        registry: &str,
        bytes: &[u8],
    ) -> Result<bool> {
        Ok(self.database
            .write_record(record_key(kind, registry)?, None, bytes)
            .await?
            .is_some())
    }

    /// Replace the record of `kind` for `registry` only while it still holds
    /// `expected`.
    pub async fn replace_registry_record_if_current(
        &self,
        kind: RegistryRecord,
        registry: &str,
        expected: &MetadataRecord,
        bytes: &[u8],
    ) -> Result<Option<MetadataRecord>> {
        self.database.write_record(record_key(kind, registry)?, Some(expected.generation), bytes)
            .await
    }

    pub async fn remove_registry_record(
        &self,
        kind: RegistryRecord,
        registry: &str,
    ) -> Result<bool> {
        self.database.remove_record(record_key(kind, registry)?).await
    }
}

/// A registry's record key. A registry declared under an ecosystem group is
/// named `<ecosystem>/<name>`, and each segment becomes one path segment.
fn registry_record_key(registry: &str) -> Result<String> {
    let segments = registry
        .split('/')
        .map(validated_record_name)
        .collect::<Result<Vec<_>>>()?;
    Ok(segments.join("/"))
}

fn record_key(kind: RegistryRecord, registry: &str) -> Result<crate::metadata::sql::RecordKey> {
    Ok(crate::metadata::sql::RecordKey::Registry {
        kind: kind.as_str().to_string(),
        name: registry_record_key(registry)?,
    })
}
