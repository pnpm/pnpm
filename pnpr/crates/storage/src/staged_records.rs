use super::{MetadataRecord, RegistryError, Result, Storage, validated_record_name};

/// Reserved directory (fs) / key segment (S3) holding staged publishes.
pub(crate) const STAGED_DIR: &str = ".staged";
const STAGED_BODY_SUFFIX: &str = ".body.json";

fn staged_body_object(stage_id: &str) -> Result<String> {
    Ok(format!("{}{STAGED_BODY_SUFFIX}", validated_stage_id(stage_id)?))
}

fn validated_stage_id(stage_id: &str) -> Result<&str> {
    let valid = !stage_id.is_empty()
        && stage_id
            .chars()
            .all(|char| char.is_ascii_hexdigit() || char == '-');
    if valid {
        Ok(stage_id)
    } else {
        Err(RegistryError::BadRequest { reason: format!("invalid stage id {stage_id:?}") })
    }
}

impl Storage {
    pub async fn read_staged_meta(&self, stage_id: &str) -> Result<Option<MetadataRecord>> {
        self.database.read_staged_record(validated_stage_id(stage_id)?).await
    }

    /// Commit metadata for a newly stored body. A failed commit is removed
    /// before its body, since its acknowledgement may have been lost.
    pub async fn create_staged_meta(&self, stage_id: &str, bytes: &[u8]) -> Result<()> {
        match self.database.write_staged_record(validated_stage_id(stage_id)?, None, bytes).await {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(RegistryError::Internal {
                reason: format!("staged record {stage_id} already exists"),
            }),
            Err(failure) => {
                let cleanup = if failure.may_have_committed {
                    self.remove_staged(stage_id).await
                } else {
                    self.remove_staged_body(stage_id).await
                };
                if let Err(error) = cleanup {
                    tracing::warn!(%error, stage_id, "staged publish cleanup failed after its metadata write failed");
                }
                Err(failure.error)
            }
        }
    }

    /// Rewrite a staged record's metadata only while it still holds
    /// `expected`, so an approval acts on the record it read or not at all.
    /// `None` means another writer claimed or removed
    /// the record in the meantime.
    pub async fn replace_staged_meta_if_current(
        &self,
        stage_id: &str,
        expected: &MetadataRecord,
        bytes: &[u8],
    ) -> Result<Option<MetadataRecord>> {
        self.database
            .write_staged_record(validated_stage_id(stage_id)?, Some(expected.generation), bytes)
            .await
            .map_err(|failure| failure.error)
    }

    pub async fn read_staged_body(&self, stage_id: &str) -> Result<Option<Vec<u8>>> {
        self.root.read_record(STAGED_DIR, &staged_body_object(stage_id)?).await
    }

    pub async fn create_staged_body(&self, stage_id: &str, bytes: &[u8]) -> Result<()> {
        let key = staged_body_object(stage_id)?;
        if self.root.create_record(STAGED_DIR, &key, bytes).await? {
            return Ok(());
        }
        Err(RegistryError::Internal {
            reason: format!("staged record {stage_id} already has a body"),
        })
    }

    async fn remove_staged_body(&self, stage_id: &str) -> Result<bool> {
        self.root.remove_record(STAGED_DIR, &staged_body_object(stage_id)?).await
    }

    /// Remove a staged record — the metadata first, so a concurrent list
    /// never surfaces a record whose body is already gone. `Ok(false)` when
    /// no metadata existed. A body-removal failure is logged rather than
    /// propagated: once the metadata is gone the record is deleted for every
    /// reader, and an error here would misreport that while leaving nothing
    /// for a retry to find (bodies are only discovered through metadata).
    pub async fn remove_staged(&self, stage_id: &str) -> Result<bool> {
        let removed = self.database.remove_staged_record(validated_stage_id(stage_id)?).await?;
        if let Err(err) = self.remove_staged_body(stage_id).await {
            tracing::warn!(error = %err, stage_id, "staged body cleanup failed after removing its metadata");
        }
        Ok(removed)
    }

    /// Every staged record's id, in unspecified order (the listing endpoint
    /// sorts by staging time).
    pub async fn list_staged_ids(&self) -> Result<Vec<String>> {
        self.database.staged_ids().await
    }

    /// The staged records from one current database snapshot.
    pub async fn list_staged_meta(&self) -> Result<Vec<(String, MetadataRecord)>> {
        self.database.staged_metadata().await
    }

    pub async fn read_pipeline_run(&self, key: &PipelineRunKey<'_>) -> Result<Option<Vec<u8>>> {
        self.root.read_record(PIPELINE_RUNS_DIR, &key.path()?).await
    }

    /// Record a run, reporting `false` when that workspace already has one
    /// under this id — a run record is written once and never rewritten.
    pub async fn create_pipeline_run(
        &self,
        key: &PipelineRunKey<'_>,
        bytes: &[u8],
    ) -> Result<bool> {
        self.root.create_record(PIPELINE_RUNS_DIR, &key.path()?, bytes).await
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
            return self.root.list_record_keys(&org_namespace).await;
        };
        let workspace = validated_record_name(workspace)?;
        let keys = self.root.list_record_keys(&format!("{org_namespace}/{workspace}")).await?;
        Ok(keys
            .into_iter()
            .map(|key| format!("{workspace}/{key}"))
            .collect())
    }
}

/// Reserved namespace holding pipeline run records.
const PIPELINE_RUNS_DIR: &str = ".pipeline-runs";

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
