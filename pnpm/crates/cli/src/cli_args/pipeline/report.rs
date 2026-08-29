//! The machine-readable account of a pipeline run: an event stream
//! (`events.ndjson`) and a run summary (`summary.json`), written under
//! the pipeline data directory. The stable task identity is the same
//! `<workspace-relative dir>#<task>` the dry-run output uses.

use super::{Selection, SelectionMode, cache::CacheDisposition};
use crate::cli_args::recursive::ExecutionStatus;
use indexmap::IndexMap;
use miette::IntoDiagnostic;
use pnpm_workspace_task_scheduler::{TaskKey, format_task};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

/// What a `--report` submission carries to the pnpr server: the same
/// summary document and event stream the local report files hold.
pub struct RunUpload {
    pub workspace: String,
    pub run_id: String,
    pub summary: Value,
    pub events: Vec<Value>,
}

pub struct RunReport {
    run_id: String,
    pipeline: String,
    base: String,
    revision: Option<String>,
    selection: Value,
    events: Mutex<Vec<Value>>,
    cache_hits: AtomicUsize,
    summary: Mutex<Value>,
}

impl RunReport {
    pub fn new(
        pipeline: &str,
        base: &str,
        selection: &Selection,
        revision: Option<String>,
    ) -> RunReport {
        let mode = match selection.mode {
            SelectionMode::Affected => "affected",
            SelectionMode::Full => "full",
        };
        RunReport {
            run_id: format!("{}-{pipeline}", now_millis()),
            pipeline: pipeline.to_string(),
            base: base.to_string(),
            revision,
            selection: json!({
                "mode": mode,
                "mergeBase": selection.merge_base,
                "changedProjects": selection.changed_count,
                "requestedProjects": selection.requested.len(),
                "includedProjects": selection.selected.len(),
            }),
            events: Mutex::new(Vec::new()),
            cache_hits: AtomicUsize::new(0),
            summary: Mutex::new(Value::Null),
        }
    }

    pub fn task_started(&self, task: &str, key: &str) {
        self.push(json!({
            "event": "taskStarted",
            "task": task,
            "key": key,
            "at": now_millis(),
        }));
    }

    pub fn task_finished(
        &self,
        task: &str,
        status: crate::cli_args::recursive::Status,
        cache: CacheDisposition,
        duration_ms: f64,
    ) {
        if cache == CacheDisposition::Hit {
            self.cache_hits.fetch_add(1, Ordering::Relaxed);
        }
        self.push(json!({
            "event": "taskFinished",
            "task": task,
            "status": status,
            "cache": cache,
            "durationMs": duration_ms,
            "at": now_millis(),
        }));
    }

    pub fn task_skipped(&self, task: &str) {
        self.push(json!({
            "event": "taskSkipped",
            "task": task,
            "at": now_millis(),
        }));
    }

    pub fn cache_hits(&self) -> usize {
        self.cache_hits.load(Ordering::Relaxed)
    }

    /// Assemble the summary document once the run has settled.
    pub fn finish(
        &self,
        statuses: &IndexMap<String, ExecutionStatus>,
        task_keys: &HashMap<TaskKey, String>,
        workspace_root: &Path,
    ) {
        let keys: IndexMap<String, &String> =
            task_keys.iter().map(|(task, key)| (format_task(task, workspace_root), key)).collect();
        *self.summary.lock().expect("summary lock is not poisoned") = json!({
            "runId": self.run_id,
            "pipeline": self.pipeline,
            "base": self.base,
            "revision": self.revision,
            "selection": self.selection,
            "tasks": statuses,
            "taskKeys": keys,
        });
    }

    /// Write `events.ndjson` and `summary.json` into the run's directory
    /// and return it.
    pub fn write(&self, data_dir: &Path) -> miette::Result<PathBuf> {
        let run_dir = data_dir.join("runs").join(&self.run_id);
        fs::create_dir_all(&run_dir).into_diagnostic()?;
        let events = self.events.lock().expect("event lock is not poisoned");
        let mut ndjson = String::new();
        for event in events.iter() {
            ndjson.push_str(&event.to_string());
            ndjson.push('\n');
        }
        fs::write(run_dir.join("events.ndjson"), ndjson).into_diagnostic()?;
        fs::write(
            run_dir.join("summary.json"),
            serde_json::to_vec_pretty(&self.summary_value()).into_diagnostic()?,
        )
        .into_diagnostic()?;
        Ok(run_dir)
    }

    /// The submission a `--report` run sends to the pnpr server.
    pub fn to_upload(&self, workspace: String) -> RunUpload {
        RunUpload {
            workspace,
            run_id: self.run_id.clone(),
            summary: self.summary_value(),
            events: self.events.lock().expect("event lock is not poisoned").clone(),
        }
    }

    /// The summary document: what [`Self::finish`] assembled, or the
    /// header alone for a run that settled before any task existed.
    fn summary_value(&self) -> Value {
        let summary = self.summary.lock().expect("summary lock is not poisoned");
        if summary.is_null() {
            json!({
                "runId": self.run_id,
                "pipeline": self.pipeline,
                "base": self.base,
                "revision": self.revision,
                "selection": self.selection,
                "tasks": {},
            })
        } else {
            summary.clone()
        }
    }

    fn push(&self, event: Value) {
        self.events.lock().expect("event lock is not poisoned").push(event);
    }
}

fn now_millis() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_millis())
}
