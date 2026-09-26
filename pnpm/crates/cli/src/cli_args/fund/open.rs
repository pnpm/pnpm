//! `pnpm fund <package>` — open the funding URL of one installed package.

use super::{
    FundError,
    funding::{FundingSource, funding_sources},
    projects::ProjectDependencies,
    report::InstalledPackage,
};
use crate::cli_args::{deps_tree::DependencyNode, sanitize::sanitize_inline};
use pnpm_network_web_auth::OpenUrl;
use pnpm_package_manifest::safe_read_project_manifest_from_dir;
use pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency;
use serde_json::Value;
use std::{num::NonZeroUsize, path::Path};

pub fn open_package_funding<Sys: OpenUrl>(
    spec: &str,
    which: Option<NonZeroUsize>,
    dir: &Path,
    projects: &[ProjectDependencies],
) -> miette::Result<()> {
    let funding = package_funding(spec, dir, projects);
    let sources = funding
        .as_ref()
        .map(funding_sources)
        .unwrap_or_default();
    if sources.is_empty() {
        return Err(FundError::NoFunding { spec: spec.to_string() }.into());
    }
    let chosen = match which {
        Some(which) => sources.get(which.get() - 1),
        None => sources
            .first()
            .filter(|_| sources.len() == 1),
    };
    match chosen {
        Some(source) => open_source::<Sys>(source),
        None => println!("{}", ambiguous_sources_message(spec, which, &sources)),
    }
    Ok(())
}

/// The `funding` field of the package `spec` names: the project in a
/// directory for a path, else the newest installed version of a package
/// name. Like `npm fund`, a version in `spec` is ignored.
fn package_funding(spec: &str, dir: &Path, projects: &[ProjectDependencies]) -> Option<Value> {
    if is_directory_spec(spec) {
        let manifest = safe_read_project_manifest_from_dir(&dir.join(spec)).ok().flatten()?;
        return manifest.get("funding").cloned();
    }
    let name = parse_wanted_dependency(spec).alias.unwrap_or_else(|| spec.to_string());
    let mut newest: Option<(node_semver::Version, Option<Value>)> = None;
    for project in projects {
        visit_named(&project.dependencies, &name, &mut |package| {
            let Some(version) = package.version
                .as_deref()
                .and_then(|version| node_semver::Version::parse(version).ok())
            else {
                return;
            };
            if newest
                .as_ref()
                .is_none_or(|(newest, _)| version > *newest)
            {
                newest = Some((version, package.funding));
            }
        });
    }
    newest.and_then(|(_, funding)| funding)
}

fn is_directory_spec(spec: &str) -> bool {
    spec.starts_with('.') || spec.starts_with('/') || Path::new(spec).is_absolute()
}

fn visit_named(nodes: &[DependencyNode], name: &str, visit: &mut impl FnMut(InstalledPackage)) {
    for node in nodes {
        if node.package.name == name {
            visit(InstalledPackage::read(node));
        }
        visit_named(&node.dependencies, name, visit);
    }
}

fn open_source<Sys: OpenUrl>(source: &FundingSource<'_>) {
    let url = source.public_url();
    println!("{}:\n{}", source_title(source), sanitize_inline(&url));
    if let Err(error) = Sys::open_url(&url) {
        tracing::debug!(target: "pnpm_cli", %error, "could not open browser");
    }
}

fn source_title(source: &FundingSource<'_>) -> String {
    match source.kind {
        Some(kind) => format!("{} funding available at the following URL", sanitize_inline(kind)),
        None => "Funding available at the following URL".to_string(),
    }
}

/// The package's funding URLs, numbered for `--which`.
fn ambiguous_sources_message(
    spec: &str,
    which: Option<NonZeroUsize>,
    sources: &[FundingSource<'_>],
) -> String {
    let mut lines = Vec::with_capacity(sources.len() + 2);
    if let Some(which) = which {
        lines.push(format!("--which={which} is not a valid index"));
    }
    for (index, source) in sources.iter().enumerate() {
        lines.push(format!(
            "{}: {}: {}",
            index + 1,
            source_title(source),
            sanitize_inline(&source.public_url()),
        ));
    }
    lines.push(format!(
        "Run `pnpm fund {} --which=1`, for example, to open the first funding URL listed in that package",
        sanitize_inline(spec),
    ));
    lines.join("\n")
}
