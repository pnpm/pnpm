use clap::{Args, Subcommand};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_catalogs_config::get_catalogs_from_workspace_manifest;
use pnpm_catalogs_types::{Catalogs, DEFAULT_CATALOG_NAME};
use pnpm_config::Config;
use pnpm_workspace_manifest_writer::{
    UpdateWorkspaceManifestOptions, update_manifest_field, update_workspace_manifest,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    io,
    path::Path,
};
use tempfile::NamedTempFile;
use tokio::process::Command;

use super::recursive::discover_workspace_projects;

#[derive(Debug, Args)]
pub struct VcsArgs {
    /// Path or command name of the Bit executable.
    #[clap(long = "bit-path", default_value = "bit", global = true)]
    pub bit_path: String,

    /// Return the structured Bit result.
    #[clap(long, global = true)]
    pub json: bool,

    /// Default Bit scope used when initializing a regular pnpm workspace.
    #[clap(long, global = true)]
    pub scope: Option<String>,

    #[clap(subcommand)]
    pub command: VcsCommand,
}

#[derive(Debug, Subcommand)]
pub enum VcsCommand {
    /// Initialize Bit and register every pnpm workspace project.
    Init,
    /// Show component changes reported by Bit.
    Status,
    /// Snap all changed components as one Bit batch.
    Commit {
        /// Describe the workspace commit.
        #[clap(short, long)]
        message: Option<String>,
    },
    /// Import Bit components and reconcile pnpm workspace catalogs.
    Import {
        /// Component IDs or patterns to import.
        #[clap(required = true)]
        components: Vec<String>,
    },
}

#[derive(Debug, Display, Error, Diagnostic)]
enum VcsError {
    #[display("Unable to find the Bit executable: {bit_path}")]
    #[diagnostic(
        code(ERR_PNPM_BIT_NOT_FOUND),
        help("Install Bit or pass its location with --bit-path.")
    )]
    NotFound { bit_path: String },

    #[display("{details}")]
    #[diagnostic(code(ERR_PNPM_BIT_COMMAND_FAILED))]
    CommandFailed { details: String },

    #[display("{details}")]
    #[diagnostic(code(ERR_PNPM_BIT_PROTOCOL_ERROR))]
    ProtocolError { details: String },

    #[display("pnpm vcs requires a pnpm workspace")]
    #[diagnostic(code(ERR_PNPM_VCS_WORKSPACE_REQUIRED))]
    WorkspaceRequired,

    #[display("A Bit scope is required to initialize this workspace")]
    #[diagnostic(
        code(ERR_PNPM_VCS_SCOPE_REQUIRED),
        help("Pass --scope <owner.scope>, for example: pnpm vcs commit --scope acme.my-workspace")
    )]
    ScopeRequired,

    #[display("Unable to prepare the pnpm workspace inventory: {details}")]
    #[diagnostic(code(ERR_PNPM_VCS_INVENTORY_ERROR))]
    InventoryError { details: String },

    #[display("Unable to update the pnpm workspace: {details}")]
    #[diagnostic(code(ERR_PNPM_VCS_WORKSPACE_UPDATE_ERROR))]
    WorkspaceUpdate { details: String },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PnpmWorkspaceInventory {
    schema_version: u8,
    default_scope: String,
    root_component_name: String,
    root_main_file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_profile: Option<WorkspaceToolMap>,
    projects: Vec<PnpmProjectInventoryItem>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PnpmProjectInventoryItem {
    root_dir: String,
    component_name: String,
    manifest_file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    requirements: Option<WorkspaceToolMap>,
}

type WorkspaceToolMap = BTreeMap<String, WorkspaceTool>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkspaceTool {
    implementation: String,
    version: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct PnpmVcsManifestConfig {
    profile: Option<WorkspaceToolMap>,
    requirements: Option<WorkspaceToolMap>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BitSyncResult {
    schema_version: u8,
    root_component: String,
    workspace_profile: WorkspaceToolMap,
    updated_components: Vec<String>,
    components: Vec<BitSyncedComponent>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BitSyncedComponent {
    id: String,
    root_dir: String,
    files: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BitStatusResult {
    #[serde(default)]
    schema_version: u8,
    #[serde(default)]
    auto_tag_pending_components: Vec<String>,
    current_lane_id: Option<String>,
    #[serde(default)]
    modified_components: Vec<String>,
    #[serde(default)]
    new_components: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BitSnapResult {
    #[serde(default)]
    schema_version: u8,
    snapped: bool,
    batch_id: Option<String>,
    lane_name: Option<String>,
    snapped_components: Vec<String>,
    auto_snapped_components: Vec<AutoSnappedComponent>,
    new_components: Vec<String>,
    removed_components: Vec<String>,
    warnings: Vec<String>,
    total_components_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutoSnappedComponent {
    id: String,
    triggered_by: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BitImportResult {
    #[serde(flatten)]
    bit: BTreeMap<String, serde_json::Value>,
    pnpm_vcs: Option<PnpmVcsImportPlan>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PnpmVcsImportPlan {
    schema_version: u8,
    components: Vec<PnpmVcsImportedComponent>,
    catalogs: Vec<PnpmVcsCatalogBinding>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PnpmVcsImportedComponent {
    id: String,
    root_dir: String,
    package_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PnpmVcsCatalogBinding {
    catalog_name: String,
    package_name: String,
    specifier: String,
    component_id: Option<String>,
}

impl VcsArgs {
    pub async fn run(self, cwd: &Path, config: &Config) -> miette::Result<()> {
        match self.command {
            VcsCommand::Init => {
                if migrate_workspace_dependencies_to_catalogs(cwd, config)? {
                    execute_pnpm_install(cwd).await?;
                }
                let sync_result =
                    prepare_workspace(&self.bit_path, self.scope.as_deref(), cwd, config).await?;
                if self.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&sync_result)
                            .expect("Bit sync result serializes"),
                    );
                } else {
                    println!("{}", render_init(&sync_result));
                }
            }
            VcsCommand::Status => {
                prepare_workspace(&self.bit_path, self.scope.as_deref(), cwd, config).await?;
                let stdout = execute_bit(&self.bit_path, &["status", "--json"], cwd).await?;
                let result: BitStatusResult = parse_bit_json(&stdout, "status")?;
                assert_status_protocol(&result)?;
                if self.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(
                            &serde_json::from_str::<serde_json::Value>(&stdout).map_err(|_| {
                                protocol_error("Bit returned invalid JSON for vcs status")
                            })?,
                        )
                        .expect("JSON value serializes"),
                    );
                } else {
                    println!("{}", render_status(&result));
                }
            }
            VcsCommand::Commit { message } => {
                prepare_workspace(&self.bit_path, self.scope.as_deref(), cwd, config).await?;
                let mut args = vec![
                    "snap",
                    "--json",
                    "--ignore-issues",
                    "MissingPackagesDependenciesOnFs,MissingManuallyConfiguredPackages,MissingLinksFromNodeModulesToSrc,MissingDists",
                ];
                if let Some(ref message) = message {
                    args.extend(["--message", message]);
                }
                let stdout = execute_bit(&self.bit_path, &args, cwd).await?;
                let result: BitSnapResult = parse_bit_json(&stdout, "commit")?;
                assert_snap_protocol(&result)?;
                if self.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&result).expect("Bit snap result serializes"),
                    );
                } else {
                    println!("{}", render_commit(&result));
                }
            }
            VcsCommand::Import { components } => {
                prepare_workspace(&self.bit_path, self.scope.as_deref(), cwd, config).await?;
                let mut args = vec![
                    "import".to_string(),
                    "--json".to_string(),
                    "--skip-dependency-installation".to_string(),
                    "--skip-write-config-files".to_string(),
                ];
                args.extend(components);
                let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
                let stdout = execute_bit(&self.bit_path, &arg_refs, cwd).await?;
                let result: BitImportResult = parse_bit_json(&stdout, "import")?;
                let plan = result.pnpm_vcs.as_ref().ok_or_else(|| {
                    protocol_error(
                        "Bit did not return a pnpm VCS import plan; update Bit or initialize this workspace with pnpm vcs init",
                    )
                })?;
                if plan.schema_version != 1 {
                    return Err(protocol_error(
                        "The installed Bit version does not support pnpm VCS import protocol version 1",
                    ));
                }
                let updated_config = apply_import_plan(cwd, config, plan)?;
                execute_pnpm_install(cwd).await?;
                let sync_result =
                    prepare_workspace(&self.bit_path, self.scope.as_deref(), cwd, &updated_config)
                        .await?;
                if self.json {
                    let mut output =
                        serde_json::to_value(&result).expect("Bit import result serializes");
                    output["pnpmVcsSync"] =
                        serde_json::to_value(&sync_result).expect("Bit sync result serializes");
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&output).expect("import result serializes"),
                    );
                } else {
                    println!(
                        "Imported {} Bit component(s), updated pnpm catalogs, and installed the workspace.",
                        plan.components.len(),
                    );
                }
            }
        }
        Ok(())
    }
}

fn migrate_workspace_dependencies_to_catalogs(cwd: &Path, config: &Config) -> miette::Result<bool> {
    require_workspace(cwd, config)?;
    let (mut projects, _) = discover_workspace_projects(cwd, config)?;
    let workspace_names = projects
        .iter()
        .filter_map(|project| {
            project
                .manifest
                .value()
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .collect::<HashSet<_>>();
    let mut bindings = BTreeMap::new();
    for project in &projects {
        for field in ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"]
        {
            let Some(dependencies) =
                project.manifest.value().get(field).and_then(|value| value.as_object())
            else {
                continue;
            };
            for (name, value) in dependencies {
                let Some(specifier) = value.as_str() else { continue };
                if !workspace_names.contains(name) || !specifier.starts_with("workspace:") {
                    continue;
                }
                if let Some(existing) = bindings.insert(name.clone(), specifier.to_string())
                    && existing != specifier
                {
                    return Err(workspace_update_error(format!(
                        "workspace package {name} is referenced with both {existing} and {specifier}; a catalog requires one workspace-wide binding",
                    )));
                }
            }
        }
    }
    if bindings.is_empty() {
        return Ok(false);
    }
    for project in &mut projects {
        let mut changed = false;
        for field in ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"]
        {
            let Some(dependencies) = project
                .manifest
                .value_mut()
                .get_mut(field)
                .and_then(serde_json::Value::as_object_mut)
            else {
                continue;
            };
            for (name, value) in dependencies {
                if bindings.contains_key(name)
                    && value.as_str().is_some_and(|specifier| specifier.starts_with("workspace:"))
                {
                    *value = serde_json::Value::String("catalog:".to_string());
                    changed = true;
                }
            }
        }
        if changed {
            project.manifest.save().map_err(|error| workspace_update_error(error.to_string()))?;
        }
    }
    let mut catalogs = Catalogs::new();
    catalogs.insert(DEFAULT_CATALOG_NAME.to_string(), bindings);
    update_workspace_manifest(
        cwd,
        &UpdateWorkspaceManifestOptions { updated_catalogs: Some(&catalogs), ..Default::default() },
    )
    .map_err(|error| workspace_update_error(error.to_string()))?;
    Ok(true)
}

fn apply_import_plan(
    cwd: &Path,
    config: &Config,
    plan: &PnpmVcsImportPlan,
) -> miette::Result<Config> {
    let manifest = pnpm_workspace::read_workspace_manifest(cwd)
        .map_err(|error| workspace_update_error(error.to_string()))?
        .ok_or_else(|| workspace_update_error("pnpm-workspace.yaml is missing".to_string()))?;
    let mut packages = manifest
        .packages
        .unwrap_or_else(|| config.workspace_package_patterns.clone().unwrap_or_default());
    for component in &plan.components {
        if !packages.contains(&component.root_dir) {
            packages.push(component.root_dir.clone());
        }
    }
    packages.sort();
    packages.dedup();
    update_manifest_field(
        &cwd.join("pnpm-workspace.yaml"),
        "packages",
        &serde_json::to_value(&packages).expect("package patterns serialize"),
    )
    .map_err(|error| workspace_update_error(error.to_string()))?;

    let mut updated_config = config.clone();
    updated_config.workspace_package_patterns = Some(packages);
    let (projects, _) = discover_workspace_projects(cwd, &updated_config)?;
    let local_packages = projects
        .iter()
        .filter_map(|project| {
            project
                .manifest
                .value()
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .collect::<HashSet<_>>();
    let workspace_manifest = pnpm_workspace::read_workspace_manifest(cwd)
        .map_err(|error| workspace_update_error(error.to_string()))?;
    let mut catalogs = get_catalogs_from_workspace_manifest(workspace_manifest.as_ref())
        .map_err(|error| workspace_update_error(error.to_string()))?;
    let imported_packages = plan
        .components
        .iter()
        .map(|component| component.package_name.as_str())
        .collect::<HashSet<_>>();
    for catalog in catalogs.values_mut() {
        for (package_name, specifier) in catalog {
            if imported_packages.contains(package_name.as_str()) {
                *specifier = "workspace:*".to_string();
            }
        }
    }
    let mut planned_bindings = BTreeMap::new();
    for binding in &plan.catalogs {
        let specifier = if local_packages.contains(&binding.package_name) {
            "workspace:*"
        } else {
            binding.specifier.as_str()
        };
        let key = (binding.catalog_name.as_str(), binding.package_name.as_str());
        if let Some(existing) = planned_bindings.insert(key, specifier)
            && existing != specifier
        {
            return Err(workspace_update_error(format!(
                "imported components require conflicting {} catalog bindings for {}: {} and {}",
                binding.catalog_name, binding.package_name, existing, specifier,
            )));
        }
        catalogs
            .entry(binding.catalog_name.clone())
            .or_default()
            .insert(binding.package_name.clone(), specifier.to_string());
    }
    update_workspace_manifest(
        cwd,
        &UpdateWorkspaceManifestOptions { updated_catalogs: Some(&catalogs), ..Default::default() },
    )
    .map_err(|error| workspace_update_error(error.to_string()))?;
    Ok(updated_config)
}

fn require_workspace(cwd: &Path, config: &Config) -> miette::Result<()> {
    if config.workspace_dir.as_deref() != Some(cwd) {
        return Err(VcsError::WorkspaceRequired.into());
    }
    Ok(())
}

async fn execute_pnpm_install(cwd: &Path) -> miette::Result<()> {
    let executable =
        std::env::current_exe().map_err(|error| workspace_update_error(error.to_string()))?;
    let output =
        Command::new(executable).arg("install").current_dir(cwd).output().await.map_err(
            |error| workspace_update_error(format!("unable to run pnpm install: {error}")),
        )?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Err(workspace_update_error(if stderr.is_empty() { stdout } else { stderr }))
}

fn workspace_update_error(details: String) -> miette::Report {
    VcsError::WorkspaceUpdate { details }.into()
}

async fn prepare_workspace(
    bit_path: &str,
    scope: Option<&str>,
    cwd: &Path,
    config: &Config,
) -> miette::Result<BitSyncResult> {
    require_workspace(cwd, config)?;
    // Validate and fully discover the pnpm workspace before `bit init` creates any files.
    // An existing Bit workspace remains authoritative for its default scope.
    let mut inventory = workspace_inventory(cwd, config, "")?;
    let is_bit_workspace = cwd.join(".bitmap").is_file() && cwd.join(".bit").is_dir();
    if !is_bit_workspace {
        let scope = scope.ok_or(VcsError::ScopeRequired)?;
        inventory.default_scope = scope.to_string();
        execute_bit(
            bit_path,
            &[
                "init",
                "--standalone",
                "--external-package-manager",
                "--skip-interactive",
                "--no-package-json",
                "--default-scope",
                scope,
            ],
            cwd,
        )
        .await?;
    }

    let mut inventory_file = NamedTempFile::new()
        .map_err(|error| VcsError::InventoryError { details: error.to_string() })?;
    serde_json::to_writer(inventory_file.as_file_mut(), &inventory)
        .map_err(|error| VcsError::InventoryError { details: error.to_string() })?;
    let inventory_path = inventory_file.path().to_string_lossy().into_owned();
    let stdout =
        execute_bit(bit_path, &["pnpm-vcs-sync", "--json", "--inventory", &inventory_path], cwd)
            .await?;
    let result: BitSyncResult = parse_bit_json(&stdout, "workspace sync")?;
    if result.schema_version != 2 {
        return Err(protocol_error(
            "The installed Bit version does not support pnpm workspace inventory protocol version 2",
        ));
    }
    Ok(result)
}

fn workspace_inventory(
    workspace_dir: &Path,
    config: &Config,
    default_scope: &str,
) -> miette::Result<PnpmWorkspaceInventory> {
    let (projects, _) = discover_workspace_projects(workspace_dir, config)?;
    let mut used_names = HashSet::new();
    let mut root_name = None;
    let mut workspace_profile = None;
    let mut inventory_projects = Vec::new();
    for project in projects {
        let relative = project
            .root_dir
            .strip_prefix(workspace_dir)
            .map_err(|error| VcsError::InventoryError { details: error.to_string() })?;
        let manifest_name = project
            .manifest
            .value()
            .get("name")
            .and_then(serde_json::Value::as_str)
            .filter(|name| !name.is_empty());
        if relative.as_os_str().is_empty() {
            root_name = manifest_name.map(sanitize_component_name);
            workspace_profile = read_vcs_manifest_config(project.manifest.value())?.profile;
            continue;
        }
        let root_dir = path_to_protocol(relative);
        let candidate = manifest_name
            .map(sanitize_component_name)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| sanitize_component_name(&root_dir));
        let component_name = unique_component_name(candidate, &root_dir, &mut used_names);
        inventory_projects.push(PnpmProjectInventoryItem {
            root_dir,
            component_name,
            manifest_file: "package.json".to_string(),
            requirements: read_component_requirements(project.manifest.value())?,
        });
    }
    let root_candidate = format!(
        "{}-workspace",
        root_name.filter(|name| !name.is_empty()).unwrap_or_else(|| "pnpm".to_string()),
    );
    let root_component_name = unique_component_name(root_candidate, "root", &mut used_names);
    Ok(PnpmWorkspaceInventory {
        schema_version: 2,
        default_scope: default_scope.to_string(),
        root_component_name,
        root_main_file: "pnpm-workspace.yaml".to_string(),
        workspace_profile,
        projects: inventory_projects,
    })
}

fn read_component_requirements(
    manifest: &serde_json::Value,
) -> miette::Result<Option<WorkspaceToolMap>> {
    let mut requirements = read_vcs_manifest_config(manifest)?.requirements.unwrap_or_default();
    if !requirements.contains_key("node")
        && let Some(node_range) = manifest
            .get("engines")
            .and_then(|engines| engines.get("node"))
            .and_then(serde_json::Value::as_str)
    {
        requirements.insert(
            "node".to_string(),
            WorkspaceTool { implementation: "node".to_string(), version: node_range.to_string() },
        );
    }
    Ok((!requirements.is_empty()).then_some(requirements))
}

fn read_vcs_manifest_config(manifest: &serde_json::Value) -> miette::Result<PnpmVcsManifestConfig> {
    let Some(config) = manifest.get("pnpm").and_then(|pnpm| pnpm.get("vcs")) else {
        return Ok(PnpmVcsManifestConfig::default());
    };
    serde_json::from_value(config.clone()).map_err(|error| {
        VcsError::InventoryError {
            details: format!("invalid package.json pnpm.vcs configuration: {error}"),
        }
        .into()
    })
}

fn sanitize_component_name(value: &str) -> String {
    let value = value.strip_prefix('@').unwrap_or(value);
    let mut result = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '/') {
            result.push(character.to_ascii_lowercase());
        } else {
            result.push('-');
        }
    }
    result.trim_matches('-').to_string()
}

fn unique_component_name(candidate: String, root_dir: &str, used: &mut HashSet<String>) -> String {
    if used.insert(candidate.clone()) {
        return candidate;
    }
    let suffix = sanitize_component_name(root_dir).replace('/', "-");
    let unique = format!("{candidate}-{suffix}");
    used.insert(unique.clone());
    unique
}

fn path_to_protocol(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

async fn execute_bit(bit_path: &str, args: &[&str], cwd: &Path) -> miette::Result<String> {
    let output =
        Command::new(bit_path).args(args).current_dir(cwd).output().await.map_err(|err| {
            if err.kind() == io::ErrorKind::NotFound {
                VcsError::NotFound { bit_path: bit_path.to_string() }
            } else {
                VcsError::CommandFailed { details: format!("Bit command failed: {err}") }
            }
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let details = if stderr.is_empty() {
            if stdout.is_empty() {
                format!("Bit command failed: {bit_path} {}", args.join(" "))
            } else {
                stdout
            }
        } else {
            stderr
        };
        return Err(VcsError::CommandFailed { details }.into());
    }
    String::from_utf8(output.stdout).map_err(|_| {
        protocol_error(&format!(
            "Bit returned non-UTF-8 output for vcs {}",
            args.first().copied().unwrap_or("command"),
        ))
    })
}

fn parse_bit_json<ResultValue: serde::de::DeserializeOwned>(
    stdout: &str,
    operation: &str,
) -> miette::Result<ResultValue> {
    serde_json::from_str(stdout).map_err(|error| {
        protocol_error(&format!(
            "Bit returned invalid or incompatible JSON for vcs {operation}: {error}",
        ))
    })
}

fn assert_snap_protocol(result: &BitSnapResult) -> miette::Result<()> {
    let valid_batch =
        !result.snapped || result.batch_id.as_ref().is_some_and(|batch_id| !batch_id.is_empty());
    if result.schema_version == 1 && valid_batch {
        return Ok(());
    }
    Err(protocol_error(
        "The installed Bit version does not support pnpm VCS snap protocol version 1",
    ))
}

fn assert_status_protocol(result: &BitStatusResult) -> miette::Result<()> {
    if result.schema_version == 1 {
        return Ok(());
    }
    Err(protocol_error(
        "The installed Bit version does not support pnpm VCS status protocol version 1",
    ))
}

fn protocol_error(details: &str) -> miette::Report {
    VcsError::ProtocolError { details: details.to_string() }.into()
}

fn render_status(result: &BitStatusResult) -> String {
    let sections = [
        render_components("New components", &result.new_components),
        render_components("Modified components", &result.modified_components),
        render_components("Auto-snap pending", &result.auto_tag_pending_components),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    if sections.is_empty() {
        return "No component changes.".to_string();
    }
    let lane = result
        .current_lane_id
        .as_ref()
        .map(|lane| format!("Bit lane: {lane}\n\n"))
        .unwrap_or_default();
    format!("{lane}{}", sections.join("\n\n"))
}

fn render_init(result: &BitSyncResult) -> String {
    let refreshed = if result.updated_components.is_empty() {
        String::new()
    } else {
        format!(
            " Refreshed {} components to the workspace profile.",
            result.updated_components.len(),
        )
    };
    format!(
        "Initialized Bit VCS with {} components. Root component: {}.{refreshed}",
        result.components.len(),
        result.root_component,
    )
}

fn render_components(title: &str, components: &[String]) -> Option<String> {
    if components.is_empty() {
        return None;
    }
    Some(format!(
        "{title}:\n{}",
        components.iter().map(|component| format!("  {component}")).collect::<Vec<_>>().join("\n"),
    ))
}

fn render_commit(result: &BitSnapResult) -> String {
    if !result.snapped {
        return "No component changes.".to_string();
    }
    let lane = result.lane_name.as_deref().unwrap_or("main");
    let batch_id = result.batch_id.as_deref().expect("validated snap result has a batch ID");
    let components = result
        .snapped_components
        .iter()
        .chain(result.auto_snapped_components.iter().map(|component| &component.id))
        .map(|component| format!("  {component}"))
        .collect::<Vec<_>>();
    let component_list =
        if components.is_empty() { String::new() } else { format!("\n{}", components.join("\n")) };
    let component_label =
        if result.total_components_count == 1 { "component" } else { "components" };
    format!(
        "Created Bit snap batch {} on {lane} ({} {component_label}).{component_list}",
        batch_id, result.total_components_count,
    )
}

#[cfg(test)]
mod tests;
