use std::{marker::PhantomData, time::Duration};

use pnpm_reporter::{
    LockfileVerificationLog, LockfileVerificationMessage, LogEvent, LogLevel, Reporter,
};
use tokio::time::Instant;

/// Shortest gap between two `Progress` events of one run: frequent
/// enough for live feedback, sparse enough for an NDJSON consumer.
const PROGRESS_REPORT_INTERVAL: Duration = Duration::from_millis(250);

pub(super) fn emit<Reporter: self::Reporter>(
    level: LogLevel,
    message: LockfileVerificationMessage,
) {
    Reporter::emit(&LogEvent::LockfileVerification(LockfileVerificationLog { level, message }));
}

/// The reporter's view of one run of the verifiers over a lockfile's
/// entries.
///
/// [`Self::start`] emits `Started` and dropping the value emits the
/// terminal event: `Done` after [`Self::pass`], `Failed` otherwise. An
/// early return and a panic in the fan-out therefore both replace the
/// "Verifying..." frame, and both report how many entries were checked.
pub(super) struct VerificationProgress<Reporter: self::Reporter> {
    entries: u64,
    checked: u64,
    passed: bool,
    lockfile_path: Option<String>,
    started_at: Instant,
    last_progress_at: Instant,
    _reporter: PhantomData<fn() -> Reporter>,
}

impl<Reporter: self::Reporter> VerificationProgress<Reporter> {
    pub(super) fn start(entries: u64, lockfile_path: Option<String>) -> Self {
        emit::<Reporter>(
            LogLevel::Debug,
            LockfileVerificationMessage::Started { entries, lockfile_path: lockfile_path.clone() },
        );
        let started_at = Instant::now();
        Self {
            entries,
            checked: 0,
            passed: false,
            lockfile_path,
            started_at,
            last_progress_at: started_at,
            _reporter: PhantomData,
        }
    }

    /// Count one more checked entry and report the count, unless the
    /// previous report is under [`PROGRESS_REPORT_INTERVAL`] old. The
    /// count of the last entry is left to the terminal event.
    pub(super) fn entry_checked(&mut self) {
        self.checked += 1;
        if self.checked >= self.entries {
            return;
        }
        let now = Instant::now();
        if now.duration_since(self.last_progress_at) < PROGRESS_REPORT_INTERVAL {
            return;
        }
        self.last_progress_at = now;
        emit::<Reporter>(
            LogLevel::Debug,
            LockfileVerificationMessage::Progress {
                entries: self.entries,
                checked: self.checked,
                lockfile_path: self.lockfile_path.clone(),
            },
        );
    }

    pub(super) fn pass(mut self) {
        self.passed = true;
    }
}

impl<Reporter: self::Reporter> Drop for VerificationProgress<Reporter> {
    fn drop(&mut self) {
        let entries = self.entries;
        let checked = self.checked;
        let elapsed_ms = self.started_at.elapsed().as_millis() as u64;
        let lockfile_path = self.lockfile_path.take();
        let message = if self.passed {
            LockfileVerificationMessage::Done { entries, checked, elapsed_ms, lockfile_path }
        } else {
            LockfileVerificationMessage::Failed { entries, checked, elapsed_ms, lockfile_path }
        };
        emit::<Reporter>(LogLevel::Debug, message);
    }
}
