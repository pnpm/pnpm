use std::{
    fs,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub(super) fn process_stamp() -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    let body = format!("pid {} in {}", std::process::id(), cwd.display());
    match unix_now() {
        Some(since) => format!("since {since}\n{body}"),
        None => body,
    }
}

pub(super) fn parse_process_stamp(text: &str) -> (Option<u64>, String) {
    let mut since = None;
    let mut info = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(value) = line.strip_prefix("since ") {
            since = value.parse().ok();
            continue;
        }
        if info.is_empty() {
            info = line.to_string();
        }
    }
    (since, info)
}

pub(super) fn elapsed_since(unix_secs: u64) -> Option<Duration> {
    unix_now()?.checked_sub(unix_secs).map(Duration::from_secs)
}

pub(super) fn elapsed_from_mtime(path: &Path) -> Option<Duration> {
    let modified = fs::metadata(path)
        .ok()?
        .modified()
        .ok()?;
    SystemTime::now().duration_since(modified).ok()
}

pub(crate) fn format_elapsed(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    if secs < 60 {
        return format!("{secs}s");
    }
    let mins = secs / 60;
    let remain = secs % 60;
    if mins < 60 {
        if remain == 0 {
            return format!("{mins}m");
        }
        return format!("{mins}m {remain}s");
    }
    let hours = mins / 60;
    let mins = mins % 60;
    if mins == 0 { format!("{hours}h") } else { format!("{hours}h {mins}m") }
}

fn unix_now() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|elapsed| elapsed.as_secs())
}
