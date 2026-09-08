//! The pipeline run-record store: append-only CI run summaries and event
//! streams, namespaced by workspace. Tier 1 of the pnpm CI-server design —
//! the server remembers what `pnpm pipeline` runs reported, and nothing
//! more: it schedules nothing and executes nothing.
//!
//! Records are testimony about something that happened once, not
//! regenerable derived data, so they live in the hosted store — the
//! authoritative one every replica of a deployment shares — rather than on
//! the replica that happened to receive the submission.

use pnpr_error::{RegistryError, Result};
use pnpr_storage::Storage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

/// Bounds a submission the same way the artifact endpoints bound theirs:
/// a malformed or hostile client must not be able to grow a record
/// without limit. The HTTP body limit is the outer bound; these are the
/// structural ones.
pub const MAX_NAME_LEN: usize = 100;
pub const MAX_RUN_EVENTS: usize = 10_000;
pub const MAX_LIST_RUNS: usize = 200;

/// What a run's key ends in, so a later record kind can share the namespace.
const RECORD_SUFFIX: &str = ".json";

/// One submitted run: the machine-readable account `pnpm pipeline`
/// produced, verbatim. The server stores the summary and events as
/// opaque JSON — their shape belongs to the client, so a client update
/// does not require a server release.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishPipelineRun {
    /// The workspace the run belongs to. An identifier the client
    /// chooses, not a path: the closed alphabet is enforced before any
    /// path join.
    pub workspace: String,
    pub run_id: String,
    pub summary: Value,
    #[serde(default)]
    pub events: Vec<Value>,
}

/// One row of a run listing: the summary without its event stream.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineRunEntry {
    pub workspace: String,
    pub run_id: String,
    pub summary: Value,
}

pub struct PipelineRunStore {
    storage: Storage,
}

impl PipelineRunStore {
    #[must_use]
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    /// Record one run. Append-only: a run id that already exists for the
    /// workspace is refused rather than overwritten — a results store
    /// whose history can be rewritten protects nothing.
    pub async fn publish(&self, run: &PublishPipelineRun) -> Result<()> {
        validate_name(&run.workspace, "workspace")?;
        validate_name(&run.run_id, "runId")?;
        if run.events.len() > MAX_RUN_EVENTS {
            return Err(RegistryError::BadRequest {
                reason: format!(
                    "a run carries at most {MAX_RUN_EVENTS} events, got {}",
                    run.events.len(),
                ),
            });
        }
        let document = serde_json::to_vec(&run)?;
        let key = format!("{}{RECORD_SUFFIX}", run.run_id);
        if self.storage.create_pipeline_run(&run.workspace, &key, &document).await? {
            return Ok(());
        }
        Err(RegistryError::BadRequest {
            reason: format!(
                "run {} is already recorded for workspace {} (runs are append-only)",
                run.run_id, run.workspace,
            ),
        })
    }

    /// The most recent runs across `workspaces`, newest first — run ids sort
    /// by their leading timestamp.
    ///
    /// Only the runs that make the page are read: the ids are picked from the
    /// listing first, so a long history costs a listing rather than a read per
    /// record. Every workspace to search is named, because which ones a caller
    /// may see is the endpoint's decision, not the store's.
    pub async fn list(&self, workspaces: &[&str], limit: usize) -> Result<Vec<PipelineRunEntry>> {
        let limit = limit.clamp(1, MAX_LIST_RUNS);
        let mut newest = BTreeSet::new();
        for workspace in workspaces {
            validate_name(workspace, "workspace")?;
            let keys = self.storage.list_pipeline_runs(workspace).await?;
            keep_newest_runs(&mut newest, workspace, keys, limit);
        }
        let mut entries = Vec::with_capacity(newest.len());
        for (run_id, workspace) in newest.into_iter().rev() {
            if let Some(record) = self.get(&workspace, &run_id).await? {
                entries.push(PipelineRunEntry { workspace, run_id, summary: record.summary });
            }
        }
        Ok(entries)
    }

    /// One run's full record — summary and event stream — or `None` when
    /// nothing was recorded under that identity.
    pub async fn get(&self, workspace: &str, run_id: &str) -> Result<Option<PublishPipelineRun>> {
        validate_name(workspace, "workspace")?;
        validate_name(run_id, "runId")?;
        let key = format!("{run_id}{RECORD_SUFFIX}");
        let Some(bytes) = self.storage.read_pipeline_run(workspace, &key).await? else {
            return Ok(None);
        };
        serde_json::from_slice(&bytes).map(Some).map_err(|error| RegistryError::Internal {
            // Name the record: it is one of many in a store several replicas
            // write, and an operator has to be able to find the one at fault.
            reason: format!("pipeline run {workspace}/{run_id} is not readable: {error}"),
        })
    }
}

/// Keep the `limit` highest run identities of one workspace's listing.
///
/// Only what this store writes is a run: anything else under the workspace —
/// a nested path, a file with another suffix — is passed over rather than
/// failing the listing.
fn keep_newest_runs(
    newest: &mut BTreeSet<(String, String)>,
    workspace: &str,
    keys: Vec<String>,
    limit: usize,
) {
    for key in keys {
        let Some(run_id) = key.strip_suffix(RECORD_SUFFIX) else { continue };
        if validate_name(run_id, "runId").is_err() {
            continue;
        }
        newest.insert((run_id.to_string(), workspace.to_string()));
        if newest.len() > limit {
            newest.pop_first();
        }
    }
}

/// The identifiers key filesystem paths, so their alphabet is closed:
/// ASCII alphanumerics plus `.`, `_`, `-`, never starting with a dot.
/// Anything else — separators, traversal, control characters, an empty
/// string — is refused before it reaches a path join.
fn validate_name(name: &str, field: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && !name.starts_with('.')
        && name.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'));
    if valid {
        return Ok(());
    }
    Err(RegistryError::BadRequest {
        reason: format!(
            "{field} must be 1-{MAX_NAME_LEN} ASCII alphanumeric/._- characters not starting with a dot",
        ),
    })
}

#[cfg(test)]
mod tests;
