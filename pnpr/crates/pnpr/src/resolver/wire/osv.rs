use pnpm_lockfile::{Lockfile, LockfileResolution, is_git_hosted_tarball_url};
use pnpr_osv::{OsvIndex, format_advisory_ids};
use pnpr_package_name::CanonicalPackageName;
use pnpr_upstream::version_route_tarball_version;

use super::{
    super::protocol::{AppliedPolicies, ResolveRequest},
    serialize_bounded_frame,
};

const OSV_VULNERABILITY_CODE: &str = "ERR_PNPR_OSV_VULNERABILITY";

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct VerifyDoneFrame {
    #[serde(rename = "type")]
    kind: &'static str,
    applied_policies: AppliedPolicies,
}

pub(in crate::resolver) fn verify_done_frame(request: &ResolveRequest) -> Vec<u8> {
    let frame =
        VerifyDoneFrame { kind: "done", applied_policies: AppliedPolicies::from_request(request) };
    serialize_bounded_frame(&frame, "verification failed", &|| false)
        .expect("unconditional frame serialization cannot cancel")
        .unwrap_or_else(|err| err)
}

pub(in crate::resolver) fn osv_violations_for_lockfile_until(
    index: &OsvIndex,
    lockfile: &Lockfile,
    should_cancel: &dyn Fn() -> bool,
) -> Option<Vec<serde_json::Value>> {
    if should_cancel() {
        return None;
    }
    let Some(packages) = lockfile.packages.as_ref() else {
        return Some(Vec::new());
    };
    let mut seen = std::collections::HashSet::new();
    let mut violations = Vec::new();
    for (package_key, snapshot) in packages {
        if should_cancel() {
            return None;
        }
        if let Some(violation) = package_osv_violation(index, package_key, snapshot, &mut seen) {
            violations.push(violation);
        }
    }
    Some(violations)
}

fn package_osv_violation(
    index: &OsvIndex,
    package_key: &pnpm_lockfile::PackageKey,
    snapshot: &pnpm_lockfile::PackageMetadata,
    seen: &mut std::collections::HashSet<(String, String)>,
) -> Option<serde_json::Value> {
    if !is_osv_checkable_resolution(&snapshot.resolution) {
        return None;
    }
    let name = package_key.name.to_string();
    let version = package_key.suffix.version().to_string();
    let ids = vulnerability_ids_for_entry(index, &snapshot.resolution, &name, &version);
    if ids.is_empty() || !seen.insert((name.clone(), version.clone())) {
        return None;
    }
    Some(serde_json::json!({
        "name": name,
        "version": version,
        "code": OSV_VULNERABILITY_CODE,
        "reason": format!(
            "is listed in the local OSV database as vulnerable ({})",
            format_advisory_ids(&ids),
        ),
    }))
}

/// The advisory ids one lockfile entry matches.
///
/// For a tarball resolution the fetched artifact's identity is its URL, not the
/// lockfile key. Under `trustLockfile` a tampered lockfile could key a safe
/// `name@version` while pointing the tarball at a vulnerable artifact, so the
/// version in the tarball filename is screened too. This is additive — a
/// mismatch alone is never a violation (custom registries may name tarballs
/// differently), only an actually-vulnerable version is.
fn vulnerability_ids_for_entry(
    index: &OsvIndex,
    resolution: &LockfileResolution,
    name: &str,
    version: &str,
) -> Vec<String> {
    let mut ids = index.vulnerability_ids(name, version);
    if let LockfileResolution::Tarball(tarball) = resolution
        && let Some(url_version) = tarball_url_version(&tarball.tarball, name)
        && url_version != version
    {
        ids.extend(index.vulnerability_ids(name, url_version));
        ids.sort_unstable();
        ids.dedup();
    }
    ids
}

/// Best-effort extraction of the version from a registry tarball URL of
/// the conventional `<unscoped-name>-<version>.tgz` shape. Returns `None`
/// for non-standard naming so a legitimate custom registry isn't
/// misjudged. Never parses the URL strictly — the lockfile is untrusted.
pub(in crate::resolver) fn tarball_url_version<'a>(url: &'a str, name: &str) -> Option<&'a str> {
    if let Ok(canonical) = CanonicalPackageName::parse(name, pnpr_package_name::Ecosystem::Npm)
        && let Some(version) = version_route_tarball_version(url, &canonical)
    {
        return Some(version);
    }
    let last = url.rsplit('/').next()?;
    let last = last
        .split(['?', '#'])
        .next()
        .unwrap_or(last);
    let stem = strip_tarball_suffix(last)?;
    let unscoped = name.rsplit('/').next().unwrap_or(name);
    let version = stem.strip_prefix(unscoped)?.strip_prefix('-')?;
    (!version.is_empty()).then_some(version)
}

/// Strip a `.tgz` / `.tar.gz` tarball suffix case-insensitively, so a
/// tampered lockfile can't dodge the URL-version cross-check with a
/// `.TGZ` or `.tar.gz` variant. Returns `None` for any other suffix.
fn strip_tarball_suffix(name: &str) -> Option<&str> {
    [".tar.gz", ".tgz"]
        .into_iter()
        .find_map(|suffix| {
            let head_len = name.len().checked_sub(suffix.len())?;
            let (head, tail) = (name.get(..head_len)?, name.get(head_len..)?);
            tail.eq_ignore_ascii_case(suffix).then_some(head)
        })
}

pub(in crate::resolver) fn is_osv_checkable_resolution(resolution: &LockfileResolution) -> bool {
    match resolution {
        LockfileResolution::Registry(_) => true,
        // A frozen lockfile is attacker-controlled, so gate on the tarball
        // URL rather than the tamper-prone `git_hosted` flag or strict URL
        // parsing — otherwise `gitHosted: true` or a barely-malformed URL
        // would let a vulnerable package opt out of the OSV scan. Mirrors
        // the npm verifier's URL-based gate.
        LockfileResolution::Tarball(tarball) => {
            is_http_tarball_url(&tarball.tarball) && !is_git_hosted_tarball_url(&tarball.tarball)
        }
        // Custom resolutions are not registry artifacts, so OSV has
        // no `name@version` advisory coordinates for them.
        LockfileResolution::Directory(_)
        | LockfileResolution::Git(_)
        | LockfileResolution::Binary(_)
        | LockfileResolution::Variations(_)
        | LockfileResolution::Custom(_) => false,
    }
}

/// Whether a tarball URL uses an HTTP or HTTPS scheme — the only schemes a
/// registry artifact is served over. Case-insensitive (so a tampered
/// uppercase scheme can't slip past) without allocating a lowercased copy.
pub(in crate::resolver) fn is_http_tarball_url(url: &str) -> bool {
    let bytes = url.as_bytes();
    bytes
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"https://"))
        || bytes
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"http://"))
}
