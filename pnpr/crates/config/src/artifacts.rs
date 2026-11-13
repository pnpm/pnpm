//! The shared-artifact surface's settings: who may read and publish what
//! each artifact owner owns, and how much the artifact store keeps.

use super::{
    AccessList, AccessSpec, ArtifactQuotaFile, ByteSize, HostedConfig, IndexMap, RegistryError,
    StorageAccessFile, TeamDirectory, validate_artifact_owner_name,
};
use crate::byte_size::parse_positive_byte_size;

/// Toggle for the shared-artifact surface. Off by default while the
/// protocol is a proof of concept.
#[derive(Debug, Default, Clone)]
pub struct ArtifactsFeature {
    /// Master switch for artifact and compiler-cache endpoints.
    pub enabled: bool,
    /// Who may read and who may publish what each artifact owner owns: its
    /// signed artifacts, its compiler cache, and its pipeline run records. An
    /// undeclared owner is unavailable.
    pub owners: IndexMap<String, StorageAccess>,
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

const GIB: u64 = 1024 * 1024 * 1024;

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
    hosted: &IndexMap<String, HostedConfig>,
) -> Result<IndexMap<String, StorageAccess>, RegistryError> {
    policies
        .into_iter()
        .map(|(name, policy)| {
            validate_artifact_owner_name(&name)?;
            let parse = |field: &str, spec: &AccessSpec| {
                let list = spec
                    .to_access_list_resolving(|reference| artifact_team(reference, hosted))
                    .map_err(|reason| RegistryError::InvalidConfig {
                        reason: format!("artifact owner {name:?}: {reason}"),
                    })?;
                if list.references_owners() {
                    return Err(RegistryError::InvalidConfig {
                        reason: format!(
                            "artifact owner {name:?}: `$owners` is not valid for artifact `{field}` rules",
                        ),
                    });
                }
                Ok(list)
            };
            let access =
                StorageAccess { access: parse("access", &policy.access)?, publish: parse("publish", &policy.publish)? };
            Ok((name, access))
        })
        .collect()
}

pub(super) fn parse_artifact_quota(
    file: &ArtifactQuotaFile,
) -> Result<ArtifactQuota, RegistryError> {
    let default = ArtifactQuota::default();
    let bytes = |raw: Option<&ByteSize>, default: u64, field: &str| {
        raw.map_or(Ok(default), |raw| parse_positive_byte_size(field, raw))
    };
    let quota = ArtifactQuota {
        owner: bytes(file.per_owner.as_ref(), default.owner, "artifacts.quota.perOwner")?,
        total: bytes(file.total.as_ref(), default.total, "artifacts.quota.total")?,
    };
    if quota.owner > quota.total {
        return Err(RegistryError::InvalidConfig {
            reason: "artifacts.quota.perOwner exceeds artifacts.quota.total".to_string(),
        });
    }
    Ok(quota)
}

fn artifact_team(
    reference: &str,
    hosted: &IndexMap<String, HostedConfig>,
) -> Result<(String, TeamDirectory), String> {
    let (registry, team) = reference
        .rsplit_once(':')
        .ok_or_else(|| "artifact teams require team:<registry>:<teamId>".to_string())?;
    let config = hosted
        .get(registry)
        .ok_or_else(|| format!("{registry:?} is not a hosted registry"))?;
    Ok((team.to_string(), config.teams.clone()))
}
