mod metadata;

use miette::{IntoDiagnostic, Result, bail};
use pep440_rs::Version;
use pep508_rs::{PackageName, Requirement};
use pnpm_config::Config;
use pnpm_python_resolver::parse_requirement;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::Path,
};

#[derive(Clone, Copy)]
pub struct DependencySelection {
    pub production: bool,
    pub development: bool,
}

impl DependencySelection {
    pub const ALL: Self = Self { production: true, development: true };
}

#[derive(Clone, Deserialize)]
pub(super) struct Manifest {
    pub(super) project: Option<Project>,
    #[serde(skip)]
    pub(super) metadata: Option<super::host::WheelMetadata>,
    #[serde(skip)]
    pub(super) metadata_output: Option<std::sync::Arc<tempfile::TempDir>>,
    #[serde(default, rename = "dependency-groups")]
    groups: BTreeMap<String, Vec<toml::Value>>,
    #[serde(default, rename = "build-system")]
    pub(super) build_system: Option<BuildSystem>,
    #[serde(default)]
    pub(super) tool: Tool,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) struct Project {
    /// Absent only in a manifest that is not a distribution: PEP 621
    /// requires it of one that is.
    pub(super) name: Option<PackageName>,
    pub(super) version: Option<Version>,
    #[serde(default)]
    pub(super) dependencies: Vec<String>,
    #[serde(default)]
    pub(super) dynamic: Vec<String>,
    pub(super) requires_python: Option<String>,
    #[serde(default)]
    optional_dependencies: BTreeMap<String, Vec<String>>,
}

/// The PEP 517 backend that turns a project directory into a wheel.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) struct BuildSystem {
    #[serde(default)]
    pub(super) requires: Vec<String>,
    pub(super) build_backend: Option<String>,
    #[serde(default)]
    pub(super) backend_path: Vec<String>,
}

#[derive(Default, Clone, Deserialize)]
pub(super) struct Tool {
    #[serde(default)]
    pub(super) uv: Uv,
}

/// The parts of uv's table pnpm reads. A project that declares a
/// dependency on another project in the repository says so here, which is
/// where every Python workspace in the wild already writes it.
#[derive(Default, Clone, Deserialize)]
pub(super) struct Uv {
    #[serde(default)]
    pub(super) sources: BTreeMap<PackageName, SourceDeclaration>,
    /// Whether this project is built and installed at all. A project that
    /// sets this to `false` contributes its dependencies and nothing else.
    pub(super) package: Option<bool>,
    /// Present on the manifest that is a workspace root, naming which
    /// projects under it the workspace contains.
    pub(super) workspace: Option<UvWorkspace>,
}

#[derive(Clone, Deserialize)]
pub(super) struct UvWorkspace {
    #[serde(default)]
    pub(super) members: Vec<String>,
    #[serde(default)]
    pub(super) exclude: Vec<String>,
}

/// A source as it is written: one table, or several selected by markers.
#[derive(Clone, Deserialize)]
#[serde(untagged)]
pub(super) enum SourceDeclaration {
    One(Source),
    Many(Vec<Source>),
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) struct Source {
    #[serde(default)]
    pub(super) workspace: bool,
    pub(super) path: Option<String>,
    pub(super) editable: Option<bool>,
    pub(super) git: Option<String>,
    pub(super) url: Option<String>,
    pub(super) index: Option<String>,
    #[serde(flatten)]
    pub(super) narrowing: Narrowing,
}

/// What narrows a source to some targets, or to one extra or group. pnpm
/// reads these to refuse them: a source it applied everywhere would
/// install a project the manifest asked for somewhere else.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) struct Narrowing {
    marker: Option<String>,
    extra: Option<String>,
    group: Option<String>,
}

impl Narrowing {
    /// What this source is narrowed by, if anything.
    pub(super) fn kind(&self) -> Option<&'static str> {
        if self.marker.is_some() {
            return Some("conditional");
        }
        if self.extra.is_some() {
            return Some("extra-qualified");
        }
        self.group.is_some().then_some("group-qualified")
    }
}

impl Project {
    fn distribution_requirements(&self, static_only: bool) -> Result<Vec<String>> {
        let mut requirements = if static_only
            && self.dynamic
                .iter()
                .any(|field| field == "dependencies")
        {
            Vec::new()
        } else {
            self.dependencies.clone()
        };
        if static_only
            && self.dynamic
                .iter()
                .any(|field| field == "optional-dependencies")
        {
            return Ok(requirements);
        }
        for (extra, dependencies) in &self.optional_dependencies {
            for dependency in dependencies {
                requirements.push(super::workspace::requirement_for_extra(dependency, extra)?);
            }
        }
        Ok(requirements)
    }

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
}

impl Manifest {
    pub(super) fn set_requirements_file(&mut self, requirements: Vec<String>) {
        self.project = Some(Project {
            name: None,
            version: None,
            dependencies: requirements,
            dynamic: Vec::new(),
            requires_python: None,
            optional_dependencies: BTreeMap::new(),
        });
        self.tool.uv.package = Some(false);
    }

    pub(super) fn parse(contents: &str) -> Result<Self> {
        toml::from_str(contents).into_diagnostic()
    }

    pub(super) fn requirements(
        &self,
        config: &Config,
        selection: DependencySelection,
    ) -> Result<Vec<Requirement>> {
        let Some(project) = &self.project else { return Ok(Vec::new()) };
        if self.metadata.is_none() {
            project.ensure_static_dependencies()?;
        }
        let mut requirements =
            if selection.production { project.dependencies.clone() } else { Vec::new() };
        if self.metadata.is_some() && selection.production {
            requirements = self.metadata_requirements(config)?;
        }
        for extra in config.python.extras
            .iter()
            .filter(|_| selection.production && self.metadata.is_none())
        {
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

    /// Every distribution this project declares a requirement on,
    /// wherever it declares it. Which source satisfies a requirement is
    /// decided per distribution, so a name is reported once.
    pub(super) fn declared_distributions(&self) -> Result<BTreeSet<PackageName>> {
        let Some(project) = &self.project else { return Ok(BTreeSet::new()) };
        if self.metadata.is_none() {
            project.ensure_static_dependencies()?;
        }
        let metadata_requirements = self.metadata
            .as_ref()
            .map(|metadata| metadata.requires_dist.as_slice())
            .unwrap_or_default();
        let groups = self.groups
            .values()
            .flatten()
            .filter_map(toml::Value::as_str)
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        project.dependencies
            .iter()
            .chain(project.optional_dependencies.values().flatten())
            .chain(&groups)
            .chain(metadata_requirements)
            .map(|requirement| Ok(parse_requirement(requirement)?.name))
            .collect()
    }

    /// What a wheel built from this project would state in its
    /// `METADATA`: its requirements, and its extras' requirements under
    /// the marker that selects each extra. A dependency group is a
    /// development input, so a wheel does not carry it.
    pub(super) fn distribution_requirements(&self) -> Result<Vec<String>> {
        let Some(project) = &self.project else { return Ok(Vec::new()) };
        if self.metadata.is_none() {
            project.ensure_static_dependencies()?;
        }
        if let Some(metadata) = &self.metadata {
            return Ok(metadata.requires_dist.clone());
        }
        project.distribution_requirements(false)
    }

    /// The extras this project offers.
    pub(super) fn extras(&self) -> Vec<String> {
        if let Some(metadata) = &self.metadata {
            return metadata.provides_extra.clone();
        }
        self.project
            .iter()
            .flat_map(|project| project.optional_dependencies.keys())
            .cloned()
            .collect()
    }

    /// The distribution this manifest declares, if it declares one.
    pub(super) fn distribution(&self) -> Option<&PackageName> {
        self.project.as_ref()?.name.as_ref()
    }

    /// Whether a wheel is built from this project and installed into its
    /// own environment. A project that declares no build backend and does
    /// not ask to be packaged is a place to collect dependencies, not a
    /// distribution.
    pub(super) fn is_packaged(&self) -> bool {
        self.tool.uv.package.unwrap_or_else(|| self.build_system.is_some())
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
