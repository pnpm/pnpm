//! The shared-artifact surface's settings: who may read and publish what
//! each organization owns, and how much the artifact store keeps.

use super::{
    AccessList, AccessSpec, ArtifactQuotaFile, IndexMap, RegistryError, StorageAccessFile,
    TeamDirectory, validate_registry_name,
};

/// Toggle for the shared-artifact surface. Off by default while the
/// protocol is a proof of concept.
#[derive(Debug, Default, Clone)]
pub struct ArtifactsFeature {
    /// Master switch for artifact and compiler-cache endpoints.
    pub enabled: bool,
    /// Who may read and who may publish what each organization owns: its
    /// signed artifacts, its compiler cache, and its pipeline run records. An
    /// undeclared organization is unavailable.
    pub orgs: IndexMap<String, StorageAccess>,
    pub quota: ArtifactQuota,
}

/// How many bytes of signed artifacts, their blobs, and compiler cache
/// entries the store keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtifactQuota {
    /// One owner: an organization, or the publisher of a package. Defaults
    /// to 1 GiB.
    pub owner: u64,
    /// Every owner together. Defaults to 10 GiB.
    pub total: u64,
}

pub(super) const GIB: u64 = 1024 * 1024 * 1024;

impl Default for ArtifactQuota {
    fn default() -> Self {
        Self { owner: GIB, total: 10 * GIB }
    }
}

#[derive(Debug, Clone)]
pub struct StorageAccess {
    pub access: AccessList,
    pub publish: AccessList,
}

pub(super) fn parse_storage_access(
    policies: IndexMap<String, StorageAccessFile>,
) -> Result<IndexMap<String, StorageAccess>, RegistryError> {
    policies
        .into_iter()
        .map(|(name, policy)| {
            validate_registry_name(&name)?;
            let parse = |spec: &AccessSpec| {
                spec.to_access_list(&TeamDirectory::default())
                    .map_err(|reason| RegistryError::InvalidConfig {
                        reason: format!("organization {name:?}: {reason}"),
                    })
            };
            let access =
                StorageAccess { access: parse(&policy.access)?, publish: parse(&policy.publish)? };
            Ok((name, access))
        })
        .collect()
}

pub(super) fn parse_artifact_quota(
    file: &ArtifactQuotaFile,
) -> Result<ArtifactQuota, RegistryError> {
    let default = ArtifactQuota::default();
    let bytes = |gib: Option<u64>, default: u64, field: &str| match gib {
        None => Ok(default),
        Some(gib) => gib
            .checked_mul(GIB)
            .filter(|bytes| *bytes > 0)
            .ok_or_else(|| RegistryError::InvalidConfig {
                reason: format!("artifacts.quota.{field} must be a positive number of GiB"),
            }),
    };
    let quota = ArtifactQuota {
        owner: bytes(file.owner_gib, default.owner, "ownerGiB")?,
        total: bytes(file.total_gib, default.total, "totalGiB")?,
    };
    if quota.owner > quota.total {
        return Err(RegistryError::InvalidConfig {
            reason: "artifacts.quota.ownerGiB exceeds artifacts.quota.totalGiB".to_string(),
        });
    }
    Ok(quota)
}
