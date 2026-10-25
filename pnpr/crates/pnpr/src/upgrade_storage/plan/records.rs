use super::{Step, TOMBSTONES};
use pnpr_package_name::Ecosystem;
use pnpr_storage::layout::{decode_package_segments, decode_segment, package_segments};
use std::collections::BTreeSet;

pub(super) fn is_new_tombstone_path(ecosystem: Ecosystem, rest: &str) -> bool {
    let Some(stem) = rest.strip_suffix(".json") else {
        return false;
    };
    let segments: Vec<&str> = stem.split('/').collect();
    decode_package_segments(ecosystem, &segments).is_some()
}

pub(super) fn root_record_step(
    path: &str,
    orgs: &BTreeSet<(String, Ecosystem)>,
    registries: &BTreeSet<(String, Ecosystem)>,
) -> Result<Option<Step>, String> {
    for prefix in [".team-rosters", ".rule-overrides"] {
        if let Some(rest) = path.strip_prefix(&format!("{prefix}/v0/")) {
            let to = if rest.contains('/') {
                format!("{prefix}/{rest}")
            } else {
                let name = rest.strip_suffix(".json").unwrap_or(rest);
                let ecosystem = record_ecosystem(path, name, orgs, registries)?;
                format!("{prefix}/{ecosystem}/{rest}")
            };
            return Ok(Some(Step::Move { to, document: false }));
        }
    }
    for (legacy, current) in [
        (".scim-users/v0/", ".scim-users/"),
        (".pipeline-runs/v1/", ".pipeline-runs/"),
        (".pnpr-artifacts/v0/", ".artifacts/"),
    ] {
        if let Some(rest) = path.strip_prefix(legacy) {
            return Ok(Some(Step::Move { to: format!("{current}{rest}"), document: false }));
        }
    }
    Ok(None)
}

fn record_ecosystem(
    path: &str,
    name: &str,
    orgs: &BTreeSet<(String, Ecosystem)>,
    registries: &BTreeSet<(String, Ecosystem)>,
) -> Result<Ecosystem, String> {
    for candidates in [registries, orgs] {
        let matched: BTreeSet<Ecosystem> = candidates
            .iter()
            .filter(|(candidate, _)| candidate == name)
            .map(|(_, eco)| *eco)
            .collect();
        if matched.len() > 1 {
            return Err(format!(
                "legacy record {path:?} matches registries in more than one ecosystem; \
                 move it to its ecosystem directory before upgrading",
            ));
        }
        if let Some(&eco) = matched.iter().next() {
            return Ok(eco);
        }
    }
    let defaults: BTreeSet<Ecosystem> = orgs
        .iter()
        .filter(|(org, _)| org.is_empty())
        .map(|(_, eco)| *eco)
        .collect();
    if defaults.len() > 1 {
        return Err(format!(
            "legacy record {path:?} is ambiguous across multiple default ecosystems; \
             move it to its ecosystem directory before upgrading",
        ));
    }
    Ok(defaults
        .into_iter()
        .next()
        .unwrap_or(Ecosystem::Npm))
}

/// A tombstone was keyed by the package name under `v0/`; it is now keyed by
/// [`package_segments`].
pub(super) fn tombstone_step(prefix: &str, ecosystem: Ecosystem, rest: &str) -> Step {
    let Some((_version, file)) = rest
        .strip_prefix(TOMBSTONES)
        .and_then(|rest| rest.strip_prefix('/'))
        .and_then(|rest| rest.split_once('/'))
    else {
        return Step::Orphan;
    };
    let Some(name) = file.strip_suffix(".json") else {
        return Step::Orphan;
    };
    let decoded = decode_segment(name).unwrap_or_else(|| name.to_string());
    let segments = package_segments(ecosystem, &decoded).join("/");
    Step::Move { to: format!("{prefix}/{TOMBSTONES}/{segments}.json"), document: false }
}
