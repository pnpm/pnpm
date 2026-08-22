//! `getDependents` / `renderDependents` — the engine side of `pnpm why`.
//!
//! Wraps [`pnpm_deps_inspection`], the crate `pnpm why` itself is built on,
//! so an embedder needs neither `@pnpm/deps.inspection.tree-builder` nor
//! `@pnpm/deps.inspection.list` (nor the `@pnpm/lockfile.fs` /
//! `@pnpm/installing.modules-yaml` readers they are fed from) to answer
//! "what pulls this package in?".
//!
//! The two halves stay separate exports, mirroring the two npm packages:
//! [`get_dependents`] reads the lockfile and returns the reverse trees as
//! plain JSON, and [`render_dependents`] turns those trees into terminal,
//! parseable, or JSON output. Splitting them is what makes the TypeScript
//! `nameFormatter` callback unnecessary — the tree walk is synchronous Rust
//! and cannot call back into JS, so an embedder that renames nodes asks for
//! the manifest fields it renames by (`manifestFields`), rewrites
//! `displayName` on the returned JSON, and hands the trees back to be
//! rendered.

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

use napi_derive::napi;
use pnpm_config::matcher::create_matcher;
use pnpm_deps_inspection::{
    MAX_WALK_DEPTH,
    build::{LoadedState, importer_root_ids, read_project_manifest, safe_importer_dir},
    dependents::{BuildDependentsOptions, DependentsTree, ImporterInfo, build_dependents_tree},
    dependents_render::{
        RenderDependentsOptions, render_dependents_json, render_dependents_parseable,
        render_dependents_tree,
    },
    graph::{BuildGraphOptions, build_dependency_graph},
    search::Searcher,
};
use pnpm_modules_yaml::{DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH, IncludedDependencies};

use crate::error::report_to_napi_error;

/// Inputs for [`get_dependents`]. Mirrors [`DependentsOptions`] in `index.d.ts`.
#[napi(object)]
pub struct DependentsOptions {
    /// Lockfile / workspace root directory.
    pub dir: String,
    /// Package selectors to search for: a name, or `name@range`.
    pub packages: Vec<String>,
    /// Importer directories to walk from. Absolute, or relative to `dir`.
    /// Omitted means every importer the lockfile records.
    pub project_dirs: Option<Vec<String>>,
    /// Importer-id patterns to skip when `project_dirs` is omitted, in
    /// pnpm's `hoistPattern` glob syntax (`*` is the only wildcard). Lets a
    /// host keep its own generated importers — Bit's `.bit_roots` projects
    /// — out of the answer without reading the lockfile itself to
    /// enumerate the rest.
    pub exclude_project_patterns: Option<Vec<String>>,
    /// `node_modules` directory. Defaults to `<dir>/node_modules`.
    pub modules_dir: Option<String>,
    /// Whether to follow `dependencies` edges. Defaults to `true`.
    pub include_dependencies: Option<bool>,
    /// Whether to follow `devDependencies` edges. Defaults to `true`.
    pub include_dev_dependencies: Option<bool>,
    /// Whether to follow `optionalDependencies` edges. Defaults to `true`.
    pub include_optional_dependencies: Option<bool>,
    /// Registry routes (`{ default: url, '@scope': url }`), used to
    /// reconstruct tarball URLs. Defaults to the public npm registry.
    pub registries: Option<HashMap<String, String>>,
    /// Fallback when `.modules.yaml` records no value.
    pub virtual_store_dir_max_length: Option<u32>,
    /// `package.json` fields to project onto every package node of the
    /// tree, as `manifest`. Nodes whose manifest is missing (and every
    /// workspace-project node) carry none.
    pub manifest_fields: Option<Vec<String>>,
}

/// Inputs for [`render_dependents`]. Mirrors [`RenderDependentsOptions`]
/// in `index.d.ts`.
#[napi(object)]
pub struct RenderDependentsInput {
    /// `"tree"` (the default), `"parseable"`, or `"json"`.
    pub format: Option<String>,
    /// Max display depth. Omitted renders the whole tree.
    pub depth: Option<u32>,
    /// Include description / repository / homepage / path for each root.
    pub long: Option<bool>,
}

/// Every package matching `packages`, each with the reverse tree of what
/// depends on it. Returns an empty array when the directory has no
/// lockfile — an un-installed workspace has no dependents to report,
/// which is an answer, not an error.
#[napi]
pub async fn get_dependents(options: DependentsOptions) -> napi::Result<serde_json::Value> {
    tokio::task::spawn_blocking(move || build_trees(&options))
        .await
        .map_err(|join_error| {
            napi::Error::from_reason(format!(
                "getDependents task panicked or was cancelled: {join_error}",
            ))
        })?
        .and_then(|trees| {
            serde_json::to_value(trees).map_err(|err| {
                napi::Error::from_reason(format!("serializing the dependents trees: {err}"))
            })
        })
}

/// Render trees produced by [`get_dependents`] — after any `displayName`
/// rewriting the caller applied — as terminal, parseable, or JSON output.
/// Synchronous: rendering is pure string work over data the caller already
/// holds, apart from the `package.json` reads `long` asks for.
#[napi]
pub fn render_dependents(
    trees: serde_json::Value,
    options: Option<RenderDependentsInput>,
) -> napi::Result<String> {
    reject_over_deep_trees(&trees)?;
    let trees: Vec<DependentsTree> = serde_json::from_value(trees).map_err(|err| {
        napi::Error::from_reason(format!("the trees argument is not a dependents tree: {err}"))
    })?;
    let options =
        options.unwrap_or(RenderDependentsInput { format: None, depth: None, long: None });
    let render_opts = RenderDependentsOptions {
        long: options.long.unwrap_or(false),
        depth: options.depth.map(|depth| depth as usize),
    };
    Ok(match options.format.as_deref() {
        None | Some("tree") => render_dependents_tree(&trees, &render_opts),
        Some("parseable") => render_dependents_parseable(&trees, &render_opts),
        Some("json") => render_dependents_json(&trees, &render_opts),
        Some(other) => {
            return Err(napi::Error::from_reason(format!(
                r#"unknown dependents render format {other:?}; expected "tree", "parseable", or "json""#,
            )));
        }
    })
}

/// Refuse a caller-supplied tree nested deeper than the walk that produced
/// it could ever go. Deserialization and all three renderers recurse over
/// `dependents`, so an arbitrarily deep argument — a hand-built one, or a
/// tree from another tool — would otherwise exhaust the stack and take the
/// host process down with it.
///
/// The check itself is iterative for the same reason.
fn reject_over_deep_trees(trees: &serde_json::Value) -> napi::Result<()> {
    let mut stack: Vec<(&serde_json::Value, usize)> = vec![(trees, 0)];
    while let Some((value, depth)) = stack.pop() {
        if depth > MAX_WALK_DEPTH {
            return Err(napi::Error::from_reason(format!(
                "the trees argument nests dependents more than {MAX_WALK_DEPTH} levels deep",
            )));
        }
        match value {
            serde_json::Value::Array(items) => {
                stack.extend(items.iter().map(|item| (item, depth)));
            }
            serde_json::Value::Object(fields) => {
                if let Some(dependents) = fields.get("dependents") {
                    stack.push((dependents, depth + 1));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn build_trees(options: &DependentsOptions) -> napi::Result<Vec<DependentsTree>> {
    let lockfile_dir = PathBuf::from(&options.dir);
    let modules_dir = options.modules_dir.as_ref().map(PathBuf::from);
    let loaded = LoadedState::load(&lockfile_dir, modules_dir.as_deref(), false)
        .map_err(|report| report_to_napi_error(&report))?;

    let registries: BTreeMap<String, String> = match &options.registries {
        Some(registries) if !registries.is_empty() => {
            registries.iter().map(|(scope, url)| (scope.clone(), url.clone())).collect()
        }
        _ => BTreeMap::from([("default".to_string(), pnpm_config::default_registry())]),
    };
    let virtual_store_dir_max_length = options
        .virtual_store_dir_max_length
        .map_or(DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH as usize, |value| value as usize);

    // No lockfile: nothing is installed, so nothing depends on anything.
    let Some(env) =
        loaded.env(&lockfile_dir, virtual_store_dir_max_length, &registries, BTreeMap::new())
    else {
        return Ok(Vec::new());
    };
    let lockfile = env.current_lockfile;

    let project_dirs: Vec<PathBuf> = if let Some(dirs) = &options.project_dirs {
        dirs.iter().map(|dir| resolve_project_dir(&lockfile_dir, dir)).collect()
    } else {
        let excluded =
            create_matcher(options.exclude_project_patterns.as_deref().unwrap_or_default());
        lockfile
            .importers
            .keys()
            .filter(|importer_id| !excluded.matches(importer_id))
            .filter_map(|importer_id| safe_importer_dir(&lockfile_dir, importer_id))
            .collect()
    };

    let mut importer_info: HashMap<String, ImporterInfo> = HashMap::new();
    for importer_id in lockfile.importers.keys() {
        // A key that cannot be safely joined (a malformed or hostile
        // lockfile) is never dereferenced; the raw key still names the
        // importer in the output.
        let manifest = safe_importer_dir(&lockfile_dir, importer_id)
            .map(|importer_dir| read_project_manifest(&importer_dir))
            .unwrap_or_default();
        let name = manifest.name.unwrap_or_else(|| {
            if importer_id == "." { "the root project".to_string() } else { importer_id.clone() }
        });
        importer_info.insert(
            importer_id.clone(),
            ImporterInfo { name, version: manifest.version.unwrap_or_default() },
        );
    }

    let include = IncludedDependencies {
        dependencies: options.include_dependencies.unwrap_or(true),
        dev_dependencies: options.include_dev_dependencies.unwrap_or(true),
        optional_dependencies: options.include_optional_dependencies.unwrap_or(true),
    };
    let root_ids = importer_root_ids(lockfile, &lockfile_dir, &project_dirs);
    let graph = build_dependency_graph(
        &root_ids,
        &BuildGraphOptions { lockfile, include, only_projects: false },
    );
    let searcher = Searcher::from_queries(&options.packages)
        .map_err(|report| report_to_napi_error(&report))?;
    let manifest_fields = options.manifest_fields.clone().unwrap_or_default();

    Ok(build_dependents_tree(&BuildDependentsOptions {
        env: &env,
        graph: &graph,
        search: &searcher,
        importer_info: &importer_info,
        manifest_fields: &manifest_fields,
    }))
}

fn resolve_project_dir(lockfile_dir: &Path, dir: &str) -> PathBuf {
    let dir = Path::new(dir);
    if dir.is_absolute() { dir.to_path_buf() } else { lockfile_dir.join(dir) }
}

#[cfg(test)]
mod tests;
