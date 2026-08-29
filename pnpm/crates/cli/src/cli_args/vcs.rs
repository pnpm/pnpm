use clap::{Args, Subcommand};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::Config;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, io, path::Path};
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
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PnpmWorkspaceInventory {
    schema_version: u8,
    default_scope: String,
    root_component_name: String,
    root_main_file: String,
    projects: Vec<PnpmProjectInventoryItem>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PnpmProjectInventoryItem {
    root_dir: String,
    component_name: String,
    manifest_file: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BitSyncResult {
    schema_version: u8,
    root_component: String,
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

impl VcsArgs {
    pub async fn run(self, cwd: &Path, config: &Config) -> miette::Result<()> {
        let sync_result =
            prepare_workspace(&self.bit_path, self.scope.as_deref(), cwd, config).await?;
        match self.command {
            VcsCommand::Init => {
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
        }
        Ok(())
    }
}

async fn prepare_workspace(
    bit_path: &str,
    scope: Option<&str>,
    cwd: &Path,
    config: &Config,
) -> miette::Result<BitSyncResult> {
    if config.workspace_dir.as_deref() != Some(cwd) {
        return Err(VcsError::WorkspaceRequired.into());
    }
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
                "--no-agent",
                "--no-mcp",
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
    if result.schema_version != 1 {
        return Err(protocol_error(
            "The installed Bit version does not support pnpm workspace inventory protocol version 1",
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
        });
    }
    let root_candidate = format!(
        "{}-workspace",
        root_name.filter(|name| !name.is_empty()).unwrap_or_else(|| "pnpm".to_string()),
    );
    let root_component_name = unique_component_name(root_candidate, "root", &mut used_names);
    Ok(PnpmWorkspaceInventory {
        schema_version: 1,
        default_scope: default_scope.to_string(),
        root_component_name,
        root_main_file: "pnpm-workspace.yaml".to_string(),
        projects: inventory_projects,
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
    format!(
        "Initialized Bit VCS with {} components. Root component: {}",
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
