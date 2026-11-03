use semver::{Version, VersionReq};

/// The requirement `pnpm update` writes for `resolved`. It keeps the
/// operator of an explicit range, or else of the existing requirement, as
/// the update of an npm dependency does: `savePrefix` and `saveExact` only
/// shape new declarations.
pub(crate) fn format_cargo_saved_spec(
    existing_raw: &str,
    resolved: &Version,
    explicit_spec: Option<&str>,
) -> String {
    if let Some(spec) = explicit_spec
        && spec.starts_with(['^', '~', '=', '>', '<'])
    {
        // A parsed requirement drops build metadata, which Cargo ignores and
        // warns about.
        return VersionReq::parse(spec).map_or_else(|_| spec.to_string(), |req| req.to_string());
    }
    format_existing_cargo_spec(existing_raw, &without_build_metadata(resolved))
}

pub(crate) fn without_build_metadata(version: &Version) -> Version {
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
