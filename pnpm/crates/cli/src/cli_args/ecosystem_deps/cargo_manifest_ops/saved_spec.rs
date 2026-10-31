use miette::{Result, bail};
use semver::{Version, VersionReq};

pub(crate) fn format_cargo_saved_spec(
    existing_raw: &str,
    resolved: &Version,
    explicit_spec: Option<&str>,
    save_exact: bool,
    save_prefix: Option<&str>,
) -> Result<String> {
    if let Some(prefix) = save_prefix
        && !matches!(prefix, "" | "^" | "~" | "=")
        && !save_exact
    {
        bail!("Cargo --save-prefix must be empty, ^, ~, or =");
    }
    let resolved = without_build_metadata(resolved);
    if save_exact {
        return Ok(format!("={resolved}"));
    }
    if let Some(spec) = explicit_spec
        && spec.starts_with(['^', '~', '=', '>', '<'])
    {
        return Ok(spec.to_string());
    }
    if existing_raw.trim().starts_with('=') {
        return Ok(format!("={resolved}"));
    }
    if let Some(prefix) = save_prefix {
        return Ok(format!("{prefix}{resolved}"));
    }
    Ok(format_existing_cargo_spec(existing_raw, &resolved))
}

fn without_build_metadata(version: &Version) -> Version {
    let mut version = version.clone();
    version.build = semver::BuildMetadata::EMPTY;
    version
}

fn format_existing_cargo_spec(existing_raw: &str, resolved: &Version) -> String {
    if !existing_raw.contains(',') {
        let prefix = existing_prefix(existing_raw);
        return format!("{prefix}{resolved}");
    }
    let matches_existing = VersionReq::parse(existing_raw).is_ok_and(|req| req.matches(resolved));
    if !matches_existing {
        return format!("^{resolved}");
    }
    let mut replaced_lower = false;
    let clauses: Vec<String> = existing_raw
        .split(',')
        .map(|clause| {
            let trimmed = clause.trim();
            if !replaced_lower && let Some(prefix) = lower_bound_prefix(trimmed) {
                replaced_lower = true;
                format!("{prefix}{resolved}")
            } else {
                trimmed.to_string()
            }
        })
        .collect();
    if replaced_lower { clauses.join(", ") } else { existing_raw.to_string() }
}

fn lower_bound_prefix(clause: &str) -> Option<&'static str> {
    if clause.starts_with('>') {
        Some(">=")
    } else {
        ["^", "~"]
            .into_iter()
            .find(|prefix| clause.starts_with(prefix))
    }
}

fn existing_prefix(raw: &str) -> &'static str {
    let trimmed = raw.trim();
    if trimmed.starts_with('>') {
        ">="
    } else {
        ["^", "~", "="]
            .into_iter()
            .find(|prefix| trimmed.starts_with(prefix))
            .unwrap_or("")
    }
}
