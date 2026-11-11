use super::{LogId, NoteSigner, ReconcileOutcome, Result, Storage, StoredCheckpoint};

pub(crate) struct ConcurrentReconcile {
    pub(crate) restarted: bool,
    pub(crate) checkpoint: Option<StoredCheckpoint>,
}

impl Storage {
    pub(crate) async fn reconcile_after_concurrent_advance(
        &self,
        log: &LogId,
        key: &NoteSigner,
        concurrent: impl std::future::Future<Output = ()>,
    ) -> Result<ConcurrentReconcile> {
        let signer = log.signer(key)?;
        let origin = signer.verifier_key().name().to_string();
        let mut target = self.read_reconcile_target(log, &origin, Some(&signer)).await?;
        concurrent.await;
        let restarted = matches!(
            self.reconcile_against_target(log, &mut target).await?,
            ReconcileOutcome::TargetAdvanced,
        );
        let checkpoint = self.reconcile_log(log, Some(key)).await?;
        Ok(ConcurrentReconcile { restarted, checkpoint })
    }
}
