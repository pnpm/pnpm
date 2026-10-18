//! What becomes of each stored path when a hosted store is converted to
//! [`pnpr_storage::layout`].
//!
//! The earlier layout put a registry without an org at the store root and an
//! org's registry in `<org>/`, so which top-level directories are orgs is
//! known only from the configuration. Inside a namespace a directory holding
//! `package.json` was a package: its other files were its blobs, and its
//! subdirectories other packages (nested image repositories).

use pnpr_storage::layout::{decode_segment, encode_segment, namespace_segments};
use std::collections::{BTreeMap, BTreeSet};

const LEGACY_DOCUMENT: &str = "package.json";
const TOMBSTONES: &str = ".tombstones";
const REVISIONS: &str = ".revisions";
const TAR_INTEGRITY: &str = ".tar-integrity";
/// Rebuilt or abandoned rather than carried over: the listing index the new
/// layout has no use for, and image uploads in progress.
const DROPPED: [&str; 2] = [".package-index", ".pnpr-uploads"];

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
    orgs: &BTreeSet<String>,
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
    orgs: &BTreeSet<String>,
    packages: &BTreeSet<(String, String)>,
) -> Result<(), String> {
    if orgs.contains("default") {
        return Err("an org named \"default\" cannot be told apart from the namespace of the \
             registry without an org; rename it before upgrading"
            .to_string());
    }
    if let Some(org) = orgs
        .iter()
        .find(|org| paths.contains(&format!("{org}/{LEGACY_DOCUMENT}")))
    {
        return Err(format!(
            "{org:?} is both an org and a package of the registry without an org; \
             move one of them aside before upgrading",
        ));
    }
    // Such a repository would share directories with the packages the
    // upgrade moves into `default/packages/` or `orgs/`.
    if let Some((_, package)) = packages
        .iter()
        .find(|(org, package)| {
            org.is_empty()
                && (package.starts_with("default/packages/") || package.starts_with("orgs/"))
        })
    {
        return Err(format!(
            "{package:?} is named like a directory of the new layout; \
             move it aside before upgrading",
        ));
    }
    Ok(())
}

/// Every `(org, package)` the earlier layout holds a document for.
fn legacy_packages(paths: &[String], orgs: &BTreeSet<String>) -> BTreeSet<(String, String)> {
    paths
        .iter()
        .filter_map(|path| {
            let (org, rest) = split_namespace(path, orgs)?;
            let package = rest.strip_suffix(LEGACY_DOCUMENT)?.strip_suffix('/')?;
            (!package.starts_with('.')).then(|| (org.to_string(), package.to_string()))
        })
        .collect()
}

fn step(
    path: &str,
    orgs: &BTreeSet<String>,
    packages: &BTreeSet<(String, String)>,
) -> Option<Step> {
    if !path.contains('/') || is_new_layout(path, orgs) {
        return None;
    }
    if let Some(rest) = path
        .strip_prefix(TAR_INTEGRITY)
        .and_then(|rest| rest.strip_prefix('/'))
    {
        return Some(package_step(TAR_INTEGRITY, rest, |package| {
            packages.contains(&(TAR_INTEGRITY.to_string(), package.to_string()))
        }));
    }
    if path.starts_with('.') && !is_namespace_record(path) {
        return None;
    }
    let (org, rest) = split_namespace(path, orgs)?;
    let prefix = namespace_segments(org).join("/");
    Some(namespace_step(&prefix, rest, |package| {
        packages.contains(&(org.to_string(), package.to_string()))
    }))
}

/// Paths an interrupted upgrade already moved: the exact shapes the new
/// layout gives a namespace's packages and records.
fn is_new_layout(path: &str, orgs: &BTreeSet<String>) -> bool {
    if let Some(rest) = path
        .strip_prefix(TAR_INTEGRITY)
        .and_then(|rest| rest.strip_prefix('/'))
    {
        return is_new_package_path(rest);
    }
    if let Some(rest) = path.strip_prefix("default/") {
        return is_new_namespace_path(rest);
    }
    let Some((org, rest)) = path
        .strip_prefix("orgs/")
        .and_then(|rest| rest.split_once('/'))
    else {
        return false;
    };
    orgs.iter()
        .any(|configured| encode_segment(configured) == org)
        && is_new_namespace_path(rest)
}

fn is_new_namespace_path(rest: &str) -> bool {
    rest.starts_with('.') || is_new_package_path(rest)
}

fn is_new_package_path(rest: &str) -> bool {
    let components: Vec<&str> = rest.split('/').collect();
    let decodes = |segment: &str| decode_segment(segment).is_some();
    match components.as_slice() {
        ["packages", package, "document.json"] => decodes(package),
        ["packages", package, "blobs", blob] => decodes(package) && decodes(blob),
        _ => false,
    }
}

fn is_namespace_record(path: &str) -> bool {
    let top = path
        .split('/')
        .next()
        .unwrap_or_default();
    top == TOMBSTONES || top == REVISIONS || DROPPED.contains(&top)
}

/// The org whose namespace `path` is in, and the path within it.
fn split_namespace<'a>(path: &'a str, orgs: &BTreeSet<String>) -> Option<(&'a str, &'a str)> {
    if let Some((top, rest)) = path.split_once('/')
        && orgs.contains(top)
    {
        return Some((top, rest));
    }
    if path.starts_with(TAR_INTEGRITY) {
        return path.split_once('/');
    }
    Some(("", path))
}

fn namespace_step(prefix: &str, rest: &str, is_package: impl Fn(&str) -> bool) -> Step {
    let top = rest
        .split('/')
        .next()
        .unwrap_or_default();
    if DROPPED.contains(&top) {
        return Step::Remove;
    }
    if top == REVISIONS {
        return Step::Move { to: format!("{prefix}/{rest}"), document: false };
    }
    if top == TOMBSTONES {
        return tombstone_step(prefix, rest);
    }
    if top.starts_with('.') {
        return Step::Orphan;
    }
    package_step(prefix, rest, is_package)
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

fn package_step(prefix: &str, rest: &str, is_package: impl Fn(&str) -> bool) -> Step {
    let Some((package, file)) = rest.rsplit_once('/') else {
        return Step::Orphan;
    };
    if !is_package(package) || file.starts_with('.') {
        return Step::Orphan;
    }
    let package_dir = format!("{prefix}/packages/{}", encode_segment(package));
    if file == LEGACY_DOCUMENT {
        Step::Move { to: format!("{package_dir}/document.json"), document: true }
    } else {
        Step::Move { to: format!("{package_dir}/blobs/{}", encode_segment(file)), document: false }
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
