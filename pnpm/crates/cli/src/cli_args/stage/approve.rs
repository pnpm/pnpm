//! `pnpm stage approve` — publish staged versions, chosen interactively
//! when none are named.
//!
//! A batch of versions is approved through a single [`OtpSession`], so one
//! proof of presence covers all of them. Every selected tarball is downloaded
//! first, and its published manifest determines dependency order.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use derive_more::{Display, Error};
use dialoguer::MultiSelect;
use miette::{Diagnostic, IntoDiagnostic};
use node_semver::Version;
use pnpm_config::Config;
use pnpm_network_web_auth::{Host as WebAuthHost, OtpSession, StdinIsTty, StdoutIsTty};
use pnpm_package_manifest::PackageManifest;
use pnpm_reporter::Reporter;
use pnpm_resolving_parse_wanted_dependency::is_valid_old_npm_package_name;
use pnpm_resolving_resolver_base::{
    ANY_VERSION_RANGE, is_any_version_range, is_valid_semver_range,
};
use pnpm_workspace::{GraphPkg, Project};
use pnpm_workspace_projects_graph::{CreateProjectsGraphOptions, create_projects_graph};
use serde_json::Value;

use super::{
    StageArgs, StageContext, StageError, StageRegistryError, fetch_stage_items,
    fetch_stage_tarball, global_info, global_warn, is_uuid, stage_endpoint_url, stage_json_request,
    stage_request_in_session, summarize_tarball::read_tarball_manifest,
};
use crate::cli_args::{recursive::sequence_graph, sanitize::sanitize_inline};

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

    /// Reads one staged version the registry described. The entry is
    /// registry-controlled input that ends up in a terminal prompt the user
    /// picks releases from, so every field is taken as it came and checked
    /// rather than repaired; the fields that are only displayed are stripped
    /// of the control characters that could redraw the prompt around a
    /// selection.
    fn from_value(item: &Value) -> Option<Self> {
        let string_field = |field: &str| {
            item.get(field)
                .and_then(Value::as_str)
                .map(|value| sanitize_inline(value).into_owned())
                .filter(|value| !value.is_empty())
        };
        // The id and the package name are validated as they came: removing a
        // hidden character must never be what makes a value valid. A name
        // that fails is not displayed. A valid name is URL-safe, so it is
        // also safe to display.
        let id = item.get("id").and_then(Value::as_str).filter(|id| is_uuid(id))?.to_owned();
        let package_name = item
            .get("packageName")
            .and_then(Value::as_str)
            .filter(|name| is_valid_old_npm_package_name(name))
            .map(str::to_owned);
        Some(StageApprovalItem {
            id,
            package_name,
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

/// The dependency order derived from the exact tarballs being approved.
struct StageApprovalOrder {
    dependency_stage_ids: HashMap<String, Vec<String>>,
    order_indices: HashMap<String, usize>,
    package_names: HashMap<String, String>,
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
    approve_staged_packages::<Reporter>(&context, items).await
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
/// about why it is missing. Its tarball still supplies the dependency graph
/// if the registry serves it.
async fn resolve_approval_items(
    context: &StageContext,
    stage_ids: &[String],
) -> miette::Result<Vec<StageApprovalItem>> {
    let mut items = Vec::with_capacity(stage_ids.len());
    for stage_id in stage_ids {
        let url = stage_endpoint_url(&context.registry, &format!("-/stage/{stage_id}"))?;
        let action = format!("view staged package {stage_id}");
        let described: Option<Value> =
            match stage_json_request(context, url.as_str(), &action).await {
                Ok(described) => Some(described),
                // Only the registry answering "no such staged version" is
                // survivable here. An authentication failure or a broken
                // connection applies to every id in the batch, so it aborts
                // before anything is approved.
                Err(error) if is_missing_stage_error(&error) => None,
                Err(error) => return Err(error),
            };
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
    items: Vec<StageApprovalItem>,
) -> miette::Result<Option<String>> {
    let order = read_stage_approval_order(context, &items).await?;
    let items = sort_items_for_approval(items, &order);
    let mut session = OtpSession::new(context.web_auth_fetch_options.clone());
    let mut unpublished_stage_ids: HashSet<String> = HashSet::new();
    let mut approved = 0_usize;
    for item in &items {
        let blockers = unavailable_dependencies(item, &unpublished_stage_ids, &order);
        if !blockers.is_empty() {
            unpublished_stage_ids.insert(item.id.clone());
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
                unpublished_stage_ids.insert(item.id.clone());
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

fn is_missing_stage_error(error: &miette::Report) -> bool {
    error.downcast_ref::<StageRegistryError>().is_some_and(|error| error.status == 404)
}

/// Download every selected package before approval and derive the graph from
/// the package.json files that will reach the registry.
async fn read_stage_approval_order(
    context: &StageContext,
    items: &[StageApprovalItem],
) -> miette::Result<StageApprovalOrder> {
    let mut projects = Vec::with_capacity(items.len());
    let mut stage_id_by_package_version: HashMap<(String, String), String> = HashMap::new();
    for item in items {
        let root_dir = PathBuf::from(&item.id);
        let tarball = fetch_stage_tarball(context, &item.id).await?;
        let manifest = read_tarball_manifest(&tarball)?;
        let package_name = manifest
            .get("name")
            .and_then(Value::as_str)
            .ok_or(StageError::TarballManifestNotFound)?
            .to_owned();
        let version = manifest
            .get("version")
            .and_then(Value::as_str)
            .ok_or(StageError::TarballManifestNotFound)?
            .to_owned();
        if let Some(first_stage_id) = stage_id_by_package_version
            .insert((package_name.clone(), version.clone()), item.id.clone())
        {
            return Err(StageError::DuplicateStagePackage {
                first_stage_id,
                second_stage_id: item.id.clone(),
                package_name,
                version,
            }
            .into());
        }
        let manifest = manifest_for_graph(manifest);
        projects.push(Project {
            manifest: PackageManifest::from_value(root_dir.join("package.json"), manifest),
            root_dir,
            dependency_manifest: None,
        });
    }
    let graph = create_projects_graph(
        projects.iter().map(|project| GraphPkg { project }).collect(),
        &CreateProjectsGraphOptions {
            link_workspace_packages: Some(true),
            ..CreateProjectsGraphOptions::default()
        },
    )
    .graph;
    let mut dependency_stage_ids_by_stage_id = HashMap::new();
    let mut order_index_by_stage_id = HashMap::new();
    let mut package_name_by_stage_id = HashMap::new();
    for (order_index, root_dir) in sequence_graph(&graph, &graph).order.into_iter().enumerate() {
        let stage_id = root_dir.to_string_lossy().into_owned();
        order_index_by_stage_id.insert(stage_id.clone(), order_index);
        dependency_stage_ids_by_stage_id.insert(
            stage_id.clone(),
            graph[&root_dir]
                .dependencies
                .iter()
                .map(|dependency| dependency.to_string_lossy().into_owned())
                .collect(),
        );
        if let Some(package_name) =
            graph[&root_dir].package.project.manifest.value().get("name").and_then(Value::as_str)
        {
            package_name_by_stage_id.insert(stage_id, package_name.to_owned());
        }
    }
    Ok(StageApprovalOrder {
        dependency_stage_ids: dependency_stage_ids_by_stage_id,
        order_indices: order_index_by_stage_id,
        package_names: package_name_by_stage_id,
    })
}

/// Approve staged dependencies before the selected packages that need them.
fn sort_items_for_approval(
    mut items: Vec<StageApprovalItem>,
    order: &StageApprovalOrder,
) -> Vec<StageApprovalItem> {
    items.sort_by_key(|item| order_index_of(item, order));
    items
}

/// Selected staged dependencies of `item` whose approval failed or was skipped.
fn unavailable_dependencies(
    item: &StageApprovalItem,
    unpublished_stage_ids: &HashSet<String>,
    order: &StageApprovalOrder,
) -> Vec<String> {
    order
        .dependency_stage_ids
        .get(&item.id)
        .into_iter()
        .flatten()
        .filter(|stage_id| unpublished_stage_ids.contains(*stage_id))
        .map(|stage_id| {
            order.package_names.get(stage_id).cloned().unwrap_or_else(|| stage_id.clone())
        })
        .collect()
}

fn order_index_of(item: &StageApprovalItem, order: &StageApprovalOrder) -> usize {
    order.order_indices.get(&item.id).copied().unwrap_or(usize::MAX)
}

fn manifest_for_graph(mut manifest: Value) -> Value {
    for field in ["peerDependencies", "devDependencies", "optionalDependencies", "dependencies"] {
        let Some(dependencies) = manifest.get(field).and_then(Value::as_object) else {
            continue;
        };
        let normalized = dependencies
            .iter()
            .filter_map(|(name, spec)| {
                let spec = spec.as_str()?;
                let (registry_name, registry_spec) =
                    PackageManifest::resolve_registry_dependency(name, spec);
                let (name, spec) =
                    if let Some(registry_spec) = registry_spec_for_graph(registry_spec) {
                        (registry_name, registry_spec)
                    } else {
                        (name.as_str(), spec)
                    };
                Some((name.to_owned(), Value::String(spec.to_owned())))
            })
            .collect();
        manifest[field] = Value::Object(normalized);
    }
    manifest
}

fn registry_spec_for_graph(spec: &str) -> Option<&str> {
    if Version::parse(spec).is_ok() {
        return Some(spec);
    }
    if !is_valid_semver_range(spec) {
        return None;
    }
    Some(if is_any_version_range(spec) { ANY_VERSION_RANGE } else { spec })
}

fn render_package_count(count: usize) -> String {
    format!("{count} staged package{}", if count == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests;
