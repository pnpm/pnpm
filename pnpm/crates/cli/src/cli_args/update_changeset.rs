mod catalogs;
mod ignore;

use catalogs::{find_changed_catalog_entries, uses_changed_catalog_entry};
use ignore::{
    ChangesetDirState, IgnoredProjects, count_package_names, ensure_changeset_dir_is_safe,
    project_reference, read_config_ignore_patterns, write_changeset,
};

use super::{recursive::discover_workspace_projects, update::UpdateSaveArgs};
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
use pnpm_versioning::{
    IntentBumpType, format_change_intent, is_dir_ref, normalize_project_dir, to_project_dir,
};
use pnpm_workspace::{
    ReadProjectManifestOnlyError, ReadWorkspaceManifestError, read_workspace_manifest,
    safe_read_project_manifest_only,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs::{self, OpenOptions},
    io::{self, ErrorKind},
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

impl UpdateSaveArgs {
    /// `--changeset` or `--no-changeset`, falling back to
    /// `updateConfig.changeset`.
    pub(super) fn generates_changeset(&self, config: &Config) -> bool {
        if self.changeset {
            true
        } else if self.no_changeset {
            false
        } else {
            config.update_config.changeset.unwrap_or(false)
        }
    }
}

pub(super) struct UpdateChangesetContext {
    workspace_dir: PathBuf,
    root_dirs: Vec<PathBuf>,
    dep_specs_before: BTreeMap<PathBuf, Option<UpdateDepSpecs>>,
    catalogs_before: Catalogs,
    has_versioning: bool,
    versioning_ignore: Vec<String>,
}

impl UpdateChangesetContext {
    pub(super) fn capture(config: &Config, manifest_path: &Path) -> miette::Result<Self> {
        let project_dir = manifest_path.parent().expect("manifest path always has a parent dir");
        let workspace_dir = config.workspace_dir
            .as_deref()
            .unwrap_or(project_dir)
            .to_path_buf();
        let root_dirs = resolve_changeset_root_dirs(&workspace_dir, project_dir, config)?;
        let dep_specs_before = capture_dep_specs(&root_dirs)?;
        let workspace_manifest =
            read_workspace_manifest(&workspace_dir).map_err(UpdateChangesetError::ReadWorkspace)?;
        let catalogs_before = get_catalogs_from_workspace_manifest(workspace_manifest.as_ref())?;
        let has_versioning = config.explicit_settings.contains_key("versioning")
            || !config.versioning.is_empty();
        let versioning_ignore = config.versioning.ignore.clone();
        Ok(Self {
            workspace_dir,
            root_dirs,
            dep_specs_before,
            catalogs_before,
            has_versioning,
            versioning_ignore,
        })
    }

    pub(super) fn generate<Output: Reporter>(self) -> miette::Result<()> {
        let changeset_dir = self.workspace_dir.join(".changeset");
        let dir_state = ensure_changeset_dir_is_safe(&changeset_dir)?;
        let config_ignore = read_config_ignore_patterns(&changeset_dir.join("config.json"))?;
        if dir_state == ChangesetDirState::Missing
            && config_ignore.is_none()
            && !self.has_versioning
        {
            global_log::<Output>(
                LogLevel::Warn,
                format!(
                    "No changeset was generated because {} does not exist and versioning is not configured in pnpm-workspace.yaml",
                    changeset_dir.display(),
                ),
            );
            return Ok(());
        }

        let ignored =
            IgnoredProjects::new(&config_ignore.unwrap_or_default(), &self.versioning_ignore);
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
        let content = format_change_intent(&releases, "Update dependencies.");
        let changeset_path = write_changeset(&changeset_dir, &content)?;
        report_generated_changeset::<Output>(&changeset_path, &releases);
        Ok(())
    }

    fn collect_releases(
        &self,
        ignored: &IgnoredProjects,
        changed_catalog_entries: &BTreeMap<String, BTreeSet<String>>,
    ) -> Result<BTreeMap<String, IntentBumpType>, UpdateChangesetError> {
        let manifests = read_project_manifests(&self.root_dirs)?;
        let name_counts = count_package_names(&manifests);
        let mut releases = BTreeMap::new();
        for (root_dir, manifest) in &manifests {
            let Some(manifest) = manifest else {
                continue;
            };
            if let Some((reference, bump)) = self.project_release(
                root_dir,
                manifest,
                ignored,
                &name_counts,
                changed_catalog_entries,
            )? {
                releases.insert(reference, bump);
            }
        }
        Ok(releases)
    }

    /// The bump a project needs, when the update changed a dependency its
    /// consumers can observe. A private or ignored package never releases.
    fn project_release(
        &self,
        root_dir: &Path,
        manifest: &PackageManifest,
        ignored: &IgnoredProjects,
        name_counts: &HashMap<String, usize>,
        changed_catalog_entries: &BTreeMap<String, BTreeSet<String>>,
    ) -> Result<Option<(String, IntentBumpType)>, UpdateChangesetError> {
        let project_dir = to_project_dir(&self.workspace_dir, root_dir);
        let Some(package_name) = releasable_package_name(manifest, &project_dir, ignored) else {
            return Ok(None);
        };
        let reference = project_reference(package_name, &self.workspace_dir, root_dir, name_counts);
        let dep_specs =
            UpdateDepSpecs::from_manifest(manifest).map_err(UpdateChangesetError::InspectProject)?;
        let dep_specs_before = self.dep_specs_before.get(root_dir).and_then(Option::as_ref);
        let peer_dependencies_changed = dep_specs_before.is_some_and(|before| {
            before.peer_dependencies != dep_specs.peer_dependencies
        }) || uses_changed_catalog_entry(
            [dep_specs.peer_dependencies.as_ref()],
            changed_catalog_entries,
        );
        if peer_dependencies_changed {
            return Ok(Some((reference, IntentBumpType::Major)));
        }
        let production_dependencies_changed = dep_specs_before.is_none_or(|before| {
            before.dependencies != dep_specs.dependencies
                || before.optional_dependencies != dep_specs.optional_dependencies
        }) || uses_changed_catalog_entry(
            dep_specs.production_groups(),
            changed_catalog_entries,
        );
        Ok(production_dependencies_changed.then_some((reference, IntentBumpType::Patch)))
    }
}

fn resolve_changeset_root_dirs(
    workspace_dir: &Path,
    project_dir: &Path,
    config: &Config,
) -> miette::Result<Vec<PathBuf>> {
    if config.workspace_dir.is_none() {
        return Ok(vec![project_dir.to_path_buf()]);
    }
    let (projects, _) = discover_workspace_projects(workspace_dir, config)?;
    let dirs = projects
        .into_iter()
        .map(|project| project.root_dir)
        .collect::<Vec<_>>();
    Ok(if dirs.is_empty() { vec![project_dir.to_path_buf()] } else { dirs })
}

fn capture_dep_specs(
    root_dirs: &[PathBuf],
) -> Result<BTreeMap<PathBuf, Option<UpdateDepSpecs>>, UpdateChangesetError> {
    root_dirs
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
        .collect()
}

fn read_project_manifests(
    root_dirs: &[PathBuf],
) -> Result<Vec<(PathBuf, Option<PackageManifest>)>, UpdateChangesetError> {
    root_dirs
        .iter()
        .map(|root_dir| {
            let manifest = safe_read_project_manifest_only(root_dir)
                .map_err(UpdateChangesetError::ReadProject)?;
            Ok((root_dir.clone(), manifest))
        })
        .collect()
}

fn releasable_package_name<'a>(
    manifest: &'a PackageManifest,
    project_dir: &str,
    ignored: &IgnoredProjects,
) -> Option<&'a str> {
    let package_name = manifest
        .value()
        .get("name")
        .and_then(Value::as_str)?;
    if manifest
        .value()
        .get("private")
        .and_then(Value::as_bool)
        == Some(true)
        || ignored.matches(package_name, project_dir)
    {
        return None;
    }
    Some(package_name)
}

fn dependency_map(value: &Value, field: &str) -> Option<BTreeMap<String, String>> {
    value
        .get(field)
        .and_then(Value::as_object)
        .map(|dependencies| {
            dependencies
                .iter()
                .filter_map(|(name, spec)| {
                    spec.as_str()
                        .map(|spec| (name.clone(), spec.to_string()))
                })
                .collect()
        })
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
