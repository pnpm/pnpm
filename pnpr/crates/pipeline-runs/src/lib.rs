//! The pipeline run-record store: append-only CI run summaries and event
//! streams, namespaced by workspace. Tier 1 of the pnpm CI-server design —
//! the server remembers what `pnpm pipeline` runs reported, and nothing
//! more: it schedules nothing and executes nothing.
//!
//! Records are testimony about something that happened once, not
//! regenerable derived data, so they live under the authoritative
//! `storage` root rather than the disposable cache root. This
//! proof-of-concept tier is filesystem-only; an S3-hosted deployment
//! keeps its run records on the replica's local storage path.

use pnpr_error::{RegistryError, Result};
use pnpr_storage::write_atomic;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Bounds a submission the same way the artifact endpoints bound theirs:
/// a malformed or hostile client must not be able to grow a record
/// without limit. The HTTP body limit is the outer bound; these are the
/// structural ones.
pub const MAX_NAME_LEN: usize = 100;
pub const MAX_RUN_EVENTS: usize = 10_000;
pub const MAX_LIST_RUNS: usize = 200;

const RUNS_DIR: &str = "pipeline-runs/v0";

/// One submitted run: the machine-readable account `pnpm pipeline`
/// produced, verbatim. The server stores the summary and events as
/// opaque JSON — their shape belongs to the client, so a client update
/// does not require a server release.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishPipelineRun {
    /// The workspace the run belongs to. An identifier the client
    /// chooses, not a path: see [`validate_name`].
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
    root: PathBuf,
}

impl PipelineRunStore {
    pub fn new(storage_root: &Path) -> Result<Self> {
        let root = storage_root.join(RUNS_DIR);
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
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
        let path = self.run_path(&run.workspace, &run.run_id);
        if path.exists() {
            return Err(RegistryError::BadRequest {
                reason: format!(
                    "run {} is already recorded for workspace {} (runs are append-only)",
                    run.run_id, run.workspace,
                ),
            });
        }
        std::fs::create_dir_all(path.parent().expect("run path has a workspace parent"))?;
        let document = serde_json::to_vec(&run)?;
        write_atomic(&path, &document).await
    }

    /// The most recent runs, newest first — run ids sort by their leading
    /// timestamp. `workspace` narrows the listing to one workspace.
    pub fn list(&self, workspace: Option<&str>, limit: usize) -> Result<Vec<PipelineRunEntry>> {
        let limit = limit.clamp(1, MAX_LIST_RUNS);
        let workspaces: Vec<String> = if let Some(workspace) = workspace {
            validate_name(workspace, "workspace")?;
            vec![workspace.to_string()]
        } else {
            let mut workspaces = Vec::new();
            for entry in read_dir_or_empty(&self.root)? {
                let entry = entry?;
                if entry.file_type()?.is_dir()
                    && let Some(name) = entry.file_name().to_str()
                {
                    workspaces.push(name.to_string());
                }
            }
            workspaces
        };
        let mut entries: Vec<PipelineRunEntry> = Vec::new();
        for workspace in workspaces {
            for entry in read_dir_or_empty(&self.root.join(&workspace))? {
                let entry = entry?;
                let file_name = entry.file_name();
                let Some(run_id) = file_name.to_str().and_then(|name| name.strip_suffix(".json"))
                else {
                    continue;
                };
                let Some(record) = read_run(&entry.path())? else { continue };
                entries.push(PipelineRunEntry {
                    workspace: workspace.clone(),
                    run_id: run_id.to_string(),
                    summary: record.summary,
                });
            }
        }
        entries.sort_by(|left, right| right.run_id.cmp(&left.run_id));
        entries.truncate(limit);
        Ok(entries)
    }

    /// One run's full record — summary and event stream — or `None` when
    /// nothing was recorded under that identity.
    pub fn get(&self, workspace: &str, run_id: &str) -> Result<Option<PublishPipelineRun>> {
        validate_name(workspace, "workspace")?;
        validate_name(run_id, "runId")?;
        read_run(&self.run_path(workspace, run_id))
    }

    fn run_path(&self, workspace: &str, run_id: &str) -> PathBuf {
        self.root.join(workspace).join(format!("{run_id}.json"))
    }
}

fn read_run(path: &Path) -> Result<Option<PublishPipelineRun>> {
    let text = match std::fs::read(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    Ok(Some(serde_json::from_slice(&text)?))
}

fn read_dir_or_empty(path: &Path) -> Result<Vec<std::io::Result<std::fs::DirEntry>>> {
    match std::fs::read_dir(path) {
        Ok(entries) => Ok(entries.collect()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
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
