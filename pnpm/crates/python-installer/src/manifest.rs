use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pep508_rs::{PackageName, Requirement};
use pnpm_config::Config;
use pnpm_python_resolver::parse_requirement;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::{Path, PathBuf},
    str::FromStr as _,
};

#[derive(Clone, Copy)]
pub struct DependencySelection {
    pub production: bool,
    pub development: bool,
}

impl DependencySelection {
    pub const ALL: Self = Self { production: true, development: true };
}

#[derive(Deserialize)]
pub(super) struct Manifest {
    pub(super) project: Option<Project>,
    #[serde(rename = "build-system")]
    build_system: Option<toml::Value>,
    #[serde(default, rename = "dependency-groups")]
    groups: BTreeMap<String, Vec<toml::Value>>,
    #[serde(default)]
    tool: Tool,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) struct Project {
    name: Option<String>,
    version: Option<String>,
    #[serde(default)]
    pub(super) dependencies: Vec<String>,
    #[serde(default)]
    dynamic: Vec<String>,
    pub(super) requires_python: Option<String>,
    #[serde(default)]
    optional_dependencies: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    scripts: BTreeMap<String, String>,
    #[serde(default)]
    gui_scripts: BTreeMap<String, String>,
    #[serde(default)]
    entry_points: BTreeMap<String, BTreeMap<String, String>>,
}

/// The `[tool]` tables pnpm reads: whether the project declares a package
/// of its own, and where that package's modules live.
#[derive(Default, Deserialize)]
struct Tool {
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

/// What a project declares of a package of its own.
pub(super) enum ProjectPackage {
    /// No package: no build backend builds this project, or
    /// `tool.uv.package = false` opts out. Only its dependencies are
    /// installed.
    Virtual,
    /// A package whose version only a build backend knows, which pnpm
    /// cannot record without building the project.
    DynamicVersion,
    Installable(Box<OwnPackage>),
}

/// The project's own package as the environment installs it: a path entry
/// that makes the project's modules importable, plus the metadata an
/// installed distribution carries.
#[derive(Serialize)]
pub(super) struct OwnPackage {
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

impl Project {
    fn ensure_static_dependencies(&self) -> Result<()> {
        if self.dynamic
            .iter()
            .any(|field| {
                matches!(
                    field.as_str(),
                    "dependencies" | "optional-dependencies" | "requires-python",
                )
            })
        {
            bail!("pnpm Python integration requires static dependency metadata in pyproject.toml");
        }
        Ok(())
    }

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

impl Tool {
    /// The directories the build backend takes the project's packages
    /// from, when it declares them in a form pnpm reads.
    fn declared_source_dirs(&self) -> Option<Vec<&str>> {
        self.hatch_source_dirs().or_else(|| self.setuptools_source_dirs())
    }

    /// Hatchling names each package by its path, so the directory to
    /// import it from is the path's parent.
    fn hatch_source_dirs(&self) -> Option<Vec<&str>> {
        let build = self.hatch.as_ref()?.build.as_ref()?;
        let packages = build.targets
            .as_ref()
            .and_then(|targets| targets.wheel.as_ref())
            .and_then(|wheel| wheel.packages.as_deref())
            .or(build.packages.as_deref())?;
        Some(
            packages
                .iter()
                .map(|package| {
                    package
                        .rsplit_once(['/', '\\'])
                        .map_or("", |(parent, _)| parent)
                })
                .collect(),
        )
    }

    fn setuptools_source_dirs(&self) -> Option<Vec<&str>> {
        Some(
            self.setuptools
                .as_ref()?
                .package_dir
                .as_ref()?
                .values()
                .map(String::as_str)
                .collect(),
        )
    }
}

impl Manifest {
    pub(super) fn parse(contents: &str) -> Result<Self> {
        toml::from_str(contents).into_diagnostic()
    }

    pub(super) fn requirements(
        &self,
        config: &Config,
        selection: DependencySelection,
    ) -> Result<Vec<Requirement>> {
        let Some(project) = &self.project else { return Ok(Vec::new()) };
        project.ensure_static_dependencies()?;
        let mut requirements =
            if selection.production { project.dependencies.clone() } else { Vec::new() };
        for extra in config.python.extras.iter().filter(|_| selection.production) {
            let dependencies = project.optional_dependencies
                .get(extra)
                .ok_or_else(|| miette::miette!("unknown Python project extra: {extra}"))?;
            requirements.extend(dependencies.iter().cloned());
        }
        if selection.development {
            self.expand_configured_groups(config, &mut requirements)?;
        }
        requirements
            .into_iter()
            .map(|requirement| parse_requirement(&requirement))
            .collect()
    }

    /// The package the project declares of itself, which the environment
    /// installs alongside the dependencies so that the project's own
    /// modules can be imported and its scripts run from it.
    ///
    /// pnpm does not build the project: the package it installs points at
    /// the source tree, the way an editable install does.
    pub(super) fn project_package(&self, root: &Path) -> Result<ProjectPackage> {
        let Some(project) = &self.project else { return Ok(ProjectPackage::Virtual) };
        // A project cannot declare a distribution without naming it, so a
        // manifest that names none declares no package either.
        let Some(name) = &project.name else { return Ok(ProjectPackage::Virtual) };
        if !self.declares_package() {
            return Ok(ProjectPackage::Virtual);
        }
        let Some(version) = &project.version else { return Ok(ProjectPackage::DynamicVersion) };
        let name = PackageName::from_str(name)
            .into_diagnostic()
            .wrap_err("read the Python project name")?;
        let version: pep440_rs::Version = version
            .parse()
            .into_diagnostic()
            .wrap_err("read the Python project version")?;
        let mut entry_points = project.entry_points.clone();
        for (group, entries) in
            [("console_scripts", &project.scripts), ("gui_scripts", &project.gui_scripts)]
        {
            if !entries.is_empty() {
                entry_points.insert(group.to_string(), entries.clone());
            }
        }
        Ok(ProjectPackage::Installable(Box::new(OwnPackage {
            dist_info: format!(
                "{}-{}.dist-info",
                name.as_dist_info_name(),
                escape_version(&version.to_string()),
            ),
            pth: format!("__pnpm__{}.pth", name.as_dist_info_name()),
            paths: self.source_paths(root),
            directory: root.to_path_buf(),
            metadata: project.core_metadata(&name, &version)?,
            entry_points,
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
    fn source_paths(&self, root: &Path) -> Vec<PathBuf> {
        let Some(declared) = self.tool.declared_source_dirs() else {
            let src = root.join("src");
            return vec![if src.is_dir() { src } else { root.to_path_buf() }];
        };
        let mut paths = Vec::new();
        for directory in declared {
            let path = if directory.is_empty() { root.to_path_buf() } else { root.join(directory) };
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
        paths
    }

    /// Expand every dependency group the config asks for. The `dev` group
    /// is asked for by default, so a project that declares no such group
    /// simply has none.
    fn expand_configured_groups(
        &self,
        config: &Config,
        requirements: &mut Vec<String>,
    ) -> Result<()> {
        for group in &config.python.groups {
            if group == "dev" && !self.groups.contains_key(group) {
                continue;
            }
            self.expand_group(group, &mut Vec::new(), requirements)?;
        }
        Ok(())
    }

    fn expand_group(
        &self,
        group: &str,
        visiting: &mut Vec<String>,
        requirements: &mut Vec<String>,
    ) -> Result<()> {
        if visiting
            .iter()
            .any(|name| name == group)
        {
            bail!("cyclic Python dependency group: {group}");
        }
        let entries = self.groups
            .get(group)
            .ok_or_else(|| miette::miette!("unknown Python dependency group: {group}"))?;
        visiting.push(group.to_string());
        for entry in entries {
            if let Some(requirement) = entry.as_str() {
                requirements.push(requirement.to_string());
            } else if let Some(table) = entry.as_table()
                && table.len() == 1
                && let Some(include) = table.get("include-group").and_then(toml::Value::as_str)
            {
                self.expand_group(include, visiting, requirements)?;
            } else {
                bail!("invalid entry in Python dependency group {group}");
            }
        }
        visiting.pop();
        Ok(())
    }
}

pub(crate) fn add(path: &Path, requirements: &[String], development: bool) -> Result<()> {
    let original = std::fs::read_to_string(path).into_diagnostic()?;
    let parsed = Manifest::parse(&original)?;
    let Some(project) = &parsed.project else {
        bail!("{} has no [project] table", path.display());
    };
    project.ensure_static_dependencies()?;
    let document: BTreeMap<String, TomlTable> = toml::from_str(&original).into_diagnostic()?;
    let (table_name, key) =
        if development { ("dependency-groups", "dev") } else { ("project", "dependencies") };
    let table = document.get(table_name);
    let existing_array = table.and_then(|table| table.get_ref().get(key));
    let mut entries = existing_array
        .map(|array| {
            array
                .get_ref()
                .as_array()
                .cloned()
                .ok_or_else(|| miette::miette!("Python dependencies must be an array"))
        })
        .transpose()?
        .unwrap_or_default();
    merge_requirements(&mut entries, requirements)?;
    let array = toml::Value::Array(entries).to_string();
    let mut updated = original.clone();
    if let Some(existing) = existing_array {
        updated.replace_range(existing.span(), &array);
    } else if let Some(table) = table {
        insert_key_into_table(&mut updated, &original, table, (table_name, key), &array)?;
    } else {
        writeln!(updated, "\n[{table_name}]\n{key} = {array}").expect(
            "writing to a String cannot fail",
        );
    }
    Manifest::parse(&updated)?;
    pnpm_fs::write_atomic(path, updated.as_bytes()).into_diagnostic()
}

/// One table of the manifest, with the span it occupies in the source.
type TomlTable = toml::Spanned<BTreeMap<String, toml::Spanned<toml::Value>>>;

/// Add each requirement to `entries`, replacing an entry that already
/// names the same package rather than declaring it twice.
fn merge_requirements(entries: &mut Vec<toml::Value>, requirements: &[String]) -> Result<()> {
    for requirement in requirements {
        let parsed = parse_requirement(requirement)?;
        let existing = entries
            .iter()
            .position(|entry| {
                entry
                    .as_str()
                    .and_then(|value| value.parse::<Requirement>().ok())
                    .is_some_and(|entry| entry.name == parsed.name)
            });
        if let Some(index) = existing {
            entries[index] = toml::Value::String(requirement.clone());
        } else {
            entries.push(toml::Value::String(requirement.clone()));
        }
    }
    Ok(())
}

/// Write `key = <array>` into a table that does not declare it yet,
/// following the table's own TOML representation.
fn insert_key_into_table(
    updated: &mut String,
    original: &str,
    table: &TomlTable,
    (table_name, key): (&str, &str),
    array: &str,
) -> Result<()> {
    let span = table.span();
    if original[span.clone()].trim_start().starts_with('[') {
        let end = original[span.end..]
            .find('\n')
            .map_or(original.len(), |end| span.end + end + 1);
        updated.insert_str(end, &format!("\n{key} = {array}\n"));
        return Ok(());
    }
    if !original[span.clone()].trim_start().starts_with('{') {
        bail!("cannot add {table_name}.{key} to this TOML table representation");
    }
    let separator = if table.get_ref().is_empty() { "" } else { "," };
    updated.insert_str(span.end - 1, &format!("{separator} {key} = {array}"));
    Ok(())
}
