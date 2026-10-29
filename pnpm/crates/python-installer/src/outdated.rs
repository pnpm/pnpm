use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use miette::{IntoDiagnostic, Result, WrapErr};
use pep440_rs::Version;
use pep508_rs::{PackageName, Requirement, VersionOrUrl};
use pnpm_python_resolver::{WheelFilename, parse_requirement, source_version};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PythonDependencyGroup {
    Prod,
    Optional,
    Dev,
}

#[derive(Debug, Clone)]
pub struct DeclaredPythonDep {
    pub name: PackageName,
    pub requirement: Requirement,
    pub group: PythonDependencyGroup,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct RawPyproject {
    project: Option<RawProject>,
    #[serde(default)]
    dependency_groups: BTreeMap<String, Vec<toml::Value>>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct RawProject {
    #[serde(default)]
    dependencies: Vec<String>,
    #[serde(default)]
    optional_dependencies: BTreeMap<String, Vec<String>>,
}

#[derive(Deserialize)]
struct RawSimplePage {
    files: Vec<RawIndexFile>,
}

#[derive(Deserialize)]
struct RawIndexFile {
    filename: String,
    #[serde(default)]
    yanked: serde_json::Value,
}

#[derive(Deserialize)]
struct RawPylock {
    #[serde(default)]
    packages: Vec<RawLockedPackage>,
}

#[derive(Deserialize)]
struct RawLockedPackage {
    name: PackageName,
    version: Option<Version>,
    #[serde(default)]
    wheels: Vec<toml::Value>,
    sdist: Option<toml::Value>,
}

/// Direct index dependencies declared in `pyproject.toml`.
pub fn declared_dependencies(pyproject_path: &Path) -> Result<Vec<DeclaredPythonDep>> {
    if !pyproject_path.is_file() {
        return Ok(Vec::new());
    }
    let contents = fs::read_to_string(pyproject_path)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", pyproject_path.display()))?;
    let manifest = super::manifest::Manifest::parse(&contents)?;
    let non_index = non_index_sources(&manifest);
    let raw: RawPyproject = toml::from_str(&contents).into_diagnostic()?;
    collect_declared_deps(raw, &non_index)
}

fn non_index_sources(manifest: &super::manifest::Manifest) -> BTreeSet<PackageName> {
    manifest.tool.uv.sources
        .iter()
        .filter(|(_, decl)| {
            decl.sources()
                .iter()
                .any(|source| {
                    source.workspace
                        || source.path.is_some()
                        || source.git.is_some()
                        || source.url.is_some()
                })
        })
        .map(|(name, _)| name.clone())
        .collect()
}

fn collect_declared_deps(
    raw: RawPyproject,
    non_index: &BTreeSet<PackageName>,
) -> Result<Vec<DeclaredPythonDep>> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(project) = raw.project {
        push_group_deps(
            project.dependencies.iter().map(String::as_str),
            PythonDependencyGroup::Prod,
            non_index,
            &mut seen,
            &mut out,
        )?;
        push_group_deps(
            project.optional_dependencies
                .values()
                .flatten()
                .map(String::as_str),
            PythonDependencyGroup::Optional,
            non_index,
            &mut seen,
            &mut out,
        )?;
    }
    push_group_deps(
        raw.dependency_groups
            .values()
            .flatten()
            .filter_map(toml::Value::as_str),
        PythonDependencyGroup::Dev,
        non_index,
        &mut seen,
        &mut out,
    )?;
    Ok(out)
}

fn push_group_deps<'a>(
    entries: impl Iterator<Item = &'a str>,
    group: PythonDependencyGroup,
    non_index: &BTreeSet<PackageName>,
    seen: &mut BTreeSet<(PackageName, PythonDependencyGroup)>,
    out: &mut Vec<DeclaredPythonDep>,
) -> Result<()> {
    for raw_req in entries {
        let requirement = parse_requirement(raw_req)?;
        if non_index.contains(&requirement.name)
            || matches!(requirement.version_or_url, Some(VersionOrUrl::Url(_)))
        {
            continue;
        }
        let name = requirement.name.clone();
        if seen.insert((name.clone(), group)) {
            out.push(DeclaredPythonDep { name, requirement, group });
        }
    }
    Ok(())
}

/// The `[project].name` declared in `pyproject.toml`, if any.
#[must_use]
pub fn project_name(pyproject_path: &Path) -> Option<String> {
    let contents = fs::read_to_string(pyproject_path).ok()?;
    let manifest = super::manifest::Manifest::parse(&contents).ok()?;
    manifest.distribution().map(ToString::to_string)
}

/// The directory that owns `pylock.toml` for `project_dir`.
#[must_use]
pub fn lockfile_dir_for(workspace_root: Option<&Path>, project_dir: &Path) -> PathBuf {
    super::workspace::members::environment_root_of(workspace_root, project_dir)
}

/// Index-resolved package versions recorded in `pylock.toml`.
pub fn locked_versions(pylock_path: &Path) -> Result<BTreeMap<PackageName, Version>> {
    let contents = fs::read_to_string(pylock_path)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", pylock_path.display()))?;
    let lock: RawPylock = toml::from_str(&contents)
        .into_diagnostic()
        .wrap_err_with(|| format!("parse {}", pylock_path.display()))?;
    Ok(lock.packages
        .into_iter()
        .filter(|pkg| !pkg.wheels.is_empty() || pkg.sdist.is_some())
        .filter_map(|pkg| pkg.version.map(|version| (pkg.name, version)))
        .collect())
}

/// Query the configured Python Simple JSON API index for the newest version of
/// `requirement.name` (either within `requirement`'s version specifiers when
/// `compatible` is true, or the newest stable release overall).
pub async fn resolve_target_version(
    config: &pnpm_config::Config,
    http_client: &pnpm_network::ThrottledClient,
    requirement: &Requirement,
    compatible: bool,
) -> Result<Option<Version>> {
    let index = super::settings::python_index(config)?;
    let fetcher = super::registry::Fetcher { config, client: http_client, index: &index };
    let cached = fetcher.index_page(&requirement.name, config.offline).await?;
    if cached.missing {
        return Ok(None);
    }
    let versions = published_versions_from_page(cached.body.get(), &requirement.name)?;
    Ok(select_version(&versions, requirement, compatible))
}

fn published_versions_from_page(page: &str, name: &PackageName) -> Result<Vec<Version>> {
    let page: RawSimplePage = serde_json::from_str(page)
        .into_diagnostic()
        .wrap_err("Python index must support the Simple JSON API")?;
    let mut versions = BTreeSet::new();
    for file in page.files {
        if !matches!(file.yanked, serde_json::Value::Null | serde_json::Value::Bool(false)) {
            continue;
        }
        let version = match WheelFilename::parse(&file.filename) {
            Ok(Some(wheel)) if wheel.name == *name => Some(wheel.version),
            Ok(Some(_)) | Err(_) => None,
            Ok(None) => source_version(&file.filename, name).ok().flatten(),
        };
        if let Some(version) = version {
            versions.insert(version);
        }
    }
    Ok(versions.into_iter().collect())
}

fn select_version(
    versions: &[Version],
    requirement: &Requirement,
    compatible: bool,
) -> Option<Version> {
    let matching: Vec<&Version> = versions
        .iter()
        .filter(|version| !compatible || matches_specifier(requirement, version))
        .collect();
    matching
        .iter()
        .rev()
        .find(|version| !version.any_prerelease())
        .or_else(|| matching.last())
        .map(|version| (*version).clone())
}

fn matches_specifier(requirement: &Requirement, version: &Version) -> bool {
    match &requirement.version_or_url {
        Some(VersionOrUrl::VersionSpecifier(specifiers)) => specifiers.contains(version),
        Some(VersionOrUrl::Url(_)) => false,
        None => true,
    }
}
