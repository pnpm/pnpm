use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use pnpm_reporter::{
    LockfileVerificationLog, LockfileVerificationMessage, LogEvent, LogLevel, Reporter,
};

/// Wall-clock spacing between throttled `Progress` events during the
/// fan-out: frequent enough for live feedback, sparse enough that
/// append-only output and CI logs don't fill with per-completion lines.
pub(super) const PROGRESS_REPORT_INTERVAL: Duration = Duration::from_millis(250);

pub(super) fn emit<Reporter: self::Reporter>(
    level: LogLevel,
    message: LockfileVerificationMessage,
) {
    Reporter::emit(&LogEvent::LockfileVerification(LockfileVerificationLog { level, message }));
}

/// Drop guard that fires the terminal `Failed` payload when the
/// runner panics or returns early through `?`. Paths that know their
/// outcome call [`Self::cancel`] with the terminal payload (`Done` on
/// success, `Failed` with the real checked count after a completed
/// fan-out), which replaces the queued message and emits it on drop
/// instead.
pub(super) struct TerminalEmitGuard<'a, Reporter: self::Reporter> {
    pending: Option<LockfileVerificationMessage>,
    started_at: Instant,
    observed_checked: Option<&'a AtomicU64>,
    _reporter: std::marker::PhantomData<Reporter>,
}

impl<'a, Reporter: self::Reporter> TerminalEmitGuard<'a, Reporter> {
    pub(super) fn failed(
        entries: u64,
        started_at: Instant,
        lockfile_path: Option<String>,
        observed_checked: Option<&'a AtomicU64>,
    ) -> Self {
        Self {
            pending: Some(LockfileVerificationMessage::Failed {
                entries,
                checked: 0,
                elapsed_ms: 0,
                lockfile_path,
            }),
            started_at,
            observed_checked,
            _reporter: std::marker::PhantomData,
        }
    }

    pub(super) fn cancel(&mut self, message: LockfileVerificationMessage) {
        self.pending = Some(message);
    }

    /// Queue the terminal `Failed` payload with an explicit checked
    /// count. The Drop impl refreshes `elapsed_ms` if it ends up
    /// emitting the queued message.
    pub(super) fn fail(&mut self, entries: u64, checked: u64, lockfile_path: Option<String>) {
        self.cancel(LockfileVerificationMessage::Failed {
            entries,
            checked,
            elapsed_ms: 0,
            lockfile_path,
        });
    }
}

impl<Reporter: self::Reporter> Drop for TerminalEmitGuard<'_, Reporter> {
    fn drop(&mut self) {
        let Some(message) = self.pending.take() else { return };
        let message = match message {
            LockfileVerificationMessage::Failed { entries, checked, lockfile_path, .. } => {
                let checked = if checked == 0 {
                    self.observed_checked.map_or(0, |c| c.load(Ordering::Relaxed))
                } else {
                    checked
                };
                LockfileVerificationMessage::Failed {
                    entries,
                    checked,
                    elapsed_ms: self.started_at.elapsed().as_millis() as u64,
                    lockfile_path,
                }
            }
            other => other,
        };
        emit::<Reporter>(LogLevel::Debug, message);
    }
}

/// Live progress for candidate verification: each completed entry is
/// reported as a `Progress` event, throttled to at most one per
/// [`PROGRESS_REPORT_INTERVAL`]. The count reaching `entries` is not
/// reported — the terminal `Done`/`Failed` carries the final count, and
/// in append-only output an extra event would be a redundant line.
/// Every reported count is mirrored into `observed`, so the caller can
/// surface how far an aborted pass got.
pub(super) fn progress_reporter<Reporter: self::Reporter>(
    entries: u64,
    started_at: Instant,
    lockfile_path: Option<String>,
    observed: &AtomicU64,
) -> impl FnMut(u64) + Send {
    let mut last_reported_at = started_at;
    move |checked: u64| {
        observed.store(checked, Ordering::Relaxed);
        if checked == entries {
            return;
        }
        let now = Instant::now();
        if now.duration_since(last_reported_at) < PROGRESS_REPORT_INTERVAL {
            return;
        }
        last_reported_at = now;
        emit::<Reporter>(
            LogLevel::Debug,
            LockfileVerificationMessage::Progress {
                entries,
                checked,
                lockfile_path: lockfile_path.clone(),
            },
        );
    }
}
