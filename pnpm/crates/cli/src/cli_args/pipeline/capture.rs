//! Capture of a task's `pnpm:lifecycle` output stream for the cache, and
//! its replay on a hit — so a hit renders exactly like a run, through the
//! same reporter.
//!
//! The executor's streamed output takes a plain `fn(&LogEvent)`, so the
//! capture is a process-global tee: [`capturing_emit`] records lifecycle
//! events into per-`(project, stage)` buffers and forwards everything to
//! the real reporter emit installed by [`install_forward`].

use pnpm_reporter::{LifecycleLog, LifecycleMessage, LifecycleStdio, LogEvent, LogLevel};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::Path,
    sync::{LazyLock, Mutex, OnceLock},
};

/// One captured lifecycle stage: what replaying needs to reconstruct the
/// `Script` → `Stdio`* → `Exit` event sequence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedScript {
    pub stage: String,
    pub command: String,
    pub lines: Vec<CapturedLine>,
    pub exit_code: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedLine {
    pub stdio: String,
    pub line: String,
}

pub(super) const MAX_CAPTURE_BYTES: usize = 1024 * 1024;

#[derive(Default)]
struct Buffer {
    command: String,
    lines: Vec<CapturedLine>,
    exit_code: i32,
    bytes: usize,
    exceeded: bool,
}

impl Buffer {
    fn push(&mut self, stdio: LifecycleStdio, line: &str) {
        if self.exceeded {
            return;
        }
        self.bytes = self.bytes.saturating_add(line.len() + std::mem::size_of::<CapturedLine>());
        if self.bytes > MAX_CAPTURE_BYTES {
            self.exceeded = true;
            self.lines.clear();
            return;
        }
        self.lines.push(CapturedLine { stdio: stdio_name(stdio), line: line.to_string() });
    }
}

static FORWARD: OnceLock<fn(&LogEvent)> = OnceLock::new();
static BUFFERS: LazyLock<Mutex<HashMap<(String, String), Buffer>>> = LazyLock::new(Mutex::default);

pub fn install_forward(emit: fn(&LogEvent)) {
    let _ = FORWARD.set(emit);
}

/// The `emit` handed to the executor: tee into the capture buffers, then
/// forward to the reporter.
pub fn capturing_emit(event: &LogEvent) {
    if let LogEvent::Lifecycle(log) = event {
        match &log.message {
            LifecycleMessage::Script { dep_path, stage, script, .. } => {
                with_buffer(dep_path, stage, |buffer| buffer.command.clone_from(script));
            }
            LifecycleMessage::Stdio { dep_path, stage, line, stdio, .. } => {
                with_buffer(dep_path, stage, |buffer| buffer.push(*stdio, line));
            }
            LifecycleMessage::Exit { dep_path, stage, exit_code, .. } => {
                with_buffer(dep_path, stage, |buffer| buffer.exit_code = *exit_code);
            }
        }
    }
    if let Some(forward) = FORWARD.get() {
        forward(event);
    }
}

/// Take the buffered stages of one task's script out of the registry, in
/// the order they ran: the `pre` hook, the script, the `post` hook.
pub fn drain_task(
    dep_path: &str,
    script: &str,
    enable_pre_post_scripts: bool,
) -> Option<Vec<CapturedScript>> {
    let stages: Vec<String> = if enable_pre_post_scripts {
        vec![format!("pre{script}"), script.to_string(), format!("post{script}")]
    } else {
        vec![script.to_string()]
    };
    let mut buffers = BUFFERS.lock().expect("capture buffer lock is not poisoned");
    let mut exceeded = false;
    let scripts = stages
        .into_iter()
        .filter_map(|stage| {
            let buffer = buffers.remove(&(dep_path.to_string(), stage.clone()))?;
            exceeded |= buffer.exceeded;
            Some(CapturedScript {
                stage,
                command: buffer.command,
                lines: buffer.lines,
                exit_code: buffer.exit_code,
            })
        })
        .collect();
    (!exceeded).then_some(scripts)
}

/// Re-emit a stored task's lifecycle events, so a cache hit renders the
/// way the original run did.
pub fn replay(scripts: &[CapturedScript], project_dir: &Path, emit: fn(&LogEvent)) {
    let dep_path = project_dir.to_string_lossy().into_owned();
    for script in scripts {
        emit(&LogEvent::Lifecycle(LifecycleLog {
            level: LogLevel::Debug,
            message: LifecycleMessage::Script {
                dep_path: dep_path.clone(),
                optional: false,
                script: script.command.clone(),
                stage: script.stage.clone(),
                wd: dep_path.clone(),
            },
        }));
        for line in &script.lines {
            emit(&LogEvent::Lifecycle(LifecycleLog {
                level: LogLevel::Debug,
                message: LifecycleMessage::Stdio {
                    dep_path: dep_path.clone(),
                    line: line.line.clone(),
                    stage: script.stage.clone(),
                    stdio: stdio_from_name(&line.stdio),
                    wd: dep_path.clone(),
                },
            }));
        }
        emit(&LogEvent::Lifecycle(LifecycleLog {
            level: LogLevel::Debug,
            message: LifecycleMessage::Exit {
                dep_path: dep_path.clone(),
                exit_code: script.exit_code,
                optional: false,
                stage: script.stage.clone(),
                wd: dep_path.clone(),
            },
        }));
    }
}

fn with_buffer(dep_path: &str, stage: &str, mutate: impl FnOnce(&mut Buffer)) {
    let mut buffers = BUFFERS.lock().expect("capture buffer lock is not poisoned");
    mutate(buffers.entry((dep_path.to_string(), stage.to_string())).or_default());
}

fn stdio_name(stdio: LifecycleStdio) -> String {
    match stdio {
        LifecycleStdio::Stdout => "stdout".to_string(),
        LifecycleStdio::Stderr => "stderr".to_string(),
    }
}

fn stdio_from_name(name: &str) -> LifecycleStdio {
    if name == "stderr" { LifecycleStdio::Stderr } else { LifecycleStdio::Stdout }
}

#[cfg(test)]
mod tests;
