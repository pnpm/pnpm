use std::{
    fs,
    path::Path,
    time::{
        Duration,
        SystemTime,
        UNIX_EPOCH,
    },
};

pub(super) struct ProcessStamp {
    pub since: Option<u64>,
    pub command: Option<String>,
    pub info: String,
}

pub(super) fn process_stamp(command: &str) -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    let pid = format!("pid {} in {}", std::process::id(), oneline(&cwd.to_string_lossy()));
    let command = oneline(command);
    let mut lines = Vec::new();
    if let Some(since) = unix_now() {
        lines.push(format!("since {since}"));
    }
    if !command.is_empty() {
        lines.push(format!("cmd {command}"));
    }
    lines.push(pid);
    lines.join("\n")
}

pub(super) fn parse_process_stamp(text: &str) -> ProcessStamp {
    let mut stamp = ProcessStamp { since: None, command: None, info: String::new() };
    for line in text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        apply_stamp_line(&mut stamp, line);
        if !stamp.info.is_empty() {
            break;
        }
    }
    stamp
}

fn apply_stamp_line(stamp: &mut ProcessStamp, line: &str) {
    if let Some(value) = line.strip_prefix("since ") {
        if stamp.since.is_none() {
            stamp.since = value.parse().ok();
        }
        return;
    }
    if let Some(value) = line.strip_prefix("cmd ") {
        if stamp.command.is_none() && !value.is_empty() {
            stamp.command = Some(value.to_string());
        }
        return;
    }
    if stamp.info.is_empty() {
        stamp.info = line.to_string();
    }
}

fn oneline(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
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
