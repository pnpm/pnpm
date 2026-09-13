use super::{CheckStatus, DoctorReport};
use std::fmt::Write;

pub(super) fn render_report(report: &DoctorReport) -> String {
    let mut lines: Vec<String> = report.checks
        .iter()
        .map(|check| {
            let mut line = format!("{} {}", status_mark(check.status), check.title);
            if let Some(detail) = &check.detail {
                let _ = write!(line, ": {detail}");
            }
            if let Some(duration) = check.duration_ms {
                let _ = write!(line, " ({duration}ms)");
            }
            if check.status != CheckStatus::Pass
                && let Some(fix) = &check.fix
            {
                let _ = write!(line, "\n    {fix}");
            }
            line
        })
        .collect();

    let failed = report.checks
        .iter()
        .filter(|check| check.status == CheckStatus::Fail)
        .count();
    let warned = report.checks
        .iter()
        .filter(|check| check.status == CheckStatus::Warn)
        .count();
    let summary = if failed > 0 {
        format!("{failed} check(s) failed")
    } else if warned > 0 {
        format!("All checks passed with {warned} warning(s)")
    } else {
        "All checks passed".to_owned()
    };
    lines.push(String::new());
    lines.push(summary);
    lines.join("\n")
}

pub(super) fn status_mark(status: CheckStatus) -> &'static str {
    match status {
        CheckStatus::Pass => "✓",
        CheckStatus::Warn => "‼",
        CheckStatus::Fail => "✗",
    }
}
