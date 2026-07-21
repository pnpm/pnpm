use super::recursive::discover_workspace_projects;
use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::Diagnostic;
use pacquet_catalogs_config::get_catalogs_from_workspace_manifest;
use pacquet_catalogs_protocol_parser::parse_catalog_protocol;
use pacquet_catalogs_types::Catalogs;
use pacquet_config::{Config, matcher::create_matcher};
use pacquet_package_manifest::{PackageManifest, PackageManifestError};
use pacquet_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use pacquet_versioning::{IntentBumpType, format_change_intent};
use pacquet_workspace::{
    ReadProjectManifestOnlyError, ReadWorkspaceManifestError, read_workspace_manifest,
    safe_read_project_manifest_only,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{self, ErrorKind, Write as _},
    path::{Path, PathBuf},
};

#[derive(Debug, Display, Error, Diagnostic)]
enum UpdateChangesetError {
    #[display("Failed to read project manifest: {_0}")]
    ReadProject(#[error(source)] ReadProjectManifestOnlyError),

    #[display("Failed to inspect project manifest: {_0}")]
    InspectProject(#[error(source)] PackageManifestError),

    #[display("Failed to read pnpm-workspace.yaml: {_0}")]
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
            let (projects, _) = discover_workspace_projects(&workspace_dir)?;
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
        let config_text = match fs::read_to_string(&config_path) {
            Ok(config_text) => config_text,
            Err(source) if source.kind() == ErrorKind::NotFound => {
                global_log::<Output>(
                    LogLevel::Warn,
                    format!(
                        "No changeset was generated because {} does not exist",
                        config_path.display(),
                    ),
                );
                return Ok(());
            }
            Err(source) => {
                return Err(UpdateChangesetError::ReadConfig { path: config_path, source }.into());
            }
        };
        let config: Value = serde_json::from_str(&config_text).map_err(|source| {
            UpdateChangesetError::ParseConfig { path: config_path.clone(), source }
        })?;
        let ignore_patterns = config
            .get("ignore")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect::<Vec<_>>();
        let ignored = create_matcher(&ignore_patterns);

        let workspace_manifest = read_workspace_manifest(&self.workspace_dir)
            .map_err(UpdateChangesetError::ReadWorkspace)?;
        let catalogs_after = get_catalogs_from_workspace_manifest(workspace_manifest.as_ref())?;
        let changed_catalog_entries =
            find_changed_catalog_entries(&self.catalogs_before, &catalogs_after);
        let mut releases = BTreeMap::new();
        for root_dir in &self.root_dirs {
            let Some(manifest) = safe_read_project_manifest_only(root_dir)
                .map_err(UpdateChangesetError::ReadProject)?
            else {
                continue;
            };
            let Some(package_name) = manifest.value().get("name").and_then(Value::as_str) else {
                continue;
            };
            if manifest.value().get("private").and_then(Value::as_bool) == Some(true)
                || ignored.matches(package_name)
            {
                continue;
            }
            let dep_specs = UpdateDepSpecs::from_manifest(&manifest)
                .map_err(UpdateChangesetError::InspectProject)?;
            let dep_specs_before = self.dep_specs_before.get(root_dir).and_then(Option::as_ref);
            let peer_dependencies_changed = dep_specs_before
                .is_some_and(|before| before.peer_dependencies != dep_specs.peer_dependencies)
                || uses_changed_catalog_entry(
                    [dep_specs.peer_dependencies.as_ref()],
                    &changed_catalog_entries,
                );
            if peer_dependencies_changed {
                releases.insert(package_name.to_string(), IntentBumpType::Major);
                continue;
            }
            let production_dependencies_changed = dep_specs_before.is_none_or(|before| {
                before.dependencies != dep_specs.dependencies
                    || before.optional_dependencies != dep_specs.optional_dependencies
            }) || uses_changed_catalog_entry(
                dep_specs.production_groups(),
                &changed_catalog_entries,
            );
            if production_dependencies_changed {
                releases.insert(package_name.to_string(), IntentBumpType::Patch);
            }
        }
        if releases.is_empty() {
            global_log::<Output>(
                LogLevel::Info,
                "No changeset was generated because the update did not change the production or peer dependencies of any workspace package".to_string(),
            );
            return Ok(());
        }

        let releases = releases.into_iter().collect::<IndexMap<_, _>>();
        let mut random = [0_u8; 4];
        getrandom::fill(&mut random).expect("read entropy from the operating system");
        let id = format!("pnpm-update-{:08x}", u32::from_be_bytes(random));
        ensure_changeset_dir_is_safe(&changeset_dir)?;
        let changeset_path = changeset_dir.join(format!("{id}.md"));
        let content = format_change_intent(&releases, "Update dependencies.");
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&changeset_path)
            .and_then(|mut file| file.write_all(content.as_bytes()))
            .map_err(|source| UpdateChangesetError::WriteChangeset {
                path: changeset_path.clone(),
                source,
            })?;
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
        Ok(())
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
        || pacquet_fs::read_symlink_dir(changeset_dir).is_ok()
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
    before
        .keys()
        .chain(after.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|catalog_name| {
            let changed = before
                .get(catalog_name)
                .into_iter()
                .flatten()
                .map(|(name, _)| name)
                .chain(after.get(catalog_name).into_iter().flatten().map(|(name, _)| name))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .filter(|dependency_name| {
                    before.get(catalog_name).and_then(|catalog| catalog.get(*dependency_name))
                        != after.get(catalog_name).and_then(|catalog| catalog.get(*dependency_name))
                })
                .cloned()
                .collect::<BTreeSet<_>>();
            (!changed.is_empty()).then(|| (catalog_name.clone(), changed))
        })
        .collect()
}

fn uses_changed_catalog_entry<'a>(
    dependency_groups: impl IntoIterator<Item = Option<&'a BTreeMap<String, String>>>,
    changed_catalog_entries: &BTreeMap<String, BTreeSet<String>>,
) -> bool {
    dependency_groups.into_iter().flatten().any(|dependencies| {
        dependencies.iter().any(|(dependency_name, spec)| {
            parse_catalog_protocol(spec).is_some_and(|catalog_name| {
                changed_catalog_entries
                    .get(catalog_name)
                    .is_some_and(|names| names.contains(dependency_name))
            })
        })
    })
}

fn global_log<Output: Reporter>(level: LogLevel, message: String) {
    Output::emit(&LogEvent::Global(GlobalLog { level, message }));
}
