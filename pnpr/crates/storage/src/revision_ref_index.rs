use super::{
    Deserialize, HashSet, MAX_HOSTED_REVISION_REFS, RegistryError, Result, Serialize,
    integrity_addressed_tarball_integrity,
};

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct HostedRevisionRefIndex {
    pub(super) refs: Vec<HostedRevisionRefIndexEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct HostedRevisionRefIndexEntry {
    pub(super) id: String,
    pub(super) committed: bool,
    pub(super) pending_owners: Vec<String>,
    pub(super) bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostedRevisionRefWrite {
    Claimed,
    AlreadyClaimed,
    Committed,
}

impl HostedRevisionRefIndex {
    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let index: Self = serde_json::from_slice(bytes)?;
        if index.refs.len() > MAX_HOSTED_REVISION_REFS {
            return Err(RegistryError::RevisionReferenceLimit { limit: MAX_HOSTED_REVISION_REFS });
        }
        let mut seen = HashSet::with_capacity(index.refs.len());
        if index.refs.iter().any(|entry| {
            !is_canonical_revision_ref_id(&entry.id)
                || (entry.committed && !entry.pending_owners.is_empty())
                || (!entry.committed && entry.pending_owners.is_empty())
                || entry.pending_owners.iter().enumerate().any(|(owner_index, owner)| {
                    !is_canonical_revision_ref_owner(owner)
                        || entry.pending_owners[..owner_index].contains(owner)
                })
                || !seen.insert(&entry.id)
        }) {
            return Err(RegistryError::Internal {
                reason: "hosted revision reference index is invalid".to_string(),
            });
        }
        Ok(index)
    }

    pub(crate) fn bodies(&self) -> impl Iterator<Item = &[u8]> {
        self.refs.iter().map(|entry| entry.bytes.as_slice())
    }

    pub(crate) fn insert(
        &mut self,
        ref_id: &str,
        owner: &str,
        bytes: &[u8],
    ) -> Result<HostedRevisionRefWrite> {
        if let Some(entry) = self.refs.iter_mut().find(|entry| entry.id == ref_id) {
            if entry.bytes != bytes {
                return Err(RegistryError::Internal {
                    reason: "hosted revision reference body conflicts with its id".to_string(),
                });
            }
            if entry.committed {
                return Ok(HostedRevisionRefWrite::Committed);
            }
            if entry.pending_owners.iter().any(|candidate| candidate == owner) {
                return Ok(HostedRevisionRefWrite::AlreadyClaimed);
            }
            entry.pending_owners.push(owner.to_string());
            return Ok(HostedRevisionRefWrite::Claimed);
        }
        if self.refs.len() == MAX_HOSTED_REVISION_REFS {
            return Err(RegistryError::RevisionReferenceLimit { limit: MAX_HOSTED_REVISION_REFS });
        }
        self.refs.push(HostedRevisionRefIndexEntry {
            id: ref_id.to_string(),
            committed: false,
            pending_owners: vec![owner.to_string()],
            bytes: bytes.to_vec(),
        });
        Ok(HostedRevisionRefWrite::Claimed)
    }

    pub(crate) fn remove_if_owned(&mut self, ref_id: &str, owner: &str) -> bool {
        let Some(entry_index) = self.refs.iter().position(|entry| entry.id == ref_id) else {
            return false;
        };
        let Some(owner_index) =
            self.refs[entry_index].pending_owners.iter().position(|candidate| candidate == owner)
        else {
            return false;
        };
        self.refs[entry_index].pending_owners.remove(owner_index);
        if self.refs[entry_index].pending_owners.is_empty() {
            self.refs.remove(entry_index);
        }
        true
    }

    pub(crate) fn is_owned_by(&self, ref_id: &str, owner: &str) -> bool {
        self.refs.iter().any(|entry| {
            entry.id == ref_id && entry.pending_owners.iter().any(|candidate| candidate == owner)
        })
    }

    pub(crate) fn commit_if_owned(&mut self, ref_id: &str, owner: &str) -> Result<bool> {
        let Some(entry) = self.refs.iter_mut().find(|entry| entry.id == ref_id) else {
            return Err(RegistryError::Internal {
                reason: "hosted revision reference is missing during commit".to_string(),
            });
        };
        if entry.committed {
            return Ok(false);
        }
        if !entry.pending_owners.iter().any(|candidate| candidate == owner) {
            return Err(RegistryError::Internal {
                reason: "hosted revision reference is not owned by its committing transaction"
                    .to_string(),
            });
        }
        entry.committed = true;
        entry.pending_owners.clear();
        Ok(true)
    }

    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("hosted revision reference index serializes")
    }
}

pub(super) fn validate_revision_digest(digest: &str) -> Result<()> {
    if integrity_addressed_tarball_integrity(digest).is_some() {
        Ok(())
    } else {
        Err(RegistryError::BadRequest { reason: "invalid sha512 revision digest".to_string() })
    }
}

pub(crate) fn is_canonical_revision_ref_id(ref_id: &str) -> bool {
    ref_id.len() == 64 && ref_id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn is_canonical_revision_ref_owner(owner: &str) -> bool {
    !owner.is_empty()
        && owner.len() <= 64
        && owner.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

pub(super) fn validate_revision_ref_id(ref_id: &str) -> Result<()> {
    if is_canonical_revision_ref_id(ref_id) {
        Ok(())
    } else {
        Err(RegistryError::BadRequest { reason: "invalid revision reference id".to_string() })
    }
}

pub(super) fn validate_revision_ref_owner(owner: &str) -> Result<()> {
    if is_canonical_revision_ref_owner(owner) {
        Ok(())
    } else {
        Err(RegistryError::BadRequest { reason: "invalid revision reference owner".to_string() })
    }
}
