//! Human- and machine-readable renderings of an audit report.

use super::{
    AuditAdvisory, AuditReport, AuditVulnerabilityCounts, ConfigAuditLevel, IntoDiagnostic,
    MAX_PATHS_COUNT, OwoColorize, Stream, severity_name, severity_number,
};

pub(crate) fn render_json_report(
    report: &AuditReport,
    audit_level: ConfigAuditLevel,
) -> miette::Result<String> {
    let advisories = report
        .advisories
        .iter()
        .filter(|(_, advisory)| severity_number(advisory.severity) >= severity_number(audit_level))
        .map(|(id, advisory)| (id.clone(), advisory.clone()))
        .collect();
    serde_json::to_string_pretty(&AuditReport { advisories, metadata: report.metadata.clone() })
        .into_diagnostic()
}

pub(crate) fn render_text_report(
    report: &AuditReport,
    audit_level: ConfigAuditLevel,
    ignored: &AuditVulnerabilityCounts,
) -> String {
    let mut advisories = report
        .advisories
        .values()
        .filter(|advisory| severity_number(advisory.severity) >= severity_number(audit_level))
        .collect::<Vec<_>>();
    advisories.sort_by(|left, right| {
        severity_number(right.severity).cmp(&severity_number(left.severity))
    });
    let mut output = String::new();
    for advisory in advisories {
        output.push_str(&render_advisory(advisory));
    }
    let mut found = AuditVulnerabilityCounts::default();
    for advisory in report.advisories.values() {
        found.increment(advisory.severity);
    }
    output.push_str(&report_summary(&found, ignored));
    output
}

pub(crate) fn render_advisory(advisory: &AuditAdvisory) -> String {
    use tabled::{builder::Builder, settings::Style};

    let paths = advisory
        .findings
        .iter()
        .flat_map(|finding| finding.paths.iter().cloned())
        .collect::<Vec<_>>();
    let rendered_paths = if paths.len() > MAX_PATHS_COUNT {
        paths[..MAX_PATHS_COUNT]
            .iter()
            .cloned()
            .chain(std::iter::once(format!(
                "... Found {} paths, run `pnpm why {}` for more information",
                paths.len(),
                advisory.module_name,
            )))
            .collect::<Vec<_>>()
            .join("\n\n")
    } else {
        paths.join("\n\n")
    };

    let mut builder = Builder::default();
    builder.push_record(vec![
        color_severity(advisory.severity, severity_name(advisory.severity)),
        bold(&advisory.title),
    ]);
    builder.push_record(vec!["Package".to_string(), advisory.module_name.clone()]);
    builder
        .push_record(vec!["Vulnerable versions".to_string(), advisory.vulnerable_versions.clone()]);
    builder.push_record(vec![
        "Patched versions".to_string(),
        advisory.patched_versions.clone().unwrap_or_else(|| {
            match advisory.patched_versions_unpublished {
                Some(true) => "None".to_string(),
                _ => "(unknown)".to_string(),
            }
        }),
    ]);
    builder.push_record(vec!["Paths".to_string(), rendered_paths]);
    builder.push_record(vec!["More info".to_string(), advisory.url.clone()]);
    let mut table = builder.build();
    table.with(Style::modern());
    format!("{table}\n")
}

pub(crate) fn report_summary(
    found: &AuditVulnerabilityCounts,
    ignored: &AuditVulnerabilityCounts,
) -> String {
    let total_ignored_count = ignored.total();
    let ignored_summary = if total_ignored_count > 0 {
        format!("\n{total_ignored_count} ignored: {}", list_severity_counts(&ignored.entries()))
    } else {
        String::new()
    };
    let total_vulnerability_count = found.total();
    if total_vulnerability_count == 0 {
        let headline = if total_ignored_count == 0 {
            "No known vulnerabilities found"
        } else {
            "All found vulnerabilities were already reviewed and decided to be ignored"
        };
        return format!("{headline}{ignored_summary}\n");
    }
    format!(
        "{} vulnerabilities found\nSeverity: {}{ignored_summary}",
        red(&total_vulnerability_count.to_string()),
        list_severity_counts(&found.entries()),
    )
}

fn list_severity_counts(severities: &[(ConfigAuditLevel, usize)]) -> String {
    severities
        .iter()
        .filter(|(_, count)| *count > 0)
        .map(|(level, count)| color_severity(*level, &format!("{count} {}", severity_name(*level))))
        .collect::<Vec<_>>()
        .join(" | ")
}

pub(crate) fn bold(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.bold()).to_string()
}

pub(crate) fn red(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.red()).to_string()
}

pub(crate) fn color_severity(level: ConfigAuditLevel, text: &str) -> String {
    match level {
        ConfigAuditLevel::Info => {
            text.if_supports_color(Stream::Stdout, |t| t.dimmed()).to_string()
        }
        ConfigAuditLevel::Low => text.if_supports_color(Stream::Stdout, |t| t.bold()).to_string(),
        ConfigAuditLevel::Moderate => {
            let style = owo_colors::Style::new().yellow().bold();
            text.if_supports_color(Stream::Stdout, |t| t.style(style)).to_string()
        }
        ConfigAuditLevel::High | ConfigAuditLevel::Critical => {
            let style = owo_colors::Style::new().red().bold();
            text.if_supports_color(Stream::Stdout, |t| t.style(style)).to_string()
        }
    }
}

pub(crate) fn green(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.green()).to_string()
}

pub(crate) fn blue(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.blue()).to_string()
}
