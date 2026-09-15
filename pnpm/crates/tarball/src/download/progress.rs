use super::{
    LogEvent, LogLevel, MAX_THROUGHPUT_PRIORITY, ProgressLog, ProgressMessage, Reporter,
    SharedReportedProgressKeys,
};

/// Emit `pnpm:progress found_in_store` for a (`package_id`, requester)
/// pair the cache resolved without a download.
pub(crate) fn emit_progress_found_in_store<Reporter: self::Reporter>(
    package_id: &str,
    requester: &str,
    progress_key: Option<(&SharedReportedProgressKeys, &str)>,
) {
    if progress_already_reported(progress_key) {
        return;
    }
    Reporter::emit(&LogEvent::Progress(ProgressLog {
        level: LogLevel::Debug,
        message: ProgressMessage::FoundInStore {
            package_id: package_id.to_owned(),
            requester: requester.to_owned(),
        },
    }));
}

pub(crate) fn emit_progress_fetched<Reporter: self::Reporter>(
    package_id: &str,
    requester: &str,
    progress_key: Option<(&SharedReportedProgressKeys, &str)>,
) {
    if progress_already_reported(progress_key) {
        return;
    }
    Reporter::emit(&LogEvent::Progress(ProgressLog {
        level: LogLevel::Debug,
        message: ProgressMessage::Fetched {
            package_id: package_id.to_owned(),
            requester: requester.to_owned(),
        },
    }));
}

pub(crate) fn progress_already_reported(
    progress_key: Option<(&SharedReportedProgressKeys, &str)>,
) -> bool {
    progress_key.is_some_and(|(reported, key)| !reported.insert(key.to_owned()))
}

/// Byte-equivalent cost of one file's fixed pipeline overhead (the
/// CAS-write syscalls and hash setup paid per file regardless of its
/// size, ~75 µs against a pipeline that moves a byte through
/// download + decompress + hash + write in ~25 ns). Folding it into
/// the priority makes a many-small-files package rank as the long
/// job it actually is: extraction cost, not just transfer cost,
/// decides when a package's pipeline work finishes.
pub(crate) const PRIORITY_BYTES_PER_FILE: u64 = 3_000;

/// Queueing priority of a tarball download: the package's estimated
/// total pipeline work (transfer + decompress + hash + CAS writes) in
/// byte-equivalents. Missing hints contribute zero, so a package with
/// no published `dist` stats queues behind every estimated one.
#[must_use]
pub fn download_priority(unpacked_size: Option<usize>, file_count: Option<usize>) -> u64 {
    let size = unpacked_size.map_or(0, |size| size as u64);
    let per_file =
        file_count.map_or(0, |count| (count as u64).saturating_mul(PRIORITY_BYTES_PER_FILE));
    // `UNPRIORITIZED` and `BACKGROUND` are class sentinels; a hostile
    // registry publishing absurd `dist` stats must not be able to
    // saturate a download's priority into either class.
    size.saturating_add(per_file).min(MAX_THROUGHPUT_PRIORITY)
}
