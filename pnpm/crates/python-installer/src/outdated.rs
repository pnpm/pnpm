use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

use miette::{IntoDiagnostic, Result, WrapErr};
use pep440_rs::Version;
use pep508_rs::{MarkerTree, PackageName, Requirement, VersionOrUrl};
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
    environments: Vec<String>,
    #[serde(default)]
    packages: Vec<RawLockedPackage>,
}

#[derive(Deserialize)]
struct RawLockedPackage {
    name: PackageName,
    version: Option<Version>,
    marker: Option<String>,
    #[serde(default)]
    wheels: Vec<toml::Value>,
    sdist: Option<toml::Value>,
}

/// Marker-aware index of package versions recorded in a PEP 751 `pylock.toml`.
pub struct LockedPythonPackages {
    by_name: BTreeMap<PackageName, Vec<(Version, MarkerTree)>>,
}

#[derive(Debug, Clone)]
pub enum TargetPyproject {
    SharedWorkspace,
    Project(Vec<DeclaredPythonDep>),
}

/// Inspect `pyproject.toml` once for `pnpm update` target classification:
/// shared-environment workspace roots require member expansion, `[project]`
/// manifests yield their declared index dependencies, and virtual unshared
/// workspace roots yield `None`.
pub fn inspect_target_pyproject(
    pyproject_path: &Path,
    workspace_root: Option<&Path>,
) -> Result<Option<TargetPyproject>> {
    if !pyproject_path.is_file() {
        return Ok(None);
    }
    let contents = read_manifest_text(pyproject_path)?;
    let manifest = super::manifest::Manifest::parse(&contents)?;
    if manifest.shares_environment() {
        return Ok(Some(TargetPyproject::SharedWorkspace));
    }
    if manifest.project.is_none() {
        return Ok(None);
    }
    let non_index = non_index_sources(pyproject_path, workspace_root, &manifest);
    Ok(Some(TargetPyproject::Project(declared_from_contents(&contents, &non_index)?)))
}

/// Direct index dependencies declared in `pyproject.toml`.
pub fn declared_dependencies(
    pyproject_path: &Path,
    workspace_root: Option<&Path>,
) -> Result<Vec<DeclaredPythonDep>> {
    if !pyproject_path.is_file() {
        return Ok(Vec::new());
    }
    let contents = read_manifest_text(pyproject_path)?;
    let manifest = super::manifest::Manifest::parse(&contents)?;
    let non_index = non_index_sources(pyproject_path, workspace_root, &manifest);
    declared_from_contents(&contents, &non_index)
}

pub(crate) fn declared_dependencies_in_workspace(
    pyproject_path: &Path,
    workspace: &super::workspace::Workspace,
) -> Result<Vec<DeclaredPythonDep>> {
    if !pyproject_path.is_file() {
        return Ok(Vec::new());
    }
    let contents = read_manifest_text(pyproject_path)?;
    let manifest = super::manifest::Manifest::parse(&contents)?;
    let non_index = pyproject_path
        .parent()
        .map_or_else(BTreeSet::new, |dir| workspace.non_index_sources(dir, &manifest));
    declared_from_contents(&contents, &non_index)
}

fn read_manifest_text(pyproject_path: &Path) -> Result<String> {
    fs::read_to_string(pyproject_path)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", pyproject_path.display()))
}

fn declared_from_contents(
    contents: &str,
    non_index: &BTreeSet<PackageName>,
) -> Result<Vec<DeclaredPythonDep>> {
    let raw: RawPyproject = toml::from_str(contents).into_diagnostic()?;
    collect_declared_deps(raw, non_index)
}

fn non_index_sources(
    pyproject_path: &Path,
    workspace_root: Option<&Path>,
    manifest: &super::manifest::Manifest,
) -> BTreeSet<PackageName> {
    let inherited = if manifest.tool.uv.workspace.is_none() {
        pyproject_path
            .parent()
            .and_then(|project_dir| {
                super::workspace::members::inherited_workspace_manifest(workspace_root, project_dir)
            })
    } else {
        None
    };
    manifest.non_index_sources(inherited.as_ref())
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
    seen: &mut BTreeSet<(PackageName, PythonDependencyGroup, MarkerTree)>,
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
        if seen.insert((name.clone(), group, requirement.marker.clone())) {
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

impl LockedPythonPackages {
    /// Parse index-backed package versions and markers from `pylock.toml`.
    pub fn read(pylock_path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(pylock_path)
            .into_diagnostic()
            .wrap_err_with(|| format!("read {}", pylock_path.display()))?;
        let lock: RawPylock = toml::from_str(&contents)
            .into_diagnostic()
            .wrap_err_with(|| format!("parse {}", pylock_path.display()))?;
        let scope = parse_environment_scope(&lock.environments)?;
        let mut by_name: BTreeMap<PackageName, Vec<(Version, MarkerTree)>> = BTreeMap::new();
        for pkg in lock.packages {
            if pkg.wheels.is_empty() && pkg.sdist.is_none() {
                continue;
            }
            let Some(version) = pkg.version else {
                continue;
            };
            let mut active = scope.clone();
            if let Some(raw_marker) = pkg.marker {
                let parsed: MarkerTree = raw_marker.parse().into_diagnostic()?;
                active.and(parsed);
            }
            by_name
                .entry(pkg.name)
                .or_default()
                .push((version, active));
        }
        Ok(Self { by_name })
    }

    /// Distinct locked versions across every environment overlapping
    /// `requirement`, in ascending version order.
    #[must_use]
    pub fn matching_versions(&self, requirement: &Requirement) -> Vec<&Version> {
        let Some(entries) = self.by_name.get(&requirement.name) else {
            return Vec::new();
        };
        let wanted = requirement.marker.clone().simplify_extras_with(|_| true);
        let distinct: BTreeSet<&Version> = entries
            .iter()
            .filter(|(_, active)| !active.is_disjoint(&wanted))
            .map(|(version, _)| version)
            .collect();
        distinct.into_iter().collect()
    }
}

fn parse_environment_scope(environments: &[String]) -> Result<MarkerTree> {
    if environments.is_empty() {
        return Ok(MarkerTree::TRUE);
    }
    let mut scope = MarkerTree::FALSE;
    for env in environments {
        let parsed: MarkerTree = env.parse().into_diagnostic()?;
        scope.or(parsed);
    }
    Ok(scope)
}

type CachedVersionsCell = Arc<tokio::sync::OnceCell<Option<Vec<Version>>>>;

/// Per-run cache of published versions from the Python Simple JSON API, so
/// checking compatible and latest targets, divergent platform locks, or
/// workspace members fetches and parses each package's index page once.
#[derive(Default)]
pub struct PythonIndexCache {
    by_name: Mutex<BTreeMap<PackageName, CachedVersionsCell>>,
}

impl PythonIndexCache {
    pub async fn resolve_target_version(
        &self,
        config: &pnpm_config::Config,
        http_client: &pnpm_network::ThrottledClient,
        requirement: &Requirement,
        compatible: bool,
    ) -> Result<Option<Version>> {
        let cell = {
            let mut guard = self.by_name.lock().unwrap_or_else(PoisonError::into_inner);
            Arc::clone(guard.entry(requirement.name.clone()).or_default())
        };
        let versions = cell.get_or_try_init(|| {
            fetch_published_versions(config, http_client, &requirement.name)
        })
        .await?;
        let Some(versions) = versions.as_ref() else {
            return Ok(None);
        };
        Ok(select_version(versions, requirement, compatible))
    }
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
    PythonIndexCache::default().resolve_target_version(config, http_client, requirement, compatible)
        .await
}

async fn fetch_published_versions(
    config: &pnpm_config::Config,
    http_client: &pnpm_network::ThrottledClient,
    name: &PackageName,
) -> Result<Option<Vec<Version>>> {
    let index = super::settings::python_index(config)?;
    let fetcher = super::registry::Fetcher { config, client: http_client, index: &index };
    let cached = fetcher.index_page(name, config.offline).await?;
    if cached.missing {
        return Ok(None);
    }
    published_versions_from_page(cached.body.get(), name).map(Some)
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
