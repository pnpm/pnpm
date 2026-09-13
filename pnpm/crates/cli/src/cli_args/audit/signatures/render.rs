use super::{SignatureIssue, SignatureVerificationResult, Stream, bold, red};
use owo_colors::OwoColorize as _;

pub(in super::super) fn render_signature_verification_result(
    result: &SignatureVerificationResult,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!(
        "audited {} {}",
        result.audited,
        plural(result.audited, "package"),
    ));
    lines.push(String::new());

    if result.verified > 0 {
        lines.push(format!(
            "{} {} {} registry {}",
            result.verified,
            if result.verified == 1 {
                "package has a"
            } else {
                "packages have"
            },
            bold("verified"),
            plural(result.verified, "signature"),
        ));
        lines.push(String::new());
    }
    push_missing_signatures(&mut lines, &result.missing);
    push_invalid_signatures(&mut lines, &result.invalid);

    if result.audited == 0
        && result.invalid.is_empty()
        && result.missing.is_empty()
        && result.verified == 0
    {
        lines.push("No dependencies were installed from a registry with signing keys".to_string());
        lines.push(String::new());
    }

    lines.join("\n")
}

/// Packages the registry has signing keys for but published unsigned.
pub(super) fn push_missing_signatures(lines: &mut Vec<String>, missing: &[SignatureIssue]) {
    let count = missing.len();
    if count == 0 {
        return;
    }
    lines.push(format!(
        "{count} {} {} registry {} but the registry is providing signing keys:",
        if count == 1 {
            "package is"
        } else {
            "packages are"
        },
        bright_red("missing"),
        plural(count, "signature"),
    ));
    lines.push(String::new());
    lines.push(issue_table(missing, false));
    lines.push(String::new());
}

/// Packages whose signature did not verify — the tampering warning.
pub(super) fn push_invalid_signatures(lines: &mut Vec<String>, invalid: &[SignatureIssue]) {
    let count = invalid.len();
    if count == 0 {
        return;
    }
    lines.push(format!(
        "{count} {} {} registry {}:",
        if count == 1 {
            "package has an"
        } else {
            "packages have"
        },
        bright_red("invalid"),
        plural(count, "signature"),
    ));
    lines.push(String::new());
    lines.push(issue_table(invalid, true));
    lines.push(String::new());
    lines.push(
        if count == 1 {
            "Someone might have tampered with this package since it was published on the registry!"
        } else {
            "Someone might have tampered with these packages since they were published on the registry!"
        }
        .to_string(),
    );
    lines.push(String::new());
}

pub(super) fn issue_table(issues: &[SignatureIssue], with_reason: bool) -> String {
    use tabled::{builder::Builder, settings::Style};

    let mut builder = Builder::default();
    for issue in issues {
        let package = red(&format!("{}@{}", issue.name, issue.version));
        if with_reason {
            let reason = issue.reason
                .clone()
                .unwrap_or_else(|| "Invalid registry signature".to_string());
            builder.push_record(vec![package, issue.registry.clone(), reason]);
        } else {
            builder.push_record(vec![package, issue.registry.clone()]);
        }
    }
    let mut table = builder.build();
    table.with(Style::modern());
    table.to_string()
}

pub(super) fn plural(count: usize, word: &str) -> String {
    if count == 1 {
        word.to_string()
    } else {
        format!("{word}s")
    }
}

pub(super) fn bright_red(text: &str) -> String {
    text
        .if_supports_color(Stream::Stdout, |t| t.bright_red())
        .to_string()
}
