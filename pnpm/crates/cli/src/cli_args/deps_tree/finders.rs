//! `--find-by` finder support: resolve finder callbacks from the
//! loaded pnpmfiles and pre-evaluate them for every package the tree
//! walk can encounter. Finders are JavaScript functions running in the
//! pnpmfile Node worker, so their verdicts are gathered up front and
//! the synchronous tree walk consults the recorded results.

use std::{collections::HashMap, path::Path, sync::Arc};

use pacquet_config::Config;
use pacquet_hooks::PnpmfileHooks;
use pacquet_store_dir::{StoreDir, StoreIndex, store_index_key};

use super::{
    TreeNodeId,
    dependents::resolve_package_nodes,
    graph::DependencyGraph,
    pkg_info::{ManifestSource, PkgInfoEnv},
    search::SearchMatch,
};

/// One resolved finder: the name it was requested by and the pnpmfile
/// hook set that exports it.
pub(crate) struct FinderHandle {
    name: String,
    hooks: Arc<dyn PnpmfileHooks>,
}

/// Resolve every `--find-by` name against the finders exported by the
/// project's pnpmfiles (the later pnpmfile wins on a name collision,
/// matching the TypeScript hook merger).
pub(crate) async fn resolve_finders(
    config: &Config,
    lockfile_dir: &Path,
    find_by: &[String],
) -> miette::Result<Vec<FinderHandle>> {
    if find_by.is_empty() {
        return Ok(Vec::new());
    }
    let pnpmfiles = crate::config_deps::load_before_packing_hooks(config, lockfile_dir);
    let mut finders_by_name: HashMap<String, Arc<dyn PnpmfileHooks>> = HashMap::new();
    for hooks in pnpmfiles {
        let names = hooks
            .get_finder_names()
            .await
            .map_err(|err| miette::miette!("loading finders from a pnpmfile: {err}"))?;
        for name in names {
            finders_by_name.insert(name, Arc::clone(&hooks));
        }
    }
    find_by
        .iter()
        .map(|name| match finders_by_name.get(name) {
            Some(hooks) => Ok(FinderHandle { name: name.clone(), hooks: Arc::clone(hooks) }),
            None => Err(miette::miette!(
                code = "ERR_PNPM_FINDER_NOT_FOUND",
                "No finder with name {name} is found"
            )),
        })
        .collect()
}

/// Every `(alias, node)` pair a finder must be evaluated for: the
/// canonical name of every resolved package node, plus each edge's
/// alias — including edges to workspace projects and unresolvable
/// link edges, which the search also visits.
pub(crate) fn finder_candidates(
    env: &PkgInfoEnv<'_>,
    graph: &DependencyGraph,
) -> Vec<(String, Option<TreeNodeId>, ManifestSource)> {
    let resolved = resolve_package_nodes(env, graph);
    let mut seen: std::collections::HashSet<(String, Option<TreeNodeId>)> =
        std::collections::HashSet::new();
    let mut candidates: Vec<(String, Option<TreeNodeId>, ManifestSource)> = Vec::new();
    let mut push = |alias: String, node_id: Option<&TreeNodeId>, source: ManifestSource| {
        if seen.insert((alias.clone(), node_id.cloned())) {
            candidates.push((alias, node_id.cloned(), source));
        }
    };

    for (node_id, source) in &resolved {
        push(source.name.clone(), Some(node_id), source.clone());
    }
    for node in graph.nodes.values() {
        for edge in &node.edges {
            match &edge.target {
                Some(target @ TreeNodeId::Package(_)) => {
                    if let Some(source) = resolved.get(target) {
                        push(edge.alias.clone(), Some(target), source.clone());
                    }
                }
                Some(target @ TreeNodeId::Importer(importer_id)) => {
                    push(
                        edge.alias.clone(),
                        Some(target),
                        ManifestSource {
                            path: env.lockfile_dir.join(importer_id),
                            integrity: None,
                            name: edge.alias.clone(),
                            version: edge.ref_display.clone(),
                        },
                    );
                }
                None => {
                    let link_target = edge.link_target.clone().unwrap_or_default();
                    push(
                        edge.alias.clone(),
                        None,
                        ManifestSource {
                            path: pacquet_fs::lexical_normalize(
                                &env.lockfile_dir.join(link_target),
                            ),
                            integrity: None,
                            name: edge.alias.clone(),
                            version: edge.ref_display.clone(),
                        },
                    );
                }
            }
        }
    }
    candidates
}

/// Run every requested finder over `candidates` and record the
/// verdicts. String results become messages (joined with `\n` when
/// several finders return one), truthy results a plain match.
pub(crate) async fn evaluate_finders(
    env: &PkgInfoEnv<'_>,
    finders: &[FinderHandle],
    candidates: Vec<(String, Option<TreeNodeId>, ManifestSource)>,
) -> miette::Result<HashMap<(String, Option<TreeNodeId>), SearchMatch>> {
    let store_index =
        env.store_dir.as_ref().and_then(|store_dir| StoreIndex::open_readonly(store_dir).ok());

    let mut results = HashMap::new();
    for (alias, node_id, source) in candidates {
        let manifest = read_manifest(env, store_index.as_ref(), &source);
        let ctx = serde_json::json!({
            "alias": alias,
            "name": source.name,
            "version": source.version,
            "manifest": manifest,
        });
        let mut messages: Vec<String> = Vec::new();
        let mut found = false;
        for finder in finders {
            let verdict = finder
                .hooks
                .run_finder(&finder.name, ctx.clone())
                .await
                .map_err(|err| miette::miette!("running finder {}: {err}", finder.name))?;
            match verdict {
                serde_json::Value::String(message) => {
                    found = true;
                    messages.push(message);
                }
                other => {
                    if truthy(&other) {
                        found = true;
                    }
                }
            }
        }
        let verdict = if !messages.is_empty() {
            SearchMatch::Message(messages.join("\n"))
        } else if found {
            SearchMatch::Yes
        } else {
            continue;
        };
        results.insert((alias, node_id), verdict);
    }
    Ok(results)
}

fn truthy(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(value) => *value,
        serde_json::Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
        serde_json::Value::String(text) => !text.is_empty(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => true,
    }
}

/// Read a package manifest, preferring the content-addressable store
/// (available even when `node_modules` was never materialized) and
/// falling back to the package directory.
fn read_manifest(
    env: &PkgInfoEnv<'_>,
    store_index: Option<&StoreIndex>,
    source: &ManifestSource,
) -> serde_json::Value {
    if let Some(manifest) = read_manifest_from_cafs(env, store_index, source) {
        return manifest;
    }
    std::fs::read(source.path.join("package.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or(serde_json::Value::Null)
}

fn read_manifest_from_cafs(
    env: &PkgInfoEnv<'_>,
    store_index: Option<&StoreIndex>,
    source: &ManifestSource,
) -> Option<serde_json::Value> {
    let store_index = store_index?;
    let integrity = source.integrity.as_deref()?;
    let store_dir = StoreDir::new(env.store_dir.as_ref()?.clone());
    let pkg_id = format!("{}@{}", source.name, source.version);
    let index = store_index.get(&store_index_key(integrity, &pkg_id)).ok()??;
    let manifest_entry = index.files.get("package.json")?;
    let manifest_path =
        store_dir.cas_file_path_by_mode(&manifest_entry.digest, manifest_entry.mode)?;
    std::fs::read(manifest_path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok())
}
