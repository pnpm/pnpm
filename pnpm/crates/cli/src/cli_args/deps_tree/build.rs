//! Assemble the tree-build environment from the lockfiles and modules
//! manifest, and build per-project dependency hierarchies. Rust
//! counterpart of the TypeScript tree-builder's
//! `buildDependenciesTree`.

use std::{
    collections::{HashMap, HashSet},
    io,
    path::{Path, PathBuf},
};

use miette::{Context, IntoDiagnostic};
use pacquet_fs::lexical_normalize;
use pacquet_lockfile::{Lockfile, ProjectSnapshot};
use pacquet_modules_yaml::{
    DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH, Host, IncludedDependencies, Modules,
    read_modules_manifest,
};

use super::{
    DependencyNode, TreeNodeId,
    dep_types::detect_dep_types,
    get_tree::{GetTreeOptions, MaterializationCache, MaxDepth, get_tree},
    graph::{BuildGraphOptions, DependencyGraph, build_dependency_graph},
    pkg_info::PkgInfoEnv,
    search::Searcher,
};

pub(crate) const DEFAULT_REGISTRY: &str = "https://registry.npmjs.org/";

/// The lockfiles and modules-manifest state one tree build runs
/// against. Owns the loaded lockfiles; [`LoadedState::env`] borrows them.
pub(crate) struct LoadedState {
    pub modules_dir: PathBuf,
    pub modules: Option<Modules>,
    pub current_lockfile: Option<Lockfile>,
    pub wanted_lockfile: Option<Lockfile>,
    pub check_wanted_lockfile_only: bool,
}

impl LoadedState {
    pub(crate) fn load(
        lockfile_dir: &Path,
        modules_dir_opt: Option<&Path>,
        check_wanted_lockfile_only: bool,
    ) -> miette::Result<LoadedState> {
        let modules_dir_raw = match modules_dir_opt {
            Some(dir) if dir.is_absolute() => dir.to_path_buf(),
            Some(dir) => lockfile_dir.join(dir),
            None => lockfile_dir.join("node_modules"),
        };
        let modules_dir = realpath_missing(&modules_dir_raw);
        let modules = read_modules_manifest::<Host>(&modules_dir)
            .into_diagnostic()
            .wrap_err("read the modules manifest")?;
        let current_lockfile =
            Lockfile::load_current_from_virtual_store_dir(&modules_dir.join(".pnpm"))
                .into_diagnostic()
                .wrap_err("load the current lockfile")?;
        let wanted_lockfile = Lockfile::load_wanted_from_dir(lockfile_dir)
            .into_diagnostic()
            .wrap_err("load the wanted lockfile")?;
        Ok(LoadedState {
            modules_dir,
            modules,
            current_lockfile,
            wanted_lockfile,
            check_wanted_lockfile_only,
        })
    }

    /// The lockfile the tree is built from: the wanted lockfile under
    /// `--lockfile-only`, otherwise the current lockfile with the
    /// wanted one as fallback.
    pub(crate) fn lockfile_to_use(&self) -> Option<&Lockfile> {
        if self.check_wanted_lockfile_only {
            self.wanted_lockfile.as_ref()
        } else {
            self.current_lockfile.as_ref().or(self.wanted_lockfile.as_ref())
        }
    }

    pub(crate) fn env<'a>(
        &'a self,
        lockfile_dir: &Path,
        virtual_store_dir_max_length: usize,
    ) -> Option<PkgInfoEnv<'a>> {
        let lockfile = self.lockfile_to_use()?;
        let mut registries = HashMap::new();
        registries.insert("default".to_string(), DEFAULT_REGISTRY.to_string());
        if let Some(modules_registries) =
            self.modules.as_ref().and_then(|modules| modules.registries.as_ref())
        {
            for (key, url) in modules_registries {
                registries.insert(key.clone(), url.clone());
            }
        }
        let virtual_store_dir = match &self.modules {
            Some(modules) if !modules.virtual_store_dir.is_empty() => {
                let dir = PathBuf::from(&modules.virtual_store_dir);
                if dir.is_absolute() { dir } else { self.modules_dir.join(dir) }
            }
            _ => self.modules_dir.join(".pnpm"),
        };
        Some(PkgInfoEnv {
            lockfile_dir: lockfile_dir.to_path_buf(),
            modules_dir: self.modules_dir.clone(),
            virtual_store_dir,
            virtual_store_dir_max_length: self.modules.as_ref().map_or(
                virtual_store_dir_max_length,
                |modules| {
                    usize::try_from(modules.virtual_store_dir_max_length)
                        .unwrap_or(DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH as usize)
                },
            ),
            registries,
            skipped: self
                .modules
                .as_ref()
                .map(|modules| modules.skipped.iter().cloned().collect::<HashSet<_>>())
                .unwrap_or_default(),
            store_dir: self
                .modules
                .as_ref()
                .map(|modules| PathBuf::from(&modules.store_dir))
                .filter(|dir| !dir.as_os_str().is_empty()),
            current_lockfile: lockfile,
            wanted_lockfile: self.wanted_lockfile.as_ref(),
            dep_types: detect_dep_types(lockfile),
        })
    }
}

/// One project's categorized dependency hierarchy — the input of the
/// `list` renderers.
#[derive(Debug, Default)]
pub(crate) struct DependenciesHierarchy {
    pub dependencies: Vec<DependencyNode>,
    pub dev_dependencies: Vec<DependencyNode>,
    pub optional_dependencies: Vec<DependencyNode>,
    pub unsaved_dependencies: Vec<DependencyNode>,
}

pub(crate) struct BuildTreeOptions<'a> {
    pub lockfile_dir: &'a Path,
    pub depth: MaxDepth,
    pub include: IncludedDependencies,
    pub exclude_peer_dependencies: bool,
    pub only_projects: bool,
    pub search: Option<&'a Searcher>,
    pub show_deduped_search_matches: bool,
    pub modules_dir_opt: Option<&'a Path>,
}

/// Build the dependency hierarchy of every project in `project_dirs`,
/// sharing one graph and one materialization cache so identical
/// subtrees are only expanded once across projects.
pub(crate) fn build_dependencies_tree(
    state: &LoadedState,
    env: &PkgInfoEnv<'_>,
    project_dirs: &[PathBuf],
    opts: &BuildTreeOptions<'_>,
) -> miette::Result<Vec<(PathBuf, DependenciesHierarchy)>> {
    let lockfile = env.current_lockfile;

    let root_ids: Vec<TreeNodeId> = project_dirs
        .iter()
        .map(|dir| TreeNodeId::Importer(importer_id_for(opts.lockfile_dir, dir)))
        .filter(|id| match id {
            TreeNodeId::Importer(importer_id) => {
                lockfile.importers.contains_key(importer_id.as_str())
            }
            TreeNodeId::Package(_) => false,
        })
        .collect();

    let graph = build_dependency_graph(
        &root_ids,
        &BuildGraphOptions { lockfile, include: opts.include, only_projects: opts.only_projects },
    );
    let mut cache: MaterializationCache = MaterializationCache::new();

    let mut result = Vec::with_capacity(project_dirs.len());
    for project_dir in project_dirs {
        let hierarchy = hierarchy_for_project(state, env, &graph, &mut cache, project_dir, opts)?;
        result.push((project_dir.clone(), hierarchy));
    }
    Ok(result)
}

fn hierarchy_for_project(
    state: &LoadedState,
    env: &PkgInfoEnv<'_>,
    graph: &DependencyGraph,
    cache: &mut MaterializationCache,
    project_dir: &Path,
    opts: &BuildTreeOptions<'_>,
) -> miette::Result<DependenciesHierarchy> {
    let importer_id = importer_id_for(opts.lockfile_dir, project_dir);
    let Some(importer) = env.current_lockfile.importers.get(importer_id.as_str()) else {
        return Ok(DependenciesHierarchy::default());
    };

    let project_modules_dir = match opts.modules_dir_opt {
        Some(dir) if dir.is_absolute() => dir.to_path_buf(),
        Some(dir) => project_dir.join(dir),
        None => project_dir.join("node_modules"),
    };

    let mut hierarchy = DependenciesHierarchy::default();

    let get_tree_opts = GetTreeOptions {
        env,
        graph,
        exclude_peer_dependencies: opts.exclude_peer_dependencies,
        only_projects: opts.only_projects,
        search: opts.search,
        show_deduped_search_matches: opts.show_deduped_search_matches,
        rewrite_link_version_dir: project_dir.to_path_buf(),
    };
    // The importer itself is one level: `opts.depth` counts levels
    // below the direct dependencies.
    let max_depth = match opts.depth {
        MaxDepth::Finite(depth) => MaxDepth::Finite(depth.saturating_add(1)),
        MaxDepth::Unlimited => MaxDepth::Unlimited,
    };
    let nodes = get_tree(
        &get_tree_opts,
        cache,
        &TreeNodeId::Importer(importer_id.clone()),
        max_depth,
        None,
    );

    let field_of = field_map(importer, opts.include);
    for node in nodes {
        match field_of.get(node.alias.as_str()) {
            Some(DependenciesField::Dependencies) => hierarchy.dependencies.push(node),
            Some(DependenciesField::DevDependencies) => hierarchy.dev_dependencies.push(node),
            Some(DependenciesField::OptionalDependencies) => {
                hierarchy.optional_dependencies.push(node);
            }
            None => {}
        }
    }

    // Unsaved (extraneous) dependencies: packages present in the
    // project's modules dir but absent from its lockfile entry. They
    // are irrelevant while searching — they are not in the lockfile
    // graph and cannot contain paths to the search target.
    if opts.search.is_none() {
        hierarchy.unsaved_dependencies =
            read_unsaved_dependencies(importer, project_dir, &project_modules_dir)?;
    }

    let _ = state;
    Ok(hierarchy)
}

#[derive(Debug, Clone, Copy)]
enum DependenciesField {
    Dependencies,
    DevDependencies,
    OptionalDependencies,
}

fn field_map(
    importer: &ProjectSnapshot,
    include: IncludedDependencies,
) -> HashMap<String, DependenciesField> {
    let mut map = HashMap::new();
    let groups: [(bool, Option<&pacquet_lockfile::ResolvedDependencyMap>, DependenciesField); 3] = [
        (include.dependencies, importer.dependencies.as_ref(), DependenciesField::Dependencies),
        (
            include.dev_dependencies,
            importer.dev_dependencies.as_ref(),
            DependenciesField::DevDependencies,
        ),
        (
            include.optional_dependencies,
            importer.optional_dependencies.as_ref(),
            DependenciesField::OptionalDependencies,
        ),
    ];
    for (included, group, field) in groups {
        if !included {
            continue;
        }
        for alias in group.into_iter().flatten().map(|(alias, _)| alias) {
            map.insert(alias.to_string(), field);
        }
    }
    map
}

/// The importer id of `project_dir` relative to the lockfile root.
pub(crate) fn importer_id_for(lockfile_dir: &Path, project_dir: &Path) -> String {
    pacquet_workspace::importer_id_from_root_dir(lockfile_dir, project_dir)
}

/// Resolve symlinks in the deepest existing ancestor of `path`,
/// re-appending the missing tail (counterpart of the `realpath-missing`
/// package).
pub(crate) fn realpath_missing(path: &Path) -> PathBuf {
    let mut current = path.to_path_buf();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    loop {
        match dunce::canonicalize(&current) {
            Ok(mut real) => {
                for component in tail.iter().rev() {
                    real.push(component);
                }
                return real;
            }
            Err(_) => match (current.parent(), current.file_name()) {
                (Some(parent), Some(name)) => {
                    tail.push(name.to_os_string());
                    current = parent.to_path_buf();
                }
                _ => return lexical_normalize(path),
            },
        }
    }
}

/// Scan the project's modules dir for packages absent from its
/// lockfile entry (npm's "extraneous" dependencies), building a leaf
/// [`DependencyNode`] for each.
fn read_unsaved_dependencies(
    importer: &ProjectSnapshot,
    project_dir: &Path,
    modules_dir: &Path,
) -> miette::Result<Vec<DependencyNode>> {
    let saved = saved_direct_dep_names(importer);
    let unsaved: Vec<DependencyNode> = read_modules_dir_names(modules_dir)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to read {}", modules_dir.display()))?
        .into_iter()
        .filter(|name| !saved.contains(name))
        .map(|name| build_unsaved_node(&name, modules_dir, project_dir))
        .collect();
    Ok(unsaved)
}

/// The alias keys of an importer's `dependencies`, `devDependencies`,
/// and `optionalDependencies` — always spanning all three groups
/// regardless of the include filter, matching the TypeScript CLI's
/// `getAllDirectDependencies`.
fn saved_direct_dep_names(importer: &ProjectSnapshot) -> HashSet<String> {
    let mut names = HashSet::new();
    let groups =
        [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies];
    for group in groups.into_iter().flatten() {
        for name in group.keys() {
            names.insert(name.to_string());
        }
    }
    names
}

/// The package directory names directly under `modules_dir`, following
/// the same enumeration rules as the TypeScript CLI's `readModulesDir`.
/// A missing `modules_dir` is not an error; any other read failure is.
fn read_modules_dir_names(modules_dir: &Path) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    collect_module_names(modules_dir, None, &mut names)?;
    Ok(names)
}

fn collect_module_names(
    modules_dir: &Path,
    scope: Option<&str>,
    names: &mut Vec<String>,
) -> io::Result<()> {
    let parent_dir = match scope {
        Some(scope) => modules_dir.join(scope),
        None => modules_dir.to_path_buf(),
    };
    let entries = match std::fs::read_dir(&parent_dir) {
        Ok(entries) => entries,
        // A missing directory is "no packages"; every other error is
        // surfaced, matching `readModulesDir`, which only swallows ENOENT.
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        if entry.file_type().is_ok_and(|file_type| file_type.is_file()) {
            continue;
        }
        if scope.is_none() && name.starts_with('@') {
            collect_module_names(modules_dir, Some(name), names)?;
            continue;
        }
        match scope {
            Some(scope) => names.push(format!("{scope}/{name}")),
            None => names.push(name.to_string()),
        }
    }
    Ok(())
}

/// Build the leaf [`DependencyNode`] for one extraneous package, taking
/// its version and path from the symlink target for a `link:`
/// dependency or from `package.json` for a regular directory.
fn build_unsaved_node(name: &str, modules_dir: &Path, project_dir: &Path) -> DependencyNode {
    let entry_path = modules_dir.join(name);
    let (path, version) = if let Some(target) = resolve_link_target(&entry_path) {
        let relative = pathdiff::diff_paths(&target, project_dir).unwrap_or_else(|| target.clone());
        let version = format!("link:{}", relative.to_string_lossy().replace('\\', "/"));
        (target.to_string_lossy().into_owned(), version)
    } else {
        let version = read_package_version(&entry_path).unwrap_or_else(|| "undefined".to_string());
        (entry_path.to_string_lossy().into_owned(), version)
    };
    DependencyNode {
        alias: name.to_string(),
        name: name.to_string(),
        version,
        path,
        ..DependencyNode::default()
    }
}

/// Resolve a symlink to the absolute path of its immediate target,
/// lexically (without requiring the target to exist), matching the
/// TypeScript CLI's `resolveLinkTarget`. `None` when `link` is not a
/// symlink.
fn resolve_link_target(link: &Path) -> Option<PathBuf> {
    let target = std::fs::read_link(link).ok()?;
    let joined = if target.is_absolute() {
        target
    } else {
        match link.parent() {
            Some(parent) => parent.join(target),
            None => target,
        }
    };
    Some(lexical_normalize(&joined))
}

fn read_package_version(pkg_dir: &Path) -> Option<String> {
    let bytes = std::fs::read(pkg_dir.join("package.json")).ok()?;
    let manifest: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    manifest.get("version").and_then(serde_json::Value::as_str).map(str::to_string)
}

#[derive(Debug, Default)]
pub(crate) struct ProjectManifestSummary {
    pub name: Option<String>,
    pub version: Option<String>,
    pub private: bool,
}

/// Read `name` / `version` / `private` from a project manifest,
/// treating an unreadable manifest as empty (mirroring the TypeScript
/// `safeReadProjectManifestOnly`).
pub(crate) fn read_project_manifest(project_dir: &Path) -> ProjectManifestSummary {
    let Ok(bytes) = std::fs::read(project_dir.join("package.json")) else {
        return ProjectManifestSummary::default();
    };
    let Ok(manifest) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return ProjectManifestSummary::default();
    };
    ProjectManifestSummary {
        name: manifest.get("name").and_then(serde_json::Value::as_str).map(str::to_string),
        version: manifest.get("version").and_then(serde_json::Value::as_str).map(str::to_string),
        private: manifest.get("private").and_then(serde_json::Value::as_bool).unwrap_or(false),
    }
}
