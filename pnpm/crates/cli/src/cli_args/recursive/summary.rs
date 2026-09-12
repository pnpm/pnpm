use super::{Context, IndexMap, IntoDiagnostic, Path, Serialize};

/// Write the recursive summary to `pnpm-exec-summary.json` under `dir`.
///
/// The per-task map is nested under an `executionStatus` key. Keys are
/// project directories, `#`-qualified with the task name for tasks
/// `dependsOn` pulled in — see `task_summary_key`.
pub fn write_recursive_summary(
    dir: &Path,
    summary: &IndexMap<String, ExecutionStatus>,
) -> miette::Result<()> {
    let path = dir.join("pnpm-exec-summary.json");
    let mut contents =
        serde_json::to_string_pretty(&ExecSummaryFile { execution_status: summary.clone() })
            .into_diagnostic()?;
    contents.push('\n');
    std::fs::write(&path, contents)
        .into_diagnostic()
        .wrap_err_with(|| format!("writing {}", path.display()))
}

/// Count the tasks whose action failed.
///
/// The caller turns a non-zero count into its command-specific
/// `ERR_PNPM_RECURSIVE_FAIL` error. Skipped dependents of a failed task do
/// not add to the count: the failure that blocked them is already counted.
pub fn count_failures(summary: &IndexMap<String, ExecutionStatus>) -> usize {
    summary.values().filter(|status| status.status == Status::Failure).count()
}

/// `pnpm-exec-summary.json` top-level shape: `{ "executionStatus": { ... } }`.
#[derive(Serialize)]
struct ExecSummaryFile {
    #[serde(rename = "executionStatus")]
    execution_status: IndexMap<String, ExecutionStatus>,
}

/// One package's entry in the recursive summary. `duration` is in
/// milliseconds and present only once the action has run; `prefix` and
/// `message` are filled in for failures.
#[derive(Debug, Clone, Serialize)]
pub struct ExecutionStatus {
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl ExecutionStatus {
    pub fn queued() -> Self {
        ExecutionStatus { status: Status::Queued, duration: None, prefix: None, message: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Queued,
    Running,
    Passed,
    Skipped,
    Failure,
}
