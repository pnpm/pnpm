//! The package a project declares of itself, as the environment installs
//! it. pnpm does not build the project: the package it installs points at
//! the source tree, the way an editable install does.

use super::{Manifest, Project};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pep508_rs::PackageName;
use pnpm_python_resolver::parse_requirement;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::{Component, Path, PathBuf},
    str::FromStr as _,
};

/// What a project declares of a package of its own.
pub(in super::super) enum ProjectPackage {
    /// No package: no build backend builds this project, or
    /// `tool.uv.package = false` opts out. Only its dependencies are
    /// installed.
    Virtual,
    /// A package pnpm cannot install without building the project, with
    /// the reason to report. Its dependencies are still installed.
    Unsupported(String),
    Installable(Box<OwnPackage>),
}

/// The project's own package as the environment installs it: a path entry
/// that makes the project's modules importable, plus the metadata an
/// installed distribution carries.
#[derive(Serialize)]
pub(in super::super) struct OwnPackage {
    /// The `.dist-info` directory, named as [PEP 427] escapes it.
    ///
    /// [PEP 427]: https://packaging.python.org/en/latest/specifications/binary-distribution-format/#escaping-and-unicode
    dist_info: String,
    /// The `.pth` file that puts `paths` on the environment's `sys.path`.
    pth: String,
    paths: Vec<PathBuf>,
    /// The project itself, recorded as the installed distribution's
    /// [PEP 610] source.
    ///
    /// [PEP 610]: https://packaging.python.org/en/latest/specifications/direct-url/
    directory: PathBuf,
    metadata: String,
    /// The `entry_points.txt` groups the project declares, by group name.
    /// `[project.scripts]` and `[project.gui-scripts]` are the two groups
    /// the environment also installs an executable for.
    entry_points: BTreeMap<String, BTreeMap<String, String>>,
}

/// Where the project's own modules live, or why no `sys.path` entry
/// names them.
enum SourcePaths {
    Directories(Vec<PathBuf>),
    Unnameable(String),
}

/// What the project declares of the interfaces it exposes: the two
/// script tables an install also puts on the PATH, and the
/// `[project.entry-points]` groups other tools discover the project by.
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) struct EntryPoints {
    #[serde(default)]
    scripts: BTreeMap<String, String>,
    #[serde(default)]
    gui_scripts: BTreeMap<String, String>,
    #[serde(default, rename = "entry-points")]
    groups: BTreeMap<String, BTreeMap<String, String>>,
}

/// The `[tool]` tables pnpm reads: whether the project declares a package
/// of its own, and where that package's modules live.
#[derive(Default, Deserialize)]
pub(super) struct Tool {
    uv: Option<Uv>,
    setuptools: Option<Setuptools>,
    hatch: Option<Hatch>,
}

#[derive(Deserialize)]
struct Uv {
    package: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Setuptools {
    package_dir: Option<BTreeMap<String, String>>,
}

#[derive(Deserialize)]
struct Hatch {
    build: Option<HatchBuild>,
}

#[derive(Deserialize)]
struct HatchBuild {
    packages: Option<Vec<String>>,
    targets: Option<HatchTargets>,
}

#[derive(Deserialize)]
struct HatchTargets {
    wheel: Option<HatchWheel>,
}

#[derive(Deserialize)]
struct HatchWheel {
    packages: Option<Vec<String>>,
}

impl Manifest {
    /// The package the project declares of itself, which the environment
    /// installs alongside the dependencies so that the project's own
    /// modules can be imported and its scripts run from it.
    pub(in super::super) fn project_package(&self, root: &Path) -> Result<ProjectPackage> {
        let Some(project) = &self.project else { return Ok(ProjectPackage::Virtual) };
        // A project cannot declare a distribution without naming it, so a
        // manifest that names none declares no package either.
        let Some(name) = &project.name else { return Ok(ProjectPackage::Virtual) };
        if !self.declares_package() {
            return Ok(ProjectPackage::Virtual);
        }
        let Some(version) = &project.version else {
            return Ok(ProjectPackage::Unsupported("its version is dynamic".to_string()));
        };
        let paths = match self.source_paths(root)? {
            SourcePaths::Directories(paths) => paths,
            SourcePaths::Unnameable(reason) => return Ok(ProjectPackage::Unsupported(reason)),
        };
        let name = PackageName::from_str(name)
            .into_diagnostic()
            .wrap_err("read the Python project name")?;
        let version: pep440_rs::Version = version
            .parse()
            .into_diagnostic()
            .wrap_err("read the Python project version")?;
        Ok(ProjectPackage::Installable(Box::new(OwnPackage {
            dist_info: format!(
                "{}-{}.dist-info",
                name.as_dist_info_name(),
                escape_version(&version.to_string()),
            ),
            pth: format!("__pnpm__{}.pth", name.as_dist_info_name()),
            paths,
            directory: root.to_path_buf(),
            metadata: project.core_metadata(&name, &version)?,
            entry_points: project.entry_points.all()?,
        })))
    }

    /// Whether the project declares a package at all: one a build backend
    /// builds, unless `tool.uv.package` says otherwise.
    fn declares_package(&self) -> bool {
        self.tool.uv
            .as_ref()
            .and_then(|uv| uv.package)
            .unwrap_or_else(|| self.build_system.is_some())
    }

    /// Where the project's own modules live. The build backend's package
    /// directories decide when it declares any; a project that declares
    /// none follows the `src` layout when it has a `src` directory, and
    /// keeps its modules beside the manifest otherwise.
    fn source_paths(&self, root: &Path) -> Result<SourcePaths> {
        if root
            .to_string_lossy()
            .contains(['\n', '\r'])
        {
            bail!(
                "pnpm cannot install a Python project whose own directory contains a line break: {}",
                root.display(),
            );
        }
        let declared = match self.tool.declared_source_dirs() {
            Some(Ok(declared)) => declared,
            Some(Err(reason)) => return Ok(SourcePaths::Unnameable(reason)),
            None => {
                let src = root.join("src");
                let path = if src.is_dir() { src } else { root.to_path_buf() };
                return Ok(SourcePaths::Directories(vec![path]));
            }
        };
        let mut paths = Vec::new();
        for directory in declared {
            let path = source_path(root, directory)?;
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
        Ok(SourcePaths::Directories(paths))
    }
}

impl Project {
    /// The [core metadata] an installed distribution of this project
    /// records. Every field is printed from its parsed form, so that no
    /// manifest string can write a header of its own.
    ///
    /// [core metadata]: https://packaging.python.org/en/latest/specifications/core-metadata/
    fn core_metadata(&self, name: &PackageName, version: &pep440_rs::Version) -> Result<String> {
        self.ensure_static_dependencies()?;
        let mut metadata = format!("Metadata-Version: 2.1\nName: {name}\nVersion: {version}\n");
        if let Some(requires_python) = &self.requires_python {
            let specifiers: pep440_rs::VersionSpecifiers =
                requires_python.parse().into_diagnostic()?;
            writeln!(metadata, "Requires-Python: {specifiers}").expect(
                "writing to a String cannot fail",
            );
        }
        for dependency in &self.dependencies {
            let requirement = parse_requirement(dependency)?;
            writeln!(metadata, "Requires-Dist: {requirement}").expect(
                "writing to a String cannot fail",
            );
        }
        Ok(metadata)
    }
}

impl EntryPoints {
    /// Every group an installed distribution records in
    /// `entry_points.txt`, under the names that format gives them.
    ///
    /// The two script groups have tables of their own, so PEP 621
    /// reserves their names in `[project.entry-points]`.
    fn all(&self) -> Result<BTreeMap<String, BTreeMap<String, String>>> {
        let mut groups = self.groups.clone();
        for (group, table, entries) in [
            ("console_scripts", "scripts", &self.scripts),
            ("gui_scripts", "gui-scripts", &self.gui_scripts),
        ] {
            if groups.contains_key(group) {
                bail!("declare [project.{table}] instead of [project.entry-points.{group}]");
            }
            if !entries.is_empty() {
                groups.insert(group.to_string(), entries.clone());
            }
        }
        Ok(groups)
    }
}

impl Tool {
    /// The directories to import the project's packages from, as the
    /// build backend's package declarations imply them. `None` when the
    /// backend declares none, and `Err` for a declaration no directory on
    /// `sys.path` can stand in for.
    fn declared_source_dirs(&self) -> Option<Result<Vec<&str>, String>> {
        if let Some(packages) = self.hatch_packages() {
            return Some(Ok(packages
                .iter()
                .map(|package| parent_dir(package))
                .collect()));
        }
        let package_dir = self.setuptools.as_ref()?.package_dir.as_ref()?;
        Some(
            package_dir
                .iter()
                .map(|(package, directory)| {
                    setuptools_source_dir(package, directory).ok_or_else(|| {
                        format!(
                            "[tool.setuptools] package-dir puts {package} in {directory}, which no directory on sys.path names",
                        )
                    })
                })
                .collect(),
        )
    }

    /// The packages hatchling builds the wheel from, by path.
    fn hatch_packages(&self) -> Option<&[String]> {
        let build = self.hatch.as_ref()?.build.as_ref()?;
        build.targets
            .as_ref()
            .and_then(|targets| targets.wheel.as_ref())
            .and_then(|wheel| wheel.packages.as_deref())
            .or(build.packages.as_deref())
    }
}

/// A version as [PEP 427] escapes it for a `.dist-info` directory name.
///
/// [PEP 427]: https://packaging.python.org/en/latest/specifications/binary-distribution-format/#escaping-and-unicode
fn escape_version(version: &str) -> String {
    let mut escaped = String::with_capacity(version.len());
    let mut previous_escaped = false;
    for character in version.chars() {
        if character.is_alphanumeric() || character == '.' || character == '_' {
            escaped.push(character);
            previous_escaped = false;
        } else if !previous_escaped {
            escaped.push('_');
            previous_escaped = true;
        }
    }
    escaped
}

/// The directory a [setuptools `package_dir`] entry maps a package into,
/// as a directory to import from. The entry names where the package's own
/// modules are, so it stands for a `sys.path` entry only when the package
/// sits under one by its own name.
///
/// [setuptools `package_dir`]: https://setuptools.pypa.io/en/latest/userguide/package_discovery.html#custom-discovery
fn setuptools_source_dir<'a>(package: &str, directory: &'a str) -> Option<&'a str> {
    if package.is_empty() {
        return Some(directory);
    }
    let parent = directory.strip_suffix(&package.replace('.', "/"))?;
    (parent.is_empty() || parent.ends_with('/')).then(|| parent.trim_end_matches('/'))
}

/// The directory a package path sits in, empty for one beside the
/// manifest.
fn parent_dir(package: &str) -> &str {
    package
        .rsplit_once(['/', '\\'])
        .map_or("", |(parent, _)| parent)
}

/// A directory the manifest declares, as a path under the project.
///
/// The environment names these directories one per line in a `.pth` file,
/// and Python executes a line of one that begins with `import`. A
/// directory carrying a line break, or reaching outside the project,
/// would make every interpreter run in that environment read code the
/// project does not own.
fn source_path(root: &Path, directory: &str) -> Result<PathBuf> {
    if directory.contains(['\n', '\r', '\0']) {
        bail!(
            "{} declares a Python package directory with a line break in it: {directory:?}",
            root.display(),
        );
    }
    let relative = Path::new(directory);
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        bail!(
            "{} declares a Python package directory outside the project: {directory}",
            root.display(),
        );
    }
    Ok(if directory.is_empty() { root.to_path_buf() } else { root.join(relative) })
}
