use super::recursive::discover_workspace_projects;
use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::Diagnostic;
use pnpm_catalogs_config::get_catalogs_from_workspace_manifest;
use pnpm_catalogs_protocol_parser::parse_catalog_protocol;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_matcher::{Matcher, create_matcher};
use pnpm_package_manifest::{PackageManifest, PackageManifestError};
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use pnpm_versioning::{IntentBumpType, format_change_intent};
use pnpm_workspace::{
    ReadProjectManifestOnlyError, ReadWorkspaceManifestError, read_workspace_manifest,
    safe_read_project_manifest_only,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    fs::OpenOptions,
    io,
    io::{ErrorKind, Write as _},
    path::{Path, PathBuf},
};

#[derive(Debug, Display, Error, Diagnostic)]
enum UpdateChangesetError {
    #[display("Failed to read project manifest: {_0}")]
    #[diagnostic(transparent)]
    ReadProject(#[error(source)] ReadProjectManifestOnlyError),

    #[display("Failed to inspect project manifest: {_0}")]
    #[diagnostic(transparent)]
    InspectProject(#[error(source)] PackageManifestError),

    #[display("Failed to read pnpm-workspace.yaml: {_0}")]
    #[diagnostic(transparent)]
    ReadWorkspace(#[error(source)] ReadWorkspaceManifestError),

    #[display("Failed to read {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_INVALID_CHANGESET_CONFIG))]
    ReadConfig {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to parse {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_INVALID_CHANGESET_CONFIG))]
    ParseConfig {
        path: PathBuf,
        #[error(source)]
        source: serde_json::Error,
    },

    #[display("Failed to inspect changeset directory at {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_UNSAFE_CHANGESET_DIR))]
    InspectChangesetDir {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display(
        "Refusing to use changeset directory at {} because it is a symlink or not a directory",
        path.display()
    )]
    #[diagnostic(code(ERR_PNPM_UNSAFE_CHANGESET_DIR))]
    UnsafeChangesetDir { path: PathBuf },

    #[display("Failed to generate a changeset ID: {source}")]
    #[diagnostic(code(ERR_PNPM_CHANGESET_ID_FAILED))]
    GenerateId {
        #[error(source)]
        source: getrandom::Error,
    },

    #[display("Failed to write {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_CHANGESET_WRITE_FAILED))]
    WriteChangeset {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },
}

#[derive(Default, PartialEq, Eq)]
struct UpdateDepSpecs {
    dependencies: Option<BTreeMap<String, String>>,
    optional_dependencies: Option<BTreeMap<String, String>>,
    peer_dependencies: Option<BTreeMap<String, String>>,
}

impl UpdateDepSpecs {
    fn from_manifest(manifest: &PackageManifest) -> Result<Self, PackageManifestError> {
        let value = manifest.written_value()?;
        Ok(Self {
            dependencies: dependency_map(&value, "dependencies"),
            optional_dependencies: dependency_map(&value, "optionalDependencies"),
            peer_dependencies: dependency_map(&value, "peerDependencies"),
        })
    }

    fn production_groups(&self) -> [Option<&BTreeMap<String, String>>; 2] {
        [self.dependencies.as_ref(), self.optional_dependencies.as_ref()]
    }
}

pub(super) struct UpdateChangesetContext {
    workspace_dir: PathBuf,
    root_dirs: Vec<PathBuf>,
    dep_specs_before: BTreeMap<PathBuf, Option<UpdateDepSpecs>>,
    catalogs_before: Catalogs,
}

impl UpdateChangesetContext {
    pub(super) fn capture(config: &Config, manifest_path: &Path) -> miette::Result<Self> {
        let project_dir = manifest_path.parent().expect("manifest path always has a parent dir");
        let workspace_dir = config.workspace_dir.as_deref().unwrap_or(project_dir).to_path_buf();
        let root_dirs = if config.workspace_dir.is_some() {
            let (projects, _) = discover_workspace_projects(&workspace_dir, config)?;
            let dirs = projects.into_iter().map(|project| project.root_dir).collect::<Vec<_>>();
            if dirs.is_empty() { vec![project_dir.to_path_buf()] } else { dirs }
        } else {
            vec![project_dir.to_path_buf()]
        };
        let dep_specs_before = root_dirs
            .iter()
            .map(|root_dir| {
                let manifest = safe_read_project_manifest_only(root_dir)
                    .map_err(UpdateChangesetError::ReadProject)?;
                let specs = manifest
                    .as_ref()
                    .map(UpdateDepSpecs::from_manifest)
                    .transpose()
                    .map_err(UpdateChangesetError::InspectProject)?;
                Ok((root_dir.clone(), specs))
            })
            .collect::<Result<_, UpdateChangesetError>>()?;
        let workspace_manifest =
            read_workspace_manifest(&workspace_dir).map_err(UpdateChangesetError::ReadWorkspace)?;
        let catalogs_before = get_catalogs_from_workspace_manifest(workspace_manifest.as_ref())?;
        Ok(Self { workspace_dir, root_dirs, dep_specs_before, catalogs_before })
    }

    pub(super) fn generate<Output: Reporter>(self) -> miette::Result<()> {
        let changeset_dir = self.workspace_dir.join(".changeset");
        ensure_changeset_dir_is_safe(&changeset_dir)?;
        let config_path = changeset_dir.join("config.json");
        let Some(ignored) = read_ignored_matcher(&config_path)? else {
            global_log::<Output>(
                LogLevel::Warn,
                format!(
                    "No changeset was generated because {} does not exist",
                    config_path.display(),
                ),
            );
            return Ok(());
        };

        let workspace_manifest = read_workspace_manifest(&self.workspace_dir)
            .map_err(UpdateChangesetError::ReadWorkspace)?;
        let catalogs_after = get_catalogs_from_workspace_manifest(workspace_manifest.as_ref())?;
        let changed_catalog_entries =
            find_changed_catalog_entries(&self.catalogs_before, &catalogs_after);
        let releases = self.collect_releases(&ignored, &changed_catalog_entries)?;
        if releases.is_empty() {
            global_log::<Output>(
                LogLevel::Info,
                "No changeset was generated because the update did not change the production or peer dependencies of any workspace package".to_string(),
            );
            return Ok(());
        }

        let releases = releases.into_iter().collect::<IndexMap<_, _>>();
        ensure_changeset_dir_is_safe(&changeset_dir)?;
        let content = format_change_intent(&releases, "Update dependencies.");
        let changeset_path = write_changeset(&changeset_dir, &content)?;
        report_generated_changeset::<Output>(&changeset_path, &releases);
        Ok(())
    }

    fn collect_releases(
        &self,
        ignored: &Matcher,
        changed_catalog_entries: &BTreeMap<String, BTreeSet<String>>,
    ) -> Result<BTreeMap<String, IntentBumpType>, UpdateChangesetError> {
        let mut releases = BTreeMap::new();
        for root_dir in &self.root_dirs {
            if let Some((package_name, bump)) =
                self.project_release(root_dir, ignored, changed_catalog_entries)?
            {
                releases.insert(package_name, bump);
            }
        }
        Ok(releases)
    }
    /// The bump a project needs, when the update changed a dependency its
    /// consumers can observe. A private or ignored package never releases.
    fn project_release(
        &self,
        root_dir: &Path,
        ignored: &Matcher,
        changed_catalog_entries: &BTreeMap<String, BTreeSet<String>>,
    ) -> Result<Option<(String, IntentBumpType)>, UpdateChangesetError> {
        let Some(manifest) =
            safe_read_project_manifest_only(root_dir).map_err(UpdateChangesetError::ReadProject)?
        else {
            return Ok(None);
        };
        let Some(package_name) = manifest.value().get("name").and_then(Value::as_str) else {
            return Ok(None);
        };
        if manifest.value().get("private").and_then(Value::as_bool) == Some(true)
            || ignored.matches(package_name)
        {
            return Ok(None);
        }
        let dep_specs = UpdateDepSpecs::from_manifest(&manifest)
            .map_err(UpdateChangesetError::InspectProject)?;
        let dep_specs_before = self.dep_specs_before.get(root_dir).and_then(Option::as_ref);
        let peer_dependencies_changed = dep_specs_before
            .is_some_and(|before| before.peer_dependencies != dep_specs.peer_dependencies)
            || uses_changed_catalog_entry(
                [dep_specs.peer_dependencies.as_ref()],
                changed_catalog_entries,
            );
        if peer_dependencies_changed {
            return Ok(Some((package_name.to_string(), IntentBumpType::Major)));
        }
        let production_dependencies_changed = dep_specs_before.is_none_or(|before| {
            before.dependencies != dep_specs.dependencies
                || before.optional_dependencies != dep_specs.optional_dependencies
        }) || uses_changed_catalog_entry(
            dep_specs.production_groups(),
            changed_catalog_entries,
        );
        Ok(production_dependencies_changed
            .then(|| (package_name.to_string(), IntentBumpType::Patch)))
    }
}

/// The matcher over the changeset config's `ignore` list, or `None` when
/// there is no config file.
fn read_ignored_matcher(config_path: &Path) -> Result<Option<Matcher>, UpdateChangesetError> {
    let config_text = match fs::read_to_string(config_path) {
        Ok(config_text) => config_text,
        Err(source) if source.kind() == ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(UpdateChangesetError::ReadConfig {
                path: config_path.to_path_buf(),
                source,
            });
        }
    };
    let config: Value = serde_json::from_str(&config_text).map_err(|source| {
        UpdateChangesetError::ParseConfig { path: config_path.to_path_buf(), source }
    })?;
    let declared = config.get("ignore").and_then(Value::as_array).into_iter().flatten();
    let ignore_patterns =
        declared.filter_map(Value::as_str).map(str::to_string).collect::<Vec<_>>();
    Ok(Some(create_matcher(&ignore_patterns)))
}

/// Write `content` to a changeset file under a freshly generated id,
/// retrying until the name is one no file already holds.
fn write_changeset(changeset_dir: &Path, content: &str) -> Result<PathBuf, UpdateChangesetError> {
    loop {
        let mut random = [0_u8; 4];
        getrandom::fill(&mut random)
            .map_err(|source| UpdateChangesetError::GenerateId { source })?;
        let id = format!("pnpm-update-{:08x}", u32::from_be_bytes(random));
        let changeset_path = changeset_dir.join(format!("{id}.md"));
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&changeset_path) {
            Ok(file) => file,
            Err(source) if source.kind() == ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(UpdateChangesetError::WriteChangeset { path: changeset_path, source });
            }
        };
        file.write_all(content.as_bytes()).map_err(|source| {
            UpdateChangesetError::WriteChangeset { path: changeset_path.clone(), source }
        })?;
        break Ok(changeset_path);
    }
}

fn ensure_changeset_dir_is_safe(changeset_dir: &Path) -> Result<(), UpdateChangesetError> {
    let metadata = match fs::symlink_metadata(changeset_dir) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(UpdateChangesetError::InspectChangesetDir {
                path: changeset_dir.to_path_buf(),
                source,
            });
        }
    };
    if metadata.file_type().is_symlink()
        || pnpm_fs::read_symlink_dir(changeset_dir).is_ok()
        || !metadata.is_dir()
    {
        return Err(UpdateChangesetError::UnsafeChangesetDir { path: changeset_dir.to_path_buf() });
    }
    Ok(())
}

fn dependency_map(value: &Value, field: &str) -> Option<BTreeMap<String, String>> {
    value.get(field).and_then(Value::as_object).map(|dependencies| {
        dependencies
            .iter()
            .filter_map(|(name, spec)| spec.as_str().map(|spec| (name.clone(), spec.to_string())))
            .collect()
    })
}

fn find_changed_catalog_entries(
    before: &Catalogs,
    after: &Catalogs,
) -> BTreeMap<String, BTreeSet<String>> {
    let catalog_names = before.keys().chain(after.keys()).collect::<BTreeSet<_>>();
    catalog_names
        .into_iter()
        .filter_map(|catalog_name| {
            let changed = changed_catalog_entries(before, after, catalog_name);
            (!changed.is_empty()).then(|| (catalog_name.clone(), changed))
        })
        .collect()
}

/// The entries of `catalog_name` whose specifier differs between the two
/// catalog sets, counting an entry only one side declares.
fn changed_catalog_entries(
    before: &Catalogs,
    after: &Catalogs,
    catalog_name: &str,
) -> BTreeSet<String> {
    let declared_before = before.get(catalog_name).into_iter().flatten().map(|(name, _)| name);
    let declared_after = after.get(catalog_name).into_iter().flatten().map(|(name, _)| name);
    let declared = declared_before.chain(declared_after).collect::<BTreeSet<_>>();
    declared
        .into_iter()
        .filter(|dependency_name| {
            before.get(catalog_name).and_then(|catalog| catalog.get(*dependency_name))
                != after.get(catalog_name).and_then(|catalog| catalog.get(*dependency_name))
        })
        .cloned()
        .collect()
}

fn uses_changed_catalog_entry<'a>(
    dependency_groups: impl IntoIterator<Item = Option<&'a BTreeMap<String, String>>>,
    changed_catalog_entries: &BTreeMap<String, BTreeSet<String>>,
) -> bool {
    for dependencies in dependency_groups {
        let Some(dependencies) = dependencies else {
            continue;
        };
        for (dependency_name, spec) in dependencies {
            let Some(catalog_name) = parse_catalog_protocol(spec) else {
                continue;
            };
            let Some(names) = changed_catalog_entries.get(catalog_name) else {
                continue;
            };
            if names.contains(dependency_name) {
                return true;
            }
        }
    }
    false
}

fn global_log<Output: Reporter>(level: LogLevel, message: String) {
    Output::emit(&LogEvent::Global(GlobalLog { level, message }));
}

fn report_generated_changeset<Output: Reporter>(
    changeset_path: &Path,
    releases: &IndexMap<String, IntentBumpType>,
) {
    global_log::<Output>(
        LogLevel::Info,
        format!(
            "Generated a changeset at {} for: {}",
            changeset_path.display(),
            releases
                .iter()
                .map(|(name, bump)| format!("{name} ({bump})"))
                .collect::<Vec<_>>()
                .join(", "),
        ),
    );
}
