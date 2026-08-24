//! `pnpm stage approve` — publish staged versions, chosen interactively
//! when none are named.
//!
//! A batch of versions is approved through a single [`OtpSession`], so one
//! proof of presence covers all of them, and in workspace dependency order,
//! so a package reaches the registry only after the workspace packages it
//! depends on.

use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use derive_more::{Display, Error};
use dialoguer::MultiSelect;
use miette::{Diagnostic, IntoDiagnostic};
use pnpm_config::{Config, LinkWorkspacePackages};
use pnpm_network_web_auth::{Host as WebAuthHost, OtpSession, StdinIsTty, StdoutIsTty};
use pnpm_reporter::Reporter;
use pnpm_workspace::GraphPkg;
use pnpm_workspace_projects_graph::{CreateProjectsGraphOptions, create_projects_graph};
use serde_json::Value;

use super::{
    StageArgs, StageContext, StageError, fetch_stage_items, global_info, global_warn, is_uuid,
    stage_endpoint_url, stage_json_request, stage_request_in_session,
};
use crate::cli_args::{
    changelog::published_name,
    recursive::{discover_workspace_projects, sort_projects},
    sanitize::sanitize_inline,
};

/// `pnpm stage approve` with no `<stage-id>` outside an interactive
/// terminal, where the staged versions cannot be chosen.
#[derive(Debug, Display, Error, Diagnostic)]
#[display(r#"Missing required <stage-id> for "pnpm stage approve""#)]
#[diagnostic(
    code(ERR_PNPM_STAGE_ID_REQUIRED),
    help(
        r#"Run "pnpm stage approve" in an interactive terminal to choose from the staged packages, or pass the stage ids to approve."#
    )
)]
struct StageApproveIdRequired;

/// One staged version, as much of it as the registry's `-/stage` listing
/// reported.
#[derive(Debug, Clone)]
struct StageApprovalItem {
    id: String,
    package_name: Option<String>,
    version: Option<String>,
    tag: Option<String>,
    created_at: Option<String>,
    actor: Option<String>,
}

impl StageApprovalItem {
    /// A staged version named on the command line, before the registry
    /// listing filled in what it publishes.
    fn from_id(id: &str) -> Self {
        StageApprovalItem {
            id: id.to_owned(),
            package_name: None,
            version: None,
            tag: None,
            created_at: None,
            actor: None,
        }
    }

    /// Reads one entry of the registry's `-/stage` listing. The entry is
    /// registry-controlled input that ends up in a terminal prompt the user
    /// picks releases from, so its id has to be the same UUID the other
    /// subcommands accept, and its text is stripped of the control characters
    /// that could redraw the prompt around a selection.
    fn from_value(item: &Value) -> Option<Self> {
        let string_field = |field: &str| {
            item.get(field)
                .and_then(Value::as_str)
                .map(|value| sanitize_inline(value).into_owned())
                .filter(|value| !value.is_empty())
        };
        // The id is validated before sanitizing: stripping a formatting
        // character out of it must not be what makes it a UUID.
        let id = item.get("id").and_then(Value::as_str).filter(|id| is_uuid(id))?.to_owned();
        Some(StageApprovalItem {
            id,
            package_name: string_field("packageName"),
            version: string_field("version"),
            tag: string_field("tag"),
            created_at: string_field("createdAt"),
            actor: string_field("actor"),
        })
    }

    /// How progress lines name this staged version.
    fn label(&self) -> String {
        match (&self.package_name, &self.version) {
            (Some(package_name), Some(version)) => format!("{package_name}@{version}"),
            (Some(package_name), None) => package_name.clone(),
            (None, _) => self.id.clone(),
        }
    }

    /// How error messages name this staged version: the label plus the id
    /// the other `stage` subcommands address it by.
    fn reference(&self) -> String {
        match self.package_name {
            Some(_) => format!("{} ({})", self.label(), self.id),
            None => self.id.clone(),
        }
    }

    /// How the interactive picker names this staged version.
    fn choice(&self) -> String {
        let details: Vec<String> = [
            self.tag.clone(),
            self.created_at.as_ref().map(|created_at| format!("staged {created_at}")),
            self.actor.as_ref().map(|actor| format!("by {actor}")),
        ]
        .into_iter()
        .flatten()
        .collect();
        if details.is_empty() {
            self.label()
        } else {
            format!("{} ({})", self.label(), details.join(", "))
        }
    }
}

/// Where each workspace package sits in the order its siblings have to be
/// published in, keyed by the name the package publishes under — the only
/// name a staged version carries.
struct WorkspaceApprovalOrder {
    /// Index of the topological chunk a package belongs to. A package only
    /// ever depends on packages in lower-indexed chunks, so approving in
    /// ascending index order publishes every dependency before its
    /// dependents.
    chunk_index_by_package_name: HashMap<String, usize>,
    /// The workspace siblings a package directly depends on.
    dependency_names_by_package_name: HashMap<String, Vec<String>>,
}

pub(super) async fn stage_approve<Reporter: self::Reporter>(
    args: &StageArgs,
    config: &Config,
) -> miette::Result<Option<String>> {
    let context = args.stage_context(config, None)?;
    let stage_ids = parse_stage_ids(&args.params)?;
    if let [stage_id] = stage_ids.as_slice() {
        let mut session = OtpSession::new(context.web_auth_fetch_options.clone());
        approve_staged_package::<Reporter>(
            &context,
            &mut session,
            &StageApprovalItem::from_id(stage_id),
        )
        .await?;
        return Ok(Some(format!("Staged package {stage_id} approved and published successfully.")));
    }
    let items = if stage_ids.is_empty() {
        if !WebAuthHost::stdin_is_tty() || !WebAuthHost::stdout_is_tty() {
            return Err(StageApproveIdRequired.into());
        }
        let staged = approval_items(&context).await?;
        if staged.is_empty() {
            return Ok(Some("There are no staged packages awaiting approval.".to_owned()));
        }
        let selected = prompt_for_staged_packages(&staged)?;
        if selected.is_empty() {
            return Ok(Some("No staged packages were selected.".to_owned()));
        }
        selected
    } else {
        resolve_approval_items(&context, &stage_ids).await?
    };
    approve_staged_packages::<Reporter>(&context, config, items).await
}

/// The `<stage-id>` arguments of `pnpm stage approve`, each validated as a
/// UUID. An empty list asks for interactive selection.
///
/// A staged version repeated on the command line is one approval: sending the
/// second request would either fail against the release the first one
/// published, or count the same package twice. Stage ids are hexadecimal, so
/// the same id in two spellings is the same id; the first spelling is the one
/// that reaches the registry.
fn parse_stage_ids(params: &[String]) -> Result<Vec<String>, StageError> {
    let mut seen = HashSet::new();
    params
        .iter()
        .skip(1)
        .filter(|stage_id| seen.insert(stage_id.to_lowercase()))
        .map(
            |stage_id| {
                if is_uuid(stage_id) {
                    Ok(stage_id.clone())
                } else {
                    Err(StageError::InvalidStageId)
                }
            },
        )
        .collect()
}

async fn approval_items(context: &StageContext) -> miette::Result<Vec<StageApprovalItem>> {
    Ok(fetch_stage_items(context, None)
        .await?
        .iter()
        .filter_map(StageApprovalItem::from_value)
        .collect())
}

/// The staged versions the given ids identify, each read from the registry's
/// entry for that id rather than from the full staged listing, which a busy
/// registry can page far beyond what the batch needs.
///
/// A version the registry does not describe is kept as its bare id, so
/// approving it fails on the registry's own error rather than on a guess
/// about why it is missing; it also carries no package name, so it is
/// approved outside the workspace order.
async fn resolve_approval_items(
    context: &StageContext,
    stage_ids: &[String],
) -> miette::Result<Vec<StageApprovalItem>> {
    let mut items = Vec::with_capacity(stage_ids.len());
    for stage_id in stage_ids {
        let url = stage_endpoint_url(&context.registry, &format!("-/stage/{stage_id}"))?;
        let action = format!("view staged package {stage_id}");
        let described: Option<Value> =
            stage_json_request(context, url.as_str(), &action).await.ok();
        items.push(
            described
                .as_ref()
                .and_then(|item| StageApprovalItem::from_value(&with_id(item, stage_id)))
                .unwrap_or_else(|| StageApprovalItem::from_id(stage_id)),
        );
    }
    Ok(items)
}

/// The registry's description of a staged version, keyed by the id the
/// command line named it with: the id in the body is the registry's own
/// spelling, and hexadecimal ids are the same id in any casing.
fn with_id(item: &Value, stage_id: &str) -> Value {
    let mut item = item.clone();
    if let Some(object) = item.as_object_mut() {
        object.insert("id".to_owned(), Value::String(stage_id.to_owned()));
    }
    item
}

/// Show the checkbox prompt; an interrupted prompt selects nothing.
fn prompt_for_staged_packages(
    staged: &[StageApprovalItem],
) -> miette::Result<Vec<StageApprovalItem>> {
    let choices: Vec<String> = staged.iter().map(StageApprovalItem::choice).collect();
    let selected = MultiSelect::new()
        .with_prompt(
            "Choose which staged packages to approve (<space> to select, <enter> to confirm)",
        )
        .items(&choices)
        .interact_opt()
        .into_diagnostic()?;
    Ok(selected.unwrap_or_default().into_iter().map(|index| staged[index].clone()).collect())
}

async fn approve_staged_packages<Reporter: self::Reporter>(
    context: &StageContext,
    config: &Config,
    items: Vec<StageApprovalItem>,
) -> miette::Result<Option<String>> {
    let order = read_workspace_approval_order(config)?;
    let items = sort_items_for_approval(items, order.as_ref());
    let mut session = OtpSession::new(context.web_auth_fetch_options.clone());
    let mut unpublished_package_names: HashSet<String> = HashSet::new();
    let mut approved = 0_usize;
    for item in &items {
        let blockers = unavailable_dependencies(item, &unpublished_package_names, order.as_ref());
        if !blockers.is_empty() {
            record_unpublished(item, &mut unpublished_package_names);
            global_warn::<Reporter>(&format!(
                "Skipped {}, as it depends on {}, which could not be approved",
                item.label(),
                blockers.join(", "),
            ));
            continue;
        }
        match approve_staged_package::<Reporter>(context, &mut session, item).await {
            Ok(()) => {
                approved += 1;
                global_info::<Reporter>(&format!("Approved {}", item.label()));
            }
            // Only the registry's verdict on one staged version is
            // survivable. An authentication failure or a broken connection
            // applies to every remaining version too, so it aborts the batch.
            Err(error) if is_stage_registry_error(&error) => {
                record_unpublished(item, &mut unpublished_package_names);
                global_warn::<Reporter>(&format!("{error}"));
            }
            Err(error) => return Err(error),
        }
    }
    if approved < items.len() {
        // pnpm prints this summary and exits 1. A command here either returns
        // output or an error, never both, so print it the way the dispatcher
        // prints a command's output and exit with the failing status.
        #[expect(clippy::exit, reason = "an incomplete approval batch exits 1, mirroring pnpm")]
        {
            println!("Approved {approved} of {}.", render_package_count(items.len()));
            std::process::exit(1);
        }
    }
    Ok(Some(format!("Approved {} successfully.", render_package_count(approved))))
}

async fn approve_staged_package<Reporter: self::Reporter>(
    context: &StageContext,
    session: &mut OtpSession,
    item: &StageApprovalItem,
) -> miette::Result<()> {
    let url = stage_endpoint_url(&context.registry, &format!("-/stage/{}/approve", item.id))?;
    let action = format!("approve staged package {}", item.reference());
    stage_request_in_session::<Reporter>(
        context,
        session,
        reqwest::Method::POST,
        url.as_str(),
        &action,
    )
    .await
}

fn is_stage_registry_error(error: &miette::Report) -> bool {
    error.code().is_some_and(|code| code.to_string() == "ERR_PNPM_STAGE_REGISTRY_ERROR")
}

fn record_unpublished(item: &StageApprovalItem, unpublished_package_names: &mut HashSet<String>) {
    if let Some(package_name) = &item.package_name {
        unpublished_package_names.insert(package_name.clone());
    }
}

/// Reads the workspace the command runs in and derives the order its
/// packages have to be approved in.
///
/// Returns `None` outside a workspace, where nothing is known about how the
/// staged versions relate and the selection order is kept as is.
fn read_workspace_approval_order(
    config: &Config,
) -> miette::Result<Option<WorkspaceApprovalOrder>> {
    let Some(workspace_dir) = config.workspace_dir.clone() else {
        return Ok(None);
    };
    let (projects, _patterns) = discover_workspace_projects(&workspace_dir, config)?;
    let graph = create_projects_graph(
        projects.iter().map(|project| GraphPkg { project }).collect(),
        &CreateProjectsGraphOptions {
            link_workspace_packages: Some(
                config.link_workspace_packages != LinkWorkspacePackages::Off,
            ),
            ..CreateProjectsGraphOptions::default()
        },
    )
    .graph;
    let published_name_by_root_dir: HashMap<&Path, String> = graph
        .iter()
        .filter_map(|(root_dir, node)| {
            let manifest = node.package.project.manifest.value();
            let name = published_name(manifest)
                .or_else(|| manifest.get("name")?.as_str())
                .filter(|name| !name.is_empty())?;
            Some((root_dir.as_path(), name.to_owned()))
        })
        .collect();
    let mut chunk_index_by_package_name = HashMap::new();
    let mut dependency_names_by_package_name = HashMap::new();
    for (chunk_index, chunk) in sort_projects(&graph, None).into_iter().enumerate() {
        for root_dir in chunk {
            let Some(package_name) = published_name_by_root_dir.get(root_dir.as_path()) else {
                continue;
            };
            chunk_index_by_package_name.insert(package_name.clone(), chunk_index);
            let dependencies: Vec<String> = graph[&root_dir]
                .dependencies
                .iter()
                .filter_map(|dependency| {
                    published_name_by_root_dir.get(dependency.as_path()).cloned()
                })
                .collect();
            dependency_names_by_package_name.insert(package_name.clone(), dependencies);
        }
    }
    Ok(Some(WorkspaceApprovalOrder {
        chunk_index_by_package_name,
        dependency_names_by_package_name,
    }))
}

/// Orders staged versions so that a workspace package is approved after the
/// workspace packages it depends on. Staged versions of packages outside the
/// workspace keep their original relative order, after the workspace ones.
fn sort_items_for_approval(
    mut items: Vec<StageApprovalItem>,
    order: Option<&WorkspaceApprovalOrder>,
) -> Vec<StageApprovalItem> {
    items.sort_by_key(|item| chunk_index_of(item, order));
    items
}

/// The packages in `unpublished_package_names` that `item` depends on, and
/// that therefore will not be on the registry by the time `item` would be
/// approved.
fn unavailable_dependencies(
    item: &StageApprovalItem,
    unpublished_package_names: &HashSet<String>,
    order: Option<&WorkspaceApprovalOrder>,
) -> Vec<String> {
    let Some((order, package_name)) = order.zip(item.package_name.as_deref()) else {
        return Vec::new();
    };
    order
        .dependency_names_by_package_name
        .get(package_name)
        .into_iter()
        .flatten()
        .filter(|dependency| unpublished_package_names.contains(*dependency))
        .cloned()
        .collect()
}

fn chunk_index_of(item: &StageApprovalItem, order: Option<&WorkspaceApprovalOrder>) -> usize {
    order
        .zip(item.package_name.as_deref())
        .and_then(|(order, package_name)| {
            order.chunk_index_by_package_name.get(package_name).copied()
        })
        .unwrap_or(usize::MAX)
}

fn render_package_count(count: usize) -> String {
    format!("{count} staged package{}", if count == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests;
