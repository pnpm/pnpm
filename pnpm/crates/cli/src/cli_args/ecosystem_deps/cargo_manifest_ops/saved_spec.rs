use semver::{Version, VersionReq};

/// The requirement `pnpm update` writes for `resolved`, without build
/// metadata, which Cargo ignores and warns about.
///
/// - An `explicit_spec` that is a requirement is written as given.
/// - Otherwise the operator of `existing_raw` is kept, as the update of an
///   npm dependency keeps it: `savePrefix` and `saveExact` only shape new
///   declarations.
/// - An existing requirement with no lower bound to move, such as `<=2.0.0`,
///   is kept while `resolved` matches it.
pub(crate) fn format_cargo_saved_spec(
    existing_raw: &str,
    resolved: &Version,
    explicit_spec: Option<&str>,
) -> String {
    if let Some(spec) = explicit_spec {
        if spec.starts_with(['^', '~', '=', '>', '<']) {
            return VersionReq::parse(spec)
                .map_or_else(|_| spec.to_string(), |req| req.to_string());
        }
        if spec.starts_with(|first: char| first.is_ascii_digit()) {
            return spec
                .split('+')
                .next()
                .unwrap_or(spec)
                .to_string();
        }
    }
    format_existing_cargo_spec(existing_raw, &without_build_metadata(resolved))
}

pub(crate) fn without_build_metadata(version: &Version) -> Version {
    let mut version = version.clone();
    version.build = semver::BuildMetadata::EMPTY;
    version
}

fn format_existing_cargo_spec(existing_raw: &str, resolved: &Version) -> String {
    let matches_existing = VersionReq::parse(existing_raw).is_ok_and(|req| req.matches(resolved));
    if !existing_raw.contains(',') {
        return format_single_clause(existing_raw, resolved, matches_existing);
    }
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

fn format_single_clause(existing_raw: &str, resolved: &Version, matches_existing: bool) -> String {
    match existing_prefix(existing_raw) {
        Some(prefix) => format!("{prefix}{resolved}"),
        None if matches_existing => requirement_without_build_metadata(existing_raw),
        None => format!("^{resolved}"),
    }
}

fn requirement_without_build_metadata(raw: &str) -> String {
    if !raw.contains('+') {
        return raw.to_string();
    }
    VersionReq::parse(raw).map_or_else(|_| raw.to_string(), |req| req.to_string())
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

/// The operator to write before the new version, or `None` for a
/// requirement with no lower bound to move, such as `<2` or `1.*`.
fn existing_prefix(raw: &str) -> Option<&'static str> {
    let trimmed = raw.trim();
    if trimmed.starts_with('>') {
        return Some(">=");
    }
    if let Some(prefix) = ["^", "~", "="]
        .into_iter()
        .find(|prefix| trimmed.starts_with(prefix))
    {
        return Some(prefix);
    }
    let version_core = trimmed
        .split(['-', '+'])
        .next()
        .unwrap_or(trimmed);
    let is_bare_version = version_core.starts_with(|first: char| first.is_ascii_digit())
        && !version_core.contains(['*', 'x', 'X']);
    is_bare_version.then_some("")
}
