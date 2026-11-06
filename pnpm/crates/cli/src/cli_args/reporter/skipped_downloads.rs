use pnpm_reporter::{
    LogEvent, SkippedOptionalDependencyLog, SkippedOptionalPackage, SkippedOptionalReason,
};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// The optional dependencies [`super::CliReporter`] saw skipped because their
/// download failed, while a [`SkippedDownloads`] guard is alive. Recorded even
/// under [`super::EventFilter::All`], which hides the warning that reports
/// them. Process-global for the reason [`super::EventFilter`] is.
static CAPTURED: Mutex<Option<Vec<String>>> = Mutex::new(None);

/// Records the optional dependencies an install skips because their download
/// failed, until it drops. Hold it only around work that runs alone.
pub(crate) struct SkippedDownloads(());

impl SkippedDownloads {
    #[must_use]
    pub(crate) fn capture() -> Self {
        *captured() = Some(Vec::new());
        SkippedDownloads(())
    }

    /// The downloads skipped since [`Self::capture`] or the last call, each
    /// naming the package and the error.
    #[expect(clippy::unused_self, reason = "only a live capture has downloads to take")]
    pub(crate) fn take(&self) -> Vec<String> {
        captured()
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }
}

impl Drop for SkippedDownloads {
    fn drop(&mut self) {
        *captured() = None;
    }
}

pub(super) fn record(event: &LogEvent) {
    let LogEvent::SkippedOptionalDependency(log) = event else { return };
    if log.reason != SkippedOptionalReason::FetchFailure {
        return;
    }
    if let Some(downloads) = captured().as_mut() {
        downloads.push(describe(log));
    }
}

fn describe(log: &SkippedOptionalDependencyLog) -> String {
    let package = match &log.package {
        SkippedOptionalPackage::Installed { name, version, .. } => format!("{name}@{version}"),
        SkippedOptionalPackage::ResolutionFailure { bare_specifier, .. } => bare_specifier.clone(),
    };
    match &log.details {
        Some(details) => format!("{package} could not be downloaded: {details}"),
        None => format!("{package} could not be downloaded"),
    }
}

fn captured() -> MutexGuard<'static, Option<Vec<String>>> {
    CAPTURED.lock().unwrap_or_else(PoisonError::into_inner)
}
