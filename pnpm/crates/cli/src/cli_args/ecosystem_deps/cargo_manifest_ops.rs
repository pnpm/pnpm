use crate::{cargo_manifest::upsert_dependency, cli_args::outdated::OutdatedQuery};
use cargo_lock::Lockfile as CargoLockfile;
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pnpm_package_manifest::DependencyGroup;
use semver::{Version, VersionReq};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

#[derive(Debug, Clone)]
pub(crate) struct DeclaredCargoDep {
    pub(crate) alias: String,
    pub(crate) package_name: String,
    pub(crate) raw_spec: String,
    pub(crate) version_req: VersionReq,
    pub(crate) group: DependencyGroup,
    pub(crate) manifest_path: PathBuf,
    pub(crate) table: &'static str,
}

const TABLES: [(&str, DependencyGroup); 4] = [
    ("dependencies", DependencyGroup::Prod),
    ("dev-dependencies", DependencyGroup::Dev),
    ("build-dependencies", DependencyGroup::Dev),
    ("workspace.dependencies", DependencyGroup::Prod),
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
        .any(|dep| {
            query.include_direct.contains(&dep.group)
                && (query.matches_name(&dep.alias) || query.matches_name(&dep.package_name))
        }))
}

pub(crate) fn find_cargo_workspace_root(
    project_dir: &Path,
    workspace_limit: Option<&Path>,
) -> PathBuf {
    let mut candidate = project_dir.to_path_buf();
    let mut current = project_dir;
    loop {
        if current.join("Cargo.lock").is_file() || has_workspace_section(current) {
            candidate = current.to_path_buf();
        }
        if workspace_limit.is_some_and(|limit| current == limit) {
            break;
        }
        let Some(parent) = current.parent() else {
            break;
        };
        current = parent;
    }
    candidate
}

fn has_workspace_section(dir: &Path) -> bool {
    let Ok(contents) = fs::read_to_string(dir.join("Cargo.toml")) else {
        return false;
    };
    toml::from_str::<toml::Table>(&contents)
        .ok()
        .is_some_and(|table| table.contains_key("workspace"))
}

pub(crate) fn declared_cargo_deps(
    project_dir: &Path,
    workspace_limit: Option<&Path>,
) -> Result<Vec<DeclaredCargoDep>> {
    let manifest_path = project_dir.join("Cargo.toml");
    if !manifest_path.is_file() {
        return Ok(Vec::new());
    }
    let cargo_root = find_cargo_workspace_root(project_dir, workspace_limit);
    let root_manifest_path = cargo_root.join("Cargo.toml");
    let workspace_specs = read_workspace_dependency_specs(&root_manifest_path)?;
    let contents = fs::read_to_string(&manifest_path)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", manifest_path.display()))?;
    let doc: toml::Table = toml::from_str(&contents).into_diagnostic()?;
    let mut out = Vec::new();
    for (table_name, group) in TABLES {
        let Some(table) = lookup_table(&doc, table_name) else {
            continue;
        };
        collect_table_deps(
            table,
            table_name,
            group,
            &manifest_path,
            (&root_manifest_path, &workspace_specs),
            &mut out,
        );
    }
    Ok(out)
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
    table_name: &'static str,
    group: DependencyGroup,
    manifest_path: &Path,
    (root_manifest_path, workspace_specs): (&Path, &HashMap<String, (String, String)>),
    out: &mut Vec<DeclaredCargoDep>,
) {
    for (alias, value) in table {
        let resolved = if is_workspace_inherited(value) {
            workspace_specs
                .get(alias)
                .cloned()
                .map(|(pkg_name, raw)| {
                    (pkg_name, raw, root_manifest_path.to_path_buf(), "workspace.dependencies")
                })
        } else {
            extract_registry_spec(alias, value)
                .map(|(pkg_name, raw)| (pkg_name, raw, manifest_path.to_path_buf(), table_name))
        };
        let Some((package_name, raw_spec, target_manifest, target_table)) = resolved else {
            continue;
        };
        if out
            .iter()
            .any(|existing| {
                existing.manifest_path == target_manifest
                    && existing.table == target_table
                    && existing.alias == *alias
            })
        {
            continue;
        }
        let Ok(version_req) = VersionReq::parse(&raw_spec) else {
            continue;
        };
        out.push(DeclaredCargoDep {
            alias: alias.clone(),
            package_name,
            raw_spec,
            version_req,
            group,
            manifest_path: target_manifest,
            table: target_table,
        });
    }
}

fn is_workspace_inherited(value: &toml::Value) -> bool {
    value
        .as_table()
        .and_then(|table| table.get("workspace"))
        .and_then(toml::Value::as_bool)
        == Some(true)
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

pub(crate) fn format_cargo_saved_spec(
    existing_raw: &str,
    resolved: &Version,
    explicit_spec: Option<&str>,
    save_exact: bool,
    save_prefix: Option<&str>,
) -> Result<String> {
    if save_exact {
        return Ok(format!("={resolved}"));
    }
    if let Some(prefix) = save_prefix {
        if !matches!(prefix, "" | "^" | "~" | "=") {
            bail!("Cargo --save-prefix must be empty, ^, ~, or =");
        }
        return Ok(format!("{prefix}{resolved}"));
    }
    if let Some(spec) = explicit_spec
        && spec.starts_with(['^', '~', '=', '>', '<'])
    {
        return Ok(spec.to_string());
    }
    Ok(format_existing_cargo_spec(existing_raw, resolved))
}

fn format_existing_cargo_spec(existing_raw: &str, resolved: &Version) -> String {
    if !existing_raw.contains(',') {
        let prefix = existing_prefix(existing_raw);
        return format!("{prefix}{resolved}");
    }
    let matches_existing = VersionReq::parse(existing_raw).is_ok_and(|req| req.matches(resolved));
    if !matches_existing {
        return format!("^{resolved}");
    }
    let mut replaced_lower = false;
    let clauses: Vec<String> = existing_raw
        .split(',')
        .map(|clause| {
            let trimmed = clause.trim();
            if !replaced_lower && let Some(prefix) = lower_bound_prefix(trimmed) {
                replaced_lower = true;
                format!("{prefix}{resolved}")
            } else {
                trimmed.to_string()
            }
        })
        .collect();
    if replaced_lower { clauses.join(", ") } else { existing_raw.to_string() }
}

fn lower_bound_prefix(clause: &str) -> Option<&'static str> {
    if clause.starts_with(">=") {
        Some(">=")
    } else if clause.starts_with('^') {
        Some("^")
    } else if clause.starts_with('~') {
        Some("~")
    } else {
        None
    }
}

fn existing_prefix(raw: &str) -> &'static str {
    let trimmed = raw.trim();
    if trimmed.starts_with('^') {
        "^"
    } else if trimmed.starts_with('~') {
        "~"
    } else if trimmed.starts_with(">=") {
        ">="
    } else if trimmed.starts_with('=') {
        "="
    } else {
        ""
    }
}

pub(crate) fn apply_manifest_edits(
    edits: &[(PathBuf, &'static str, String, String)],
) -> Result<()> {
    let mut by_file: BTreeMap<&Path, Vec<(&'static str, &str, &str)>> = BTreeMap::new();
    for (path, table, alias, spec) in edits {
        by_file
            .entry(path.as_path())
            .or_default()
            .push((*table, alias.as_str(), spec.as_str()));
    }
    for (path, file_edits) in by_file {
        let mut contents = fs::read_to_string(path)
            .into_diagnostic()
            .wrap_err_with(|| format!("read {}", path.display()))?;
        for (table, alias, spec) in file_edits {
            contents = upsert_dependency(&contents, table, alias, spec)?;
        }
        pnpm_fs::write_atomic(path, contents.as_bytes()).into_diagnostic()?;
    }
    Ok(())
}
