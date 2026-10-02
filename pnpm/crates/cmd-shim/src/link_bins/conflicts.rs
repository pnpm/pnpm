use super::{BinOrigin, PackageBinSource};
use crate::bin_resolver::pkg_owns_bin;
use node_semver::Version;
use serde_json::Value;

/// Return `true` when `candidate` should replace `existing` for `bin_name`.
pub(super) fn pick_winner(
    bin_name: &str,
    existing: &PackageBinSource,
    candidate: &PackageBinSource,
) -> bool {
    match (existing.origin, candidate.origin) {
        (BinOrigin::Direct, BinOrigin::Hoisted | BinOrigin::Peer)
        | (BinOrigin::Hoisted, BinOrigin::Peer) => return false,
        (BinOrigin::Hoisted | BinOrigin::Peer, BinOrigin::Direct)
        | (BinOrigin::Peer, BinOrigin::Hoisted) => return true,
        _ => {}
    }
    let existing_name = package_name(existing);
    let candidate_name = package_name(candidate);
    let existing_owns = pkg_owns_bin(bin_name, existing_name);
    let candidate_owns = pkg_owns_bin(bin_name, candidate_name);
    match (existing_owns, candidate_owns) {
        (true, false) => return false,
        (false, true) => return true,
        _ => {}
    }
    if candidate_name != existing_name {
        return candidate_name < existing_name;
    }
    match (package_version(existing), package_version(candidate)) {
        (Some(existing_version), Some(candidate_version)) => candidate_version > existing_version,
        _ => false,
    }
}

pub(super) fn package_name(pkg: &PackageBinSource) -> &str {
    pkg.manifest
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
}

fn package_version(pkg: &PackageBinSource) -> Option<Version> {
    pkg.manifest
        .get("version")
        .and_then(Value::as_str)
        .and_then(|version| Version::parse(version).ok())
}
