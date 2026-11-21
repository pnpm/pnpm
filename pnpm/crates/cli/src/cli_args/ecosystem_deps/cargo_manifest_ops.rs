pub(crate) use saved_spec::{
    format_cargo_saved_spec,
    without_build_metadata,
};
mod saved_spec;

use crate::{
    cargo_manifest::{
        ConfiguredWorkspace,
        configured_workspace_state,
        lookup_inherited_workspace_entry,
        resolve_local_path,
        upsert_dependency_in_dir,
        workspace_includes_member,
    },
    cli_args::outdated::OutdatedQuery,
};
use cargo_lock::Lockfile as CargoLockfile;
use miette::{
    IntoDiagnostic,
    Result,
    WrapErr,
};
use pnpm_matcher::Matcher;
use pnpm_package_manifest::DependencyGroup;
use semver::{
    Version,
    VersionReq,
};
use std::{
    collections::{
        BTreeMap,
        HashMap,
    },
    fs,
    path::{
        Path,
        PathBuf,
    },
    str::FromStr,
};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone)]
pub(crate) struct DeclaredCargoDep {
    pub(crate) aliases: Vec<String>,
    pub(crate) manifest_key: String,
    pub(crate) package_name: String,
    pub(crate) raw_spec: String,
    pub(crate) version_req: VersionReq,
    pub(crate) group: DependencyGroup,
    pub(crate) manifest_path: PathBuf,
    /// The keys leading to the dependency table, such as
    /// `["target", "cfg(unix)", "dependencies"]`.
    pub(crate) table: Vec<String>,
}

impl DeclaredCargoDep {
    pub(crate) fn primary_alias(&self) -> &str {
        &self.aliases[0]
    }

    pub(crate) fn record_alias(&mut self, alias: &str) {
        if !self.has_alias(alias) {
            self.aliases.push(alias.to_string());
        }
    }

    pub(crate) fn merge_aliases_from(&mut self, other: &Self) {
        for alias in &other.aliases {
            self.record_alias(alias);
        }
    }

    pub(crate) fn has_alias(&self, name: &str) -> bool {
        self.aliases
            .iter()
            .any(|alias| alias == name)
    }

    pub(crate) fn identities(&self) -> Vec<&str> {
        let mut ids = vec![self.package_name.as_str(), self.manifest_key.as_str()];
        for alias in &self.aliases {
            if !ids.contains(&alias.as_str()) {
                ids.push(alias.as_str());
            }
        }
        ids
    }

    pub(crate) fn matches_name(&self, name: &str) -> bool {
        self.package_name == name || self.manifest_key == name || self.has_alias(name)
    }

    pub(crate) fn matches_wildcard(&self, matcher: &pnpm_matcher::WildcardMatcher) -> bool {
        matcher.matches(&self.package_name)
            || matcher.matches(&self.manifest_key)
            || self.aliases
                .iter()
                .any(|alias| matcher.matches(alias))
    }

    pub(crate) fn is_ignored(&self, matcher: &Matcher) -> bool {
        matcher.matches_any(&self.identities())
    }

    pub(crate) fn matches_pattern(&self, matcher: &Matcher) -> bool {
        let aliases: Vec<&str> = self.aliases
            .iter()
            .map(String::as_str)
            .collect();
        matcher.matches_any(&aliases)
    }

    pub(crate) fn matches_query(&self, query: &OutdatedQuery<'_>) -> bool {
        let ids = self.identities();
        query.match_names.is_none_or(|matcher| matcher.matches_any(&ids))
            && !query.ignore_names.is_some_and(|matcher| matcher.matches_any(&ids))
    }
}

const DEPENDENCY_KINDS: [(&str, DependencyGroup); 3] = [
    ("dependencies", DependencyGroup::Prod),
    ("dev-dependencies", DependencyGroup::Dev),
    ("build-dependencies", DependencyGroup::Dev),
];

pub(crate) fn cargo_project_name(project_dir: &Path) -> Option<String> {
    let contents = fs::read_to_string(project_dir.join("Cargo.toml")).ok()?;
    let doc: toml::Table = toml::from_str(&contents).ok()?;
    doc.get("package")?
        .as_table()?
        .get("name")?
        .as_str()
        .map(str::to_string)
}

pub(crate) fn has_matching_cargo_deps(
    project_dir: &Path,
    workspace_limit: Option<&Path>,
    query: &OutdatedQuery<'_>,
) -> Result<bool> {
    Ok(declared_cargo_deps(project_dir, workspace_limit)?
        .into_iter()
        .any(|dep| query.include_direct.contains(&dep.group) && dep.matches_query(query)))
}

pub(crate) fn find_cargo_workspace_root(
    project_dir: &Path,
    workspace_limit: Option<&Path>,
) -> PathBuf {
    if has_workspace_section(project_dir) {
        return project_dir.to_path_buf();
    }
    match configured_workspace_state(project_dir) {
        ConfiguredWorkspace::Path(ws_dir) => {
            let within_limit =
                workspace_limit.is_none_or(|limit| ws_dir.starts_with(resolve_local_path(limit)));
            if within_limit && workspace_includes_member(&ws_dir, project_dir) {
                ws_dir
            } else {
                project_dir.to_path_buf()
            }
        }
        ConfiguredWorkspace::Invalid => project_dir.to_path_buf(),
        ConfiguredWorkspace::Unset => find_ancestor_workspace_root(project_dir, workspace_limit),
    }
}

fn find_ancestor_workspace_root(project_dir: &Path, workspace_limit: Option<&Path>) -> PathBuf {
    let mut current = project_dir;
    while workspace_limit.is_none_or(|limit| current != limit) {
        let Some(parent) = current.parent() else {
            break;
        };
        current = parent;
        if workspace_includes_member(current, project_dir) {
            return current.to_path_buf();
        }
    }
    project_dir.to_path_buf()
}

fn has_workspace_section(dir: &Path) -> bool {
    let Ok(contents) = fs::read_to_string(dir.join("Cargo.toml")) else {
        return false;
    };
    toml::from_str::<toml::Table>(&contents).is_ok_and(|table| table.contains_key("workspace"))
}

pub(crate) fn declared_cargo_deps(
    project_dir: &Path,
    workspace_limit: Option<&Path>,
) -> Result<Vec<DeclaredCargoDep>> {
    let manifest_path = project_dir.join("Cargo.toml");
    if !manifest_path.is_file() {
        return Ok(Vec::new());
    }
    let manifest_path = dunce::canonicalize(&manifest_path).unwrap_or(manifest_path);
    let cargo_root = find_cargo_workspace_root(project_dir, workspace_limit);
    let root_manifest_path = cargo_root.join("Cargo.toml");
    let root_manifest_path = dunce::canonicalize(&root_manifest_path).unwrap_or(root_manifest_path);
    let workspace_specs = read_workspace_dependency_specs(&root_manifest_path)?;
    let contents = fs::read_to_string(&manifest_path)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", manifest_path.display()))?;
    let doc: toml::Table = toml::from_str(&contents).into_diagnostic()?;
    let mut out = Vec::new();
    for (table_path, table, group) in dependency_tables(&doc) {
        collect_table_deps(
            table,
            &table_path,
            group,
            &manifest_path,
            (&root_manifest_path, &workspace_specs),
            &mut out,
        );
    }
    Ok(out)
}

/// The dependency tables of a manifest with their paths: the top-level ones,
/// the ones under each `[target.<cfg or triple>]`, and
/// `[workspace.dependencies]`.
fn dependency_tables(doc: &toml::Table) -> Vec<(Vec<String>, &toml::Table, DependencyGroup)> {
    let targets = doc
        .get("target")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flatten()
        .filter_map(|(target, scope)| {
            Some((vec!["target".to_string(), target.clone()], scope.as_table()?))
        });
    let mut tables = Vec::new();
    for (prefix, scope) in std::iter::once((Vec::new(), doc)).chain(targets) {
        for (kind, group) in DEPENDENCY_KINDS {
            if let Some(table) = scope.get(kind).and_then(toml::Value::as_table) {
                let mut path = prefix.clone();
                path.push(kind.to_string());
                tables.push((path, table, group));
            }
        }
    }
    if let Some(table) = lookup_table(doc, "workspace.dependencies") {
        tables.push((workspace_dependencies_table(), table, DependencyGroup::Prod));
    }
    tables
}

fn workspace_dependencies_table() -> Vec<String> {
    vec!["workspace".to_string(), "dependencies".to_string()]
}

fn lookup_table<'a>(doc: &'a toml::Table, table_name: &str) -> Option<&'a toml::Table> {
    if let Some((first, second)) = table_name.split_once('.') {
        doc.get(first)?
            .as_table()?
            .get(second)?
            .as_table()
    } else {
        doc.get(table_name)?.as_table()
    }
}

fn read_workspace_dependency_specs(
    root_manifest_path: &Path,
) -> Result<HashMap<String, (String, String)>> {
    if !root_manifest_path.is_file() {
        return Ok(HashMap::new());
    }
    let contents = fs::read_to_string(root_manifest_path).into_diagnostic()?;
    let doc: toml::Table = toml::from_str(&contents).into_diagnostic()?;
    let Some(table) = lookup_table(&doc, "workspace.dependencies") else {
        return Ok(HashMap::new());
    };
    let mut specs = HashMap::new();
    for (alias, value) in table {
        if let Some((pkg_name, raw_spec)) = extract_registry_spec(alias, value) {
            specs.insert(alias.clone(), (pkg_name, raw_spec));
        }
    }
    Ok(specs)
}

fn collect_table_deps(
    table: &toml::Table,
    table_path: &[String],
    group: DependencyGroup,
    manifest_path: &Path,
    (root_manifest_path, workspace_specs): (&Path, &HashMap<String, (String, String)>),
    out: &mut Vec<DeclaredCargoDep>,
) {
    for (alias, value) in table {
        let resolved = if is_workspace_inherited(value) {
            resolve_inherited_dep(alias, value, root_manifest_path.to_path_buf(), workspace_specs)
        } else {
            extract_registry_spec(alias, value)
                .map(|(pkg_name, raw)| {
                    (alias.clone(), pkg_name, raw, manifest_path.to_path_buf(), table_path.to_vec())
                })
        };
        let Some((manifest_key, package_name, raw_spec, target_manifest, target_table)) = resolved
        else {
            continue;
        };
        if let Some(existing) = out
            .iter_mut()
            .find(|existing| {
                existing.manifest_path == target_manifest
                    && existing.table == target_table
                    && existing.manifest_key == manifest_key
                    && existing.group == group
            })
        {
            existing.record_alias(alias);
            continue;
        }
        let Ok(version_req) = VersionReq::parse(&raw_spec) else {
            continue;
        };
        out.push(DeclaredCargoDep {
            aliases: vec![alias.clone()],
            manifest_key,
            package_name,
            raw_spec,
            version_req,
            group,
            manifest_path: target_manifest,
            table: target_table,
        });
    }
}

fn resolve_inherited_dep(
    alias: &str,
    value: &toml::Value,
    root_manifest_path: PathBuf,
    workspace_specs: &HashMap<String, (String, String)>,
) -> Option<(String, String, String, PathBuf, Vec<String>)> {
    let local_package = value
        .as_table()
        .and_then(|dep| dep.get("package"))
        .and_then(toml::Value::as_str);
    let (ws_key, (pkg_name, raw)) =
        lookup_inherited_workspace_entry(alias, local_package, |key| {
            workspace_specs.get_key_value(key)
        })?;
    Some((
        ws_key.clone(),
        pkg_name.clone(),
        raw.clone(),
        root_manifest_path,
        workspace_dependencies_table(),
    ))
}

fn is_workspace_inherited(value: &toml::Value) -> bool {
    value
        .as_table()
        .and_then(|table| table.get("workspace"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(false)
}

fn extract_registry_spec(alias: &str, value: &toml::Value) -> Option<(String, String)> {
    if let Some(version) = value.as_str() {
        return Some((alias.to_string(), version.to_string()));
    }
    let table = value.as_table()?;
    if table.contains_key("git") || table.contains_key("path") || table.contains_key("registry") {
        return None;
    }
    let version = table
        .get("version")?
        .as_str()?
        .to_string();
    let package_name = table
        .get("package")
        .and_then(toml::Value::as_str)
        .unwrap_or(alias)
        .to_string();
    Some((package_name, version))
}

pub(crate) fn read_locked_cargo_versions(
    cargo_lock_path: &Path,
) -> Result<HashMap<String, Vec<Version>>> {
    let contents = fs::read_to_string(cargo_lock_path)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", cargo_lock_path.display()))?;
    let lockfile = CargoLockfile::from_str(&contents)
        .into_diagnostic()
        .wrap_err_with(|| format!("parse {}", cargo_lock_path.display()))?;
    let mut map: HashMap<String, Vec<Version>> = HashMap::new();
    for pkg in lockfile.packages {
        if pkg.source.as_ref().is_some_and(cargo_lock::SourceId::is_registry) {
            map.entry(pkg.name.as_str().to_string())
                .or_default()
                .push(pkg.version);
        }
    }
    for versions in map.values_mut() {
        versions.sort();
    }
    Ok(map)
}

pub(crate) fn select_locked_version<'a>(
    locked: &'a HashMap<String, Vec<Version>>,
    dep: &DeclaredCargoDep,
) -> Option<&'a Version> {
    let versions = locked.get(&dep.package_name)?;
    versions
        .iter()
        .rev()
        .find(|version| dep.version_req.matches(version))
        .or_else(|| (versions.len() == 1).then(|| &versions[0]))
}

pub(crate) fn same_declared_dep(lhs: &DeclaredCargoDep, rhs: &DeclaredCargoDep) -> bool {
    lhs.manifest_key == rhs.manifest_key
        && lhs.table == rhs.table
        && lhs.manifest_path == rhs.manifest_path
}

/// A manifest edit: the manifest, the keys leading to its dependency table,
/// the dependency key, and the requirement to write.
pub(crate) type ManifestEdit = (PathBuf, Vec<String>, String, String);

pub(crate) fn apply_manifest_edits(edits: &[ManifestEdit]) -> Result<()> {
    let mut by_file: BTreeMap<&Path, Vec<&ManifestEdit>> = BTreeMap::new();
    for edit in edits {
        by_file
            .entry(edit.0.as_path())
            .or_default()
            .push(edit);
    }
    for (path, file_edits) in by_file {
        let mut contents = fs::read_to_string(path)
            .into_diagnostic()
            .wrap_err_with(|| format!("read {}", path.display()))?;
        for (_, table, alias, spec) in file_edits {
            let table = table
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            contents = upsert_dependency_in_dir(&contents, path.parent(), &table, alias, spec)?;
        }
        pnpm_fs::write_atomic(path, contents.as_bytes()).into_diagnostic()?;
    }
    Ok(())
}
