//! What becomes of each stored path when a hosted store is converted to
//! [`pnpr_storage::layout`].
//!
//! The earlier layout put a registry without an org at the store root and an
//! org's registry in `<org>/`, so which top-level directories are orgs is
//! known only from the configuration. Inside a namespace a directory holding
//! `package.json` was a package: its other files were its blobs, and its
//! subdirectories other packages (nested image repositories).

use pnpr_package_name::Ecosystem;
use pnpr_storage::layout::{
    blob_segment, decode_blob_segment, decode_package_segments, encode_segment, namespace_segments,
    package_segments,
};
use std::collections::{BTreeMap, BTreeSet};

const LEGACY_DOCUMENT: &str = "package.json";
const TOMBSTONES: &str = ".tombstones";
const TAR_INTEGRITY: &str = ".tar-integrity";
/// Rebuilt or abandoned rather than carried over: the listing index and
/// digest-revision reverse index the new layout has no use for, and image
/// uploads in progress.
const DROPPED: [&str; 3] = [".package-index", ".pnpr-uploads", ".revisions"];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Namespace<'a> {
    org: &'a str,
    ecosystem: Ecosystem,
}

/// What to do with one stored path, given as its `/`-separated components
/// below the store root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Step {
    /// Move it here. `document` marks a package document, moved after the
    /// package's blobs so an interrupted upgrade can be resumed.
    Move {
        to: String,
        document: bool,
    },
    Remove,
    /// Belongs to no package; reported and left where it is.
    Orphan,
}

/// The step for every path that needs one. Paths the upgrade leaves alone
/// (root files, shared records, anything already in the new layout) have
/// none.
///
/// # Errors
///
/// A top-level directory is both an org's namespace and a package of the
/// default namespace, which the earlier layout cannot tell apart.
pub(crate) fn plan(
    paths: &[String],
    orgs: &BTreeSet<(String, Ecosystem)>,
) -> Result<Vec<(String, Step)>, String> {
    let packages = legacy_packages(paths, orgs);
    check_unambiguous(paths, orgs, &packages)?;
    Ok(paths
        .iter()
        .filter_map(|path| step(path, orgs, &packages).map(|step| (path.clone(), step)))
        .collect())
}

/// Refuse what the earlier layout cannot tell apart from the new one.
fn check_unambiguous(
    paths: &[String],
    orgs: &BTreeSet<(String, Ecosystem)>,
    packages: &BTreeSet<(Namespace<'_>, String)>,
) -> Result<(), String> {
    if orgs
        .iter()
        .any(|(org, _)| org == "default")
    {
        return Err("an org named \"default\" cannot be told apart from the namespace of the \
             registry without an org; rename it before upgrading"
            .to_string());
    }
    if let Some((org, _)) = orgs
        .iter()
        .find(|(org, _)| !org.is_empty() && paths.contains(&format!("{org}/{LEGACY_DOCUMENT}")))
    {
        return Err(format!(
            "{org:?} is both an org and a package of the registry without an org; \
             move one of them aside before upgrading",
        ));
    }
    check_shared_orgs(paths, orgs, packages)?;
    // Such a repository would share directories with the packages the
    // upgrade moves into `default/` or `orgs/`.
    if let Some((_, package)) = packages
        .iter()
        .find(|(ns, package)| {
            ns.org.is_empty() && (package.starts_with("default/") || package.starts_with("orgs/"))
        })
    {
        return Err(format!(
            "{package:?} is named like a directory of the new layout; \
             move it aside before upgrading",
        ));
    }
    Ok(())
}

fn check_shared_orgs(
    paths: &[String],
    orgs: &BTreeSet<(String, Ecosystem)>,
    packages: &BTreeSet<(Namespace<'_>, String)>,
) -> Result<(), String> {
    if orgs
        .iter()
        .filter(|(org, _)| org.is_empty())
        .count()
        > 1
        && (packages.iter().any(|(ns, _)| ns.org.is_empty())
            || paths.iter().any(|path| is_namespace_record(path)))
    {
        return Err("more than one ecosystem is configured without an org; \
             keep only the ecosystem the store was written with before upgrading"
            .to_string());
    }
    for (org, ecosystem) in orgs {
        if !org.is_empty() {
            check_named_org(paths, orgs, org, *ecosystem)?;
        }
    }
    Ok(())
}

fn check_named_org(
    paths: &[String],
    orgs: &BTreeSet<(String, Ecosystem)>,
    org: &str,
    ecosystem: Ecosystem,
) -> Result<(), String> {
    if orgs
        .iter()
        .filter(|(other, _)| other == org)
        .count()
        > 1
        && has_path_under(paths, org)
    {
        return Err(format!(
            "{org:?} is configured for more than one ecosystem; \
             keep only the ecosystem the store was written with before upgrading",
        ));
    }
    let synthetic = format!("{ecosystem}~{org}");
    if orgs
        .iter()
        .any(|(other, _)| other == &synthetic)
        && has_path_under(paths, &synthetic)
    {
        return Err(format!(
            "{synthetic:?} is both a configured org and the legacy namespace of {org:?}; \
             rename one before upgrading",
        ));
    }
    Ok(())
}

fn has_path_under(paths: &[String], prefix: &str) -> bool {
    paths
        .iter()
        .any(|path| {
            path.strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('/'))
        })
}

/// Every `(namespace, package)` with a legacy package document in `paths`.
fn legacy_packages<'a>(
    paths: &'a [String],
    orgs: &'a BTreeSet<(String, Ecosystem)>,
) -> BTreeSet<(Namespace<'a>, String)> {
    paths
        .iter()
        .filter_map(|path| {
            let (ns, rest) = split_namespace(path, orgs)?;
            let package = rest.strip_suffix(LEGACY_DOCUMENT)?.strip_suffix('/')?;
            package
                .split('/')
                .all(|part| !part.is_empty() && !part.starts_with('.'))
                .then(|| (ns, package.to_string()))
        })
        .collect()
}

fn step(
    path: &str,
    orgs: &BTreeSet<(String, Ecosystem)>,
    packages: &BTreeSet<(Namespace<'_>, String)>,
) -> Option<Step> {
    if !path.contains('/') || is_new_layout(path, orgs) {
        return None;
    }
    if let Some(rest) = path
        .strip_prefix(TAR_INTEGRITY)
        .and_then(|rest| rest.strip_prefix('/'))
    {
        let ns = Namespace { org: TAR_INTEGRITY, ecosystem: Ecosystem::Npm };
        return Some(package_step(TAR_INTEGRITY, Ecosystem::Npm, rest, |package| {
            packages.contains(&(ns.clone(), package.to_string()))
        }));
    }
    if path.starts_with('.') && !is_namespace_record(path) {
        return None;
    }
    let (ns, rest) = split_namespace(path, orgs)?;
    let prefix = namespace_segments(ns.org, ns.ecosystem).join("/");
    Some(namespace_step(&prefix, ns.ecosystem, rest, |package| {
        packages.contains(&(ns.clone(), package.to_string()))
    }))
}

/// Paths an interrupted upgrade already moved: the exact shapes the new
/// layout gives a namespace's packages and records.
fn is_new_layout(path: &str, orgs: &BTreeSet<(String, Ecosystem)>) -> bool {
    if let Some(rest) = path
        .strip_prefix(TAR_INTEGRITY)
        .and_then(|rest| rest.strip_prefix('/'))
    {
        return is_new_package_path(Ecosystem::Npm, rest);
    }
    if let Some((eco, rest)) = path
        .strip_prefix("default/")
        .and_then(|rest| rest.split_once('/'))
        && let Some(ecosystem) = parse_ecosystem(eco)
    {
        return is_new_namespace_path(ecosystem, rest);
    }
    let Some((org, rest)) = path
        .strip_prefix("orgs/")
        .and_then(|rest| rest.split_once('/'))
    else {
        return false;
    };
    let Some((eco, rest)) = rest.split_once('/') else {
        return false;
    };
    let Some(ecosystem) = parse_ecosystem(eco) else {
        return false;
    };
    orgs.iter()
        .any(|(configured, configured_eco)| {
            !configured.is_empty()
                && *configured_eco == ecosystem
                && encode_segment(configured) == org
        })
        && is_new_namespace_path(ecosystem, rest)
}

fn parse_ecosystem(raw: &str) -> Option<Ecosystem> {
    Ecosystem::all()
        .into_iter()
        .find(|ecosystem| ecosystem.as_str() == raw)
}

fn is_new_namespace_path(ecosystem: Ecosystem, rest: &str) -> bool {
    rest.starts_with('.') || is_new_package_path(ecosystem, rest)
}

fn is_new_package_path(ecosystem: Ecosystem, rest: &str) -> bool {
    let components: Vec<&str> = rest.split('/').collect();
    match components.as_slice() {
        ["packages", segments @ .., "document.json"] => {
            decode_package_segments(ecosystem, segments).is_some()
        }
        ["packages", segments @ .., "blobs", blob] => decode_package_segments(ecosystem, segments)
            .is_some_and(|name| decode_blob_segment(&name, blob).is_some()),
        _ => false,
    }
}

fn is_namespace_record(path: &str) -> bool {
    let top = path
        .split('/')
        .next()
        .unwrap_or_default();
    top == TOMBSTONES || DROPPED.contains(&top)
}

/// The namespace `path` is in, and the path within it.
fn split_namespace<'a>(
    path: &'a str,
    orgs: &'a BTreeSet<(String, Ecosystem)>,
) -> Option<(Namespace<'a>, &'a str)> {
    if let Some((top, rest)) = path.split_once('/') {
        if let Some((org, ecosystem)) = orgs
            .iter()
            .find(|(org, _)| !org.is_empty() && org == top)
        {
            return Some((Namespace { org: org.as_str(), ecosystem: *ecosystem }, rest));
        }
        if let Some((eco_str, org_name)) = top.split_once('~')
            && !org_name.is_empty()
            && let Some((org, ecosystem)) = orgs
                .iter()
                .find(|(org, eco)| org == org_name && eco.as_str() == eco_str)
        {
            return Some((Namespace { org: org.as_str(), ecosystem: *ecosystem }, rest));
        }
    }
    if path.starts_with(TAR_INTEGRITY) {
        let (top, rest) = path.split_once('/')?;
        return Some((Namespace { org: top, ecosystem: Ecosystem::Npm }, rest));
    }
    let ecosystem = orgs
        .iter()
        .find_map(|(org, eco)| org.is_empty().then_some(*eco))
        .unwrap_or(Ecosystem::Npm);
    Some((Namespace { org: "", ecosystem }, path))
}

fn namespace_step(
    prefix: &str,
    ecosystem: Ecosystem,
    rest: &str,
    is_package: impl Fn(&str) -> bool,
) -> Step {
    let top = rest
        .split('/')
        .next()
        .unwrap_or_default();
    if DROPPED.contains(&top) {
        return Step::Remove;
    }
    if top == TOMBSTONES {
        return tombstone_step(prefix, rest);
    }
    if top.starts_with('.') {
        return Step::Orphan;
    }
    package_step(prefix, ecosystem, rest, is_package)
}

/// A tombstone was keyed by the package name as it is; it is now keyed by
/// the name as one encoded segment.
fn tombstone_step(prefix: &str, rest: &str) -> Step {
    let Some((version, file)) = rest
        .strip_prefix(TOMBSTONES)
        .and_then(|rest| rest.strip_prefix('/'))
        .and_then(|rest| rest.split_once('/'))
    else {
        return Step::Orphan;
    };
    let Some(name) = file.strip_suffix(".json") else {
        return Step::Orphan;
    };
    Step::Move {
        to: format!("{prefix}/{TOMBSTONES}/{version}/{}.json", encode_segment(name)),
        document: false,
    }
}

fn package_step(
    prefix: &str,
    ecosystem: Ecosystem,
    rest: &str,
    is_package: impl Fn(&str) -> bool,
) -> Step {
    let Some((package, file)) = rest.rsplit_once('/') else {
        return Step::Orphan;
    };
    if !is_package(package) || file.starts_with('.') {
        return Step::Orphan;
    }
    let package_dir =
        format!("{prefix}/packages/{}", package_segments(ecosystem, package).join("/"));
    if file == LEGACY_DOCUMENT {
        Step::Move { to: format!("{package_dir}/document.json"), document: true }
    } else {
        Step::Move {
            to: format!("{package_dir}/blobs/{}", blob_segment(package, file)),
            document: false,
        }
    }
}

/// How many paths each kind of step covers, for the report.
pub(crate) fn tally(steps: &[(String, Step)]) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for (_, step) in steps {
        let kind = match step {
            Step::Move { document: true, .. } => "documents",
            Step::Move { document: false, .. } => "other files",
            Step::Remove => "removed",
            Step::Orphan => "orphans",
        };
        *counts.entry(kind).or_insert(0) += 1;
    }
    counts
}

#[cfg(test)]
mod tests;
