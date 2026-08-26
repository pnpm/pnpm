//! The machine-local trust registry and its terminal prompt.

use super::Candidate;
use pnpm_fs::lexical_normalize;
use serde_json::{Value, json};
use std::{io::IsTerminal, path::Path};

/// Test-only escape hatch mirroring `PNPM_AUTO_APPROVE_BUILDS_FOR_TESTS`:
/// treats every project as trusted without prompting or recording.
pub(super) const AUTO_TRUST_ENV: &str = "PNPM_AUTO_APPROVE_PROJECT_BINS_FOR_TESTS";

/// The trust registry, one JSON record per line, last matching record wins:
/// `{"projectDir": <abs path>, "candidateId": <sha256>, "allow": bool,
/// "decidedAt": <ms since epoch>}`. Corrupt or interleaved lines are ignored,
/// so a concurrent append can only make the dispatcher ask again.
pub(super) const TRUST_FILE_NAME: &str = "global-bin-trust.jsonl";

pub(super) fn is_trusted(candidate: &Candidate, name: &str, state_dir: &Path) -> bool {
    // Debug builds only: the e2e suite spawns real (debug) binaries, and
    // a release binary must not carry an environment backdoor around the
    // trust gate.
    if cfg!(debug_assertions) && std::env::var(AUTO_TRUST_ENV).as_deref() == Ok("1") {
        return true;
    }
    let project_dir = candidate.project_dir();
    let candidate_id = candidate.identity();
    let project_key = lexical_normalize(project_dir).display().to_string();
    let trust_file = (!state_dir.as_os_str().is_empty()).then(|| state_dir.join(TRUST_FILE_NAME));
    if let Some(trust_file) = &trust_file
        && let Some(allow) = read_trust_decision(trust_file, &project_key, candidate_id)
    {
        return allow;
    }
    let Some(allow) = prompt_for_trust(&project_key, name) else {
        return false;
    };
    if let Some(trust_file) = &trust_file {
        // Best-effort: an unwritable state dir means re-prompting next
        // time, which is strictly better than failing the command.
        let _ = append_trust_decision(trust_file, &project_key, candidate_id, allow);
    }
    allow
}

/// The recorded decision for `project_key`, last record wins.
pub(super) fn read_trust_decision(
    trust_file: &Path,
    project_key: &str,
    candidate_id: &str,
) -> Option<bool> {
    let content = std::fs::read_to_string(trust_file).ok()?;
    let mut decision = None;
    for line in content.lines() {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if record.get("projectDir").and_then(Value::as_str) == Some(project_key)
            && record.get("candidateId").and_then(Value::as_str) == Some(candidate_id)
        {
            decision = record.get("allow").and_then(Value::as_bool);
        }
    }
    decision
}

pub(super) fn append_trust_decision(
    trust_file: &Path,
    project_key: &str,
    candidate_id: &str,
    allow: bool,
) -> std::io::Result<()> {
    use std::io::Write as _;
    if let Some(parent) = trust_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let decided_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or_default();
    let record = json!({
        "projectDir": project_key,
        "candidateId": candidate_id,
        "allow": allow,
        "decidedAt": decided_at,
    });
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(trust_file)?;
    writeln!(file, "{record}")
}

/// Ask on the terminal whether the project's bins may be run. `None`
/// means the question could not be answered (CI, no TTY, or the prompt
/// was interrupted) — the caller falls back to the global target and
/// records nothing, so the next interactive invocation asks again.
pub(super) fn prompt_for_trust(project_key: &str, name: &str) -> Option<bool> {
    if is_ci::cached() || !std::io::stdin().is_terminal() {
        return None;
    }
    let prompt = format!(
        "The project at \"{project_key}\" provides its own \"{name}\", which will be used instead of the globally installed one.\nDo you trust this project?",
    );
    dialoguer::Confirm::new().with_prompt(prompt).default(false).interact().ok()
}
