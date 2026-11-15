use miette::{IntoDiagnostic, Result};
use pep440_rs::{Operator, Version, VersionSpecifier, VersionSpecifiers};
use pep508_rs::{Requirement, VersionOrUrl};
use pnpm_python_resolver::LockedPin;

pub(super) fn compute_saved_specifiers_for_pins(
    req: &Requirement,
    pins: &[LockedPin<'_>],
    explicit_spec: Option<&str>,
    latest: bool,
) -> Result<Option<VersionSpecifiers>> {
    let Some(min_version) = pins.iter().map(|pin| pin.version).min() else {
        return Ok(None);
    };
    if pins
        .iter()
        .all(|pin| pin.version == min_version)
    {
        return compute_saved_specifiers(req, min_version, explicit_spec, latest);
    }
    let candidate = compute_saved_specifiers(req, min_version, explicit_spec, latest)?;
    let existing = match &req.version_or_url {
        Some(VersionOrUrl::VersionSpecifier(specifiers)) => Some(specifiers),
        Some(VersionOrUrl::Url(_)) | None => None,
    };
    let effective = candidate.as_ref().or(existing);
    if effective.is_none_or(|specifiers| admits_all_pins(specifiers, pins)) {
        return Ok(candidate);
    }
    if let Some(base_specs) = effective.filter(|specifiers| {
        pins.iter()
            .all(|pin| specifiers.contains(pin.version))
    }) {
        let with_pre: VersionSpecifiers =
            format!("{base_specs},>=0a0").parse().into_diagnostic()?;
        return Ok((existing != Some(&with_pre) && admits_all_pins(&with_pre, pins)).then_some(
            with_pre,
        ));
    }
    if latest || explicit_spec.is_some() {
        return widened_floor_for_pins(min_version, pins, existing);
    }
    Ok(None)
}

fn admits_all_pins(specifiers: &VersionSpecifiers, pins: &[LockedPin<'_>]) -> bool {
    let admits_prerelease = specifiers.iter().any(VersionSpecifier::any_prerelease);
    pins.iter()
        .all(|pin| {
            specifiers.contains(pin.version) && (!pin.version.any_prerelease() || admits_prerelease)
        })
}

fn widened_floor_for_pins(
    min_version: &Version,
    pins: &[LockedPin<'_>],
    existing: Option<&VersionSpecifiers>,
) -> Result<Option<VersionSpecifiers>> {
    let base = min_version.clone().without_local();
    let needs_prerelease =
        !min_version.any_prerelease() && pins.iter().any(|pin| pin.version.any_prerelease());
    let floor = if needs_prerelease { format!(">=0a0,>={base}") } else { format!(">={base}") };
    let parsed: VersionSpecifiers = floor.parse().into_diagnostic()?;
    Ok((existing != Some(&parsed) && admits_all_pins(&parsed, pins)).then_some(parsed))
}

fn compute_saved_specifiers(
    req: &Requirement,
    version: &Version,
    explicit_spec: Option<&str>,
    latest: bool,
) -> Result<Option<VersionSpecifiers>> {
    if let Some(spec) = explicit_spec
        && !spec.starts_with("==")
    {
        return Ok(Some(spec.parse().into_diagnostic()?));
    }
    let Some(VersionOrUrl::VersionSpecifier(existing)) = &req.version_or_url else {
        return explicit_spec
            .map(|_| format!(">={version}").parse().into_diagnostic())
            .transpose();
    };
    updated_existing_specifiers(existing, version, latest || explicit_spec.is_some())
}

/// The lower bound of a `~=` requirement updated to `version`, with as many
/// release segments as `spec` so its upper bound keeps the same precision.
/// The pre-release marker stays, so the requirement still admits `version`,
/// and the bound does not drop below `spec`'s while `spec` admits `version`.
fn compatible_release_floor(spec: &VersionSpecifier, version: &Version) -> Version {
    let segments = spec.version().release().len();
    let floor = version
        .clone()
        .without_local()
        .with_release(padded_release(version, segments));
    if spec.contains(version) { floor.max(spec.version().clone()) } else { floor }
}

/// `version`'s epoch and first `segments` release numbers, zero-padded.
fn release_prefix(version: &Version, segments: usize) -> Version {
    Version::new(padded_release(version, segments)).with_epoch(version.epoch())
}

fn padded_release(version: &Version, segments: usize) -> Vec<u64> {
    let mut release = version.release().to_vec();
    release.resize(segments, 0);
    release
}

/// Updates `existing` for `version`. The resolver admits a pre-release only
/// when a clause names one, so a selected pre-release that the result does not
/// name gets a `>=version` clause.
pub(super) fn updated_existing_specifiers(
    existing: &VersionSpecifiers,
    version: &Version,
    force_bump: bool,
) -> Result<Option<VersionSpecifiers>> {
    let updated = updated_primary_specifiers(existing, version, force_bump)?;
    let saved = updated.as_ref().unwrap_or(existing);
    if !version.any_prerelease() || saved.iter().any(VersionSpecifier::any_prerelease) {
        return Ok(updated.filter(|updated| updated != existing));
    }
    let saved: VersionSpecifiers =
        format!("{saved},>={}", version.clone().without_local()).parse().into_diagnostic()?;
    Ok(Some(saved))
}

fn updated_primary_specifiers(
    existing: &VersionSpecifiers,
    version: &Version,
    force_bump: bool,
) -> Result<Option<VersionSpecifiers>> {
    if existing.len() == 1 {
        return updated_single_specifier(&existing[0], existing, version, force_bump);
    }
    if force_bump && !existing.contains(version) {
        return Ok(Some(format!(">={version}").parse().into_diagnostic()?));
    }
    let clauses: Vec<String> = existing
        .iter()
        .map(|clause| {
            if *clause.operator() == Operator::GreaterThanEqual {
                format!(">={version}")
            } else {
                clause.to_string()
            }
        })
        .collect();
    Ok(Some(
        clauses
            .join(",")
            .parse()
            .into_diagnostic()?,
    ))
}

fn updated_single_specifier(
    spec: &VersionSpecifier,
    existing: &VersionSpecifiers,
    version: &Version,
    force_bump: bool,
) -> Result<Option<VersionSpecifiers>> {
    let formatted = match *spec.operator() {
        Operator::Equal => force_bump.then(|| format!("=={version}")),
        Operator::ExactEqual => force_bump.then(|| format!("==={version}")),
        Operator::TildeEqual => Some(format!("~={}", compatible_release_floor(spec, version))),
        Operator::GreaterThanEqual => Some(format!(">={version}")),
        Operator::EqualStar => (force_bump && !existing.contains(version)).then(|| {
            format!("=={}.*", release_prefix(version, spec.version().release().len()))
        }),
        _ => (force_bump && !existing.contains(version)).then(|| format!(">={version}")),
    };
    formatted
        .map(|text| text.parse().into_diagnostic())
        .transpose()
}
