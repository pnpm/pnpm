use super::{
    DocumentWrite, RegistryError, Result, STAGED_DIR, Storage, staged_body_object,
    staged_id_of_meta_object, staged_meta_object, validated_record_name,
};

impl Storage {
    pub async fn read_staged_meta(&self, stage_id: &str) -> Result<Option<Vec<u8>>> {
        self.hosted.read_record(STAGED_DIR, &staged_meta_object(stage_id)?).await
    }

    pub async fn create_staged_meta(&self, stage_id: &str, bytes: &[u8]) -> Result<()> {
        let key = staged_meta_object(stage_id)?;
        if self.hosted.create_record(STAGED_DIR, &key, bytes).await? {
            return Ok(());
        }
        // Stage ids are minted from the CSPRNG, so a taken key is not a
        // publisher's doing.
        Err(RegistryError::Internal { reason: format!("staged record {stage_id} already exists") })
    }

    /// Rewrite a staged record's metadata only while it still holds
    /// `expected`, so an approval acts on the record it read or not at all.
    /// [`DocumentWrite::Conflict`] means another writer claimed or removed
    /// the record in the meantime.
    pub async fn replace_staged_meta_if_current(
        &self,
        stage_id: &str,
        expected: &[u8],
        bytes: &[u8],
    ) -> Result<DocumentWrite> {
        self.hosted.replace_record_if_current(
            STAGED_DIR,
            &staged_meta_object(stage_id)?,
            expected,
            bytes,
        )
        .await
    }

    pub async fn read_staged_body(&self, stage_id: &str) -> Result<Option<Vec<u8>>> {
        self.hosted.read_record(STAGED_DIR, &staged_body_object(stage_id)?).await
    }

    pub async fn create_staged_body(&self, stage_id: &str, bytes: &[u8]) -> Result<()> {
        let key = staged_body_object(stage_id)?;
        if self.hosted.create_record(STAGED_DIR, &key, bytes).await? {
            return Ok(());
        }
        Err(RegistryError::Internal {
            reason: format!("staged record {stage_id} already has a body"),
        })
    }

    /// Remove a staged record — the metadata first, so a concurrent list
    /// never surfaces a record whose body is already gone. `Ok(false)` when
    /// no metadata existed. A body-removal failure is logged rather than
    /// propagated: once the metadata is gone the record is deleted for every
    /// reader, and an error here would misreport that while leaving nothing
    /// for a retry to find (bodies are only discovered through metadata).
    pub async fn remove_staged(&self, stage_id: &str) -> Result<bool> {
        let removed = self.hosted.remove_record(STAGED_DIR, &staged_meta_object(stage_id)?).await?;
        if let Err(err) =
            self.hosted.remove_record(STAGED_DIR, &staged_body_object(stage_id)?).await
        {
            tracing::warn!(error = %err, stage_id, "staged body cleanup failed after removing its metadata");
        }
        Ok(removed)
    }

    /// Every staged record's id, in unspecified order (the listing endpoint
    /// sorts by staging time).
    pub async fn list_staged_ids(&self) -> Result<Vec<String>> {
        let keys = self.hosted.list_record_keys(STAGED_DIR).await?;
        Ok(keys
            .iter()
            .filter_map(|key| staged_id_of_meta_object(key))
            .map(str::to_string)
            .collect())
    }

    pub async fn read_pipeline_run(&self, key: &PipelineRunKey<'_>) -> Result<Option<Vec<u8>>> {
        self.hosted.read_record(PIPELINE_RUNS_DIR, &key.path()?).await
    }

    /// Record a run, reporting `false` when that workspace already has one
    /// under this id — a run record is written once and never rewritten.
    pub async fn create_pipeline_run(
        &self,
        key: &PipelineRunKey<'_>,
        bytes: &[u8],
    ) -> Result<bool> {
        self.hosted.create_record(PIPELINE_RUNS_DIR, &key.path()?, bytes).await
    }

    /// An organization's recorded run keys, `<workspace>/<run id>`, in
    /// unspecified order, within `workspace` when one is named. Scoped so a
    /// listing costs what it asks about rather than what the deployment holds.
    pub async fn list_pipeline_runs(
        &self,
        org: &str,
        workspace: Option<&str>,
    ) -> Result<Vec<String>> {
        let org_namespace = format!("{PIPELINE_RUNS_DIR}/{}", validated_record_name(org)?);
        let Some(workspace) = workspace else {
            return self.hosted.list_record_keys(&org_namespace).await;
        };
        let workspace = validated_record_name(workspace)?;
        let keys = self.hosted.list_record_keys(&format!("{org_namespace}/{workspace}")).await?;
        Ok(keys
            .into_iter()
            .map(|key| format!("{workspace}/{key}"))
            .collect())
    }
}

/// Reserved namespace holding pipeline run records, versioned so a later
/// record shape can live beside this one.
const PIPELINE_RUNS_DIR: &str = ".pipeline-runs/v1";

/// The identity of one pipeline run record.
pub struct PipelineRunKey<'a> {
    pub org: &'a str,
    pub workspace: &'a str,
    /// The record's file name: the run id and its suffix.
    pub run_id: &'a str,
}

impl PipelineRunKey<'_> {
    /// The key within the namespace. The identifiers are the client's, so
    /// they are checked here as well as by the endpoint that accepts them.
    fn path(&self) -> Result<String> {
        Ok(format!(
            "{}/{}/{}",
            validated_record_name(self.org)?,
            validated_record_name(self.workspace)?,
            validated_record_name(self.run_id)?,
        ))
    }
}
