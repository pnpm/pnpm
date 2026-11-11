//! Reading, verifying, signing, and advancing `.transparency-log/<log>/checkpoint`.

use super::{LOG_NAMESPACE, LogId, Storage, log_error};
use crate::DocumentWrite;
use pnpm_transparency_log::{
    Checkpoint, CheckpointPolicy, KeySet, NoteSigner, TreeHead, sign_checkpoint,
};
use pnpr_error::{RegistryError, Result};

/// A log's latest checkpoint as it is stored, and the tree head it signs.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredCheckpoint {
    /// The signed note.
    pub note: Vec<u8>,
    pub tree_head: TreeHead,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum CheckpointStatus {
    Published(Option<StoredCheckpoint>),
    BehindStored(TreeHead),
}

impl Storage {
    /// The latest checkpoint of `log`, `None` until an integration signs one.
    pub async fn log_checkpoint(&self, log: &LogId) -> Result<Option<StoredCheckpoint>> {
        let Some(note) = self.root.read_record(LOG_NAMESPACE, &log.key("checkpoint")).await? else {
            return Ok(None);
        };
        let tree_head = tree_head_of(&note)?;
        Ok(Some(StoredCheckpoint { note, tree_head }))
    }

    /// Sign and write a checkpoint of `tree_head`, verifying first that any
    /// stored checkpoint is an exact prefix of `tree_head`.
    pub(super) async fn publish_checkpoint(
        &self,
        log: &LogId,
        prior_head: Option<&TreeHead>,
        tree_head: &TreeHead,
        signer: &NoteSigner,
    ) -> Result<CheckpointStatus> {
        let current = self.log_checkpoint(log).await?;
        if let Some(status) =
            self.check_checkpoint_before_write(log, current.as_ref(), prior_head, tree_head, signer)
                .await?
        {
            return Ok(status);
        }
        let note = sign_checkpoint(tree_head, &[signer]).map_err(log_error)?;
        self.advance_checkpoint(log, current, prior_head, tree_head, signer, &note).await
    }

    async fn check_checkpoint_before_write(
        &self,
        log: &LogId,
        current: Option<&StoredCheckpoint>,
        prior_head: Option<&TreeHead>,
        tree_head: &TreeHead,
        signer: &NoteSigner,
    ) -> Result<Option<CheckpointStatus>> {
        let Some(stored) = current else {
            return Ok((tree_head.size == 0).then_some(CheckpointStatus::Published(None)));
        };
        verify_stored_checkpoint(log, stored, Some(signer))?;
        if stored.tree_head.size > tree_head.size {
            return Ok(Some(CheckpointStatus::BehindStored(stored.tree_head.clone())));
        }
        if stored.tree_head.size == tree_head.size {
            if stored.tree_head.hash != tree_head.hash {
                return Err(RegistryError::Internal {
                    reason: format!(
                        "the stored checkpoint of the log {log} disagrees with its integrated tree",
                    ),
                });
            }
            return Ok(Some(CheckpointStatus::Published(Some(stored.clone()))));
        }
        if prior_head.is_none_or(|prior| {
            prior.size != stored.tree_head.size
                || prior.hash != stored.tree_head.hash
        }) {
            self.verify_checkpoint_consistency(log, &stored.tree_head, tree_head).await?;
        }
        Ok(None)
    }

    /// Replace `current` with `note`, verifying any checkpoint another
    /// integrator wrote meanwhile before skipping or replacing it.
    async fn advance_checkpoint(
        &self,
        log: &LogId,
        mut current: Option<StoredCheckpoint>,
        prior_head: Option<&TreeHead>,
        tree_head: &TreeHead,
        signer: &NoteSigner,
        note: &[u8],
    ) -> Result<CheckpointStatus> {
        let key = log.key("checkpoint");
        loop {
            let written = match &current {
                None => self.root.create_record(LOG_NAMESPACE, &key, note).await?,
                Some(stored) => {
                    let replaced = self.root.replace_record_if_current(
                        LOG_NAMESPACE,
                        &key,
                        &stored.note,
                        note,
                    )
                    .await?;
                    replaced == DocumentWrite::Written
                }
            };
            if written {
                return Ok(CheckpointStatus::Published(Some(StoredCheckpoint {
                    note: note.to_vec(),
                    tree_head: tree_head.clone(),
                })));
            }
            current = self.log_checkpoint(log).await?;
            if let Some(status) = self.check_checkpoint_before_write(
                log,
                current.as_ref(),
                prior_head,
                tree_head,
                signer,
            )
            .await?
            {
                return Ok(status);
            }
        }
    }
}

pub(super) fn verify_stored_checkpoint(
    log: &LogId,
    stored: &StoredCheckpoint,
    signer: Option<&NoteSigner>,
) -> Result<()> {
    if stored.tree_head.size == 0 {
        return Err(RegistryError::Internal {
            reason: format!("the stored checkpoint of the log {log} signs an empty tree"),
        });
    }
    let Some(signer) = signer else {
        return Err(RegistryError::Internal {
            reason: format!(
                "the stored checkpoint of the log {log} cannot be verified without a configured log signing key",
            ),
        });
    };
    let origin = signer.verifier_key().name();
    let log_keys = KeySet::new(vec![signer.verifier_key().clone()], 1).map_err(log_error)?;
    let policy = CheckpointPolicy::new(origin, log_keys, KeySet::empty()).map_err(log_error)?;
    policy
        .verify(&stored.note)
        .map_err(|err| RegistryError::Internal {
            reason: format!("the stored checkpoint of the log {log} failed verification: {err}"),
        })?;
    Ok(())
}

/// The tree head a stored checkpoint note signs: the text before the blank
/// line that starts its signatures.
fn tree_head_of(note: &[u8]) -> Result<TreeHead> {
    let malformed =
        || RegistryError::Internal { reason: "a stored log checkpoint is malformed".to_string() };
    let text_end = note
        .windows(2)
        .position(|pair| pair == b"\n\n")
        .ok_or_else(malformed)?;
    let checkpoint = Checkpoint::from_bytes(&note[..=text_end]).map_err(|_| malformed())?;
    Ok(TreeHead {
        origin: checkpoint.origin().to_string(),
        size: checkpoint.size(),
        hash: *checkpoint.hash(),
    })
}
