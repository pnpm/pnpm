use super::{
    ErrorKind, HOSTED_REVISION_REF_INDEX_FILE, HOSTED_REVISION_REFS_DIR, HostedRevisionRefIndex,
    HostedRevisionRefWrite, PathBuf, Result, Store, fs, write_atomic,
};

impl Store {
    pub(in super::super) async fn read_revision_refs(&self, digest: &str) -> Result<Vec<Vec<u8>>> {
        let index = self.read_revision_ref_index(digest).await?;
        Ok(index.bodies().map(<[u8]>::to_vec).collect())
    }

    pub(in super::super) async fn write_revision_ref(
        &self,
        digest: &str,
        ref_id: &str,
        owner: &str,
        bytes: &[u8],
    ) -> Result<HostedRevisionRefWrite> {
        let _guard = self.revision_ref_write_lock.lock().await;
        let mut index = self.read_revision_ref_index(digest).await?;
        let outcome = index.insert(ref_id, owner, bytes)?;
        if outcome == HostedRevisionRefWrite::Claimed {
            write_atomic(&self.revision_ref_index_path(digest), &index.to_bytes()).await?;
        }
        Ok(outcome)
    }

    pub(in super::super) async fn remove_revision_ref(
        &self,
        digest: &str,
        ref_id: &str,
        owner: &str,
    ) -> Result<()> {
        let _guard = self.revision_ref_write_lock.lock().await;
        let mut index = self.read_revision_ref_index(digest).await?;
        if index.remove_if_owned(ref_id, owner) {
            write_atomic(&self.revision_ref_index_path(digest), &index.to_bytes()).await?;
        }
        Ok(())
    }

    pub(in super::super) async fn commit_revision_ref(
        &self,
        digest: &str,
        ref_id: &str,
        owner: &str,
    ) -> Result<()> {
        let _guard = self.revision_ref_write_lock.lock().await;
        let mut index = self.read_revision_ref_index(digest).await?;
        if index.commit_if_owned(ref_id, owner)? {
            write_atomic(&self.revision_ref_index_path(digest), &index.to_bytes()).await?;
        }
        Ok(())
    }

    pub(in super::super) async fn read_revision_ref_index(
        &self,
        digest: &str,
    ) -> Result<HostedRevisionRefIndex> {
        match fs::read(self.revision_ref_index_path(digest)).await {
            Ok(bytes) => HostedRevisionRefIndex::from_bytes(&bytes),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(HostedRevisionRefIndex::default()),
            Err(err) => Err(err.into()),
        }
    }

    pub(in super::super) fn revision_refs_dir(&self, digest: &str) -> PathBuf {
        self.root.join(HOSTED_REVISION_REFS_DIR).join(digest)
    }

    pub(in super::super) fn revision_ref_index_path(&self, digest: &str) -> PathBuf {
        self.revision_refs_dir(digest).join(HOSTED_REVISION_REF_INDEX_FILE)
    }
}
