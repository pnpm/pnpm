pub(crate) use membership::workspace_includes_member;

mod membership;

use miette::{IntoDiagnostic, Result, WrapErr, bail};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) struct WorkspaceCargoDep {
    pub(crate) workspace_root: PathBuf,
    pub(crate) package_name: String,
    pub(crate) path: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ConfiguredWorkspace {
    Unset,
    Path(PathBuf),
    Invalid,
}

/// Look up the `[workspace.dependencies]` entry inherited by `dep_key`,
/// checking `dep_key` first and falling back to the member's `package` rename.
pub(crate) fn lookup_inherited_workspace_entry<'a, Value, Lookup>(
    dep_key: &str,
    dep_package: Option<&str>,
    mut get: Lookup,
) -> Option<(&'a String, &'a Value)>
where
    Lookup: FnMut(&str) -> Option<(&'a String, &'a Value)>,
{
    get(dep_key).or_else(|| dep_package.and_then(get))
}

pub(crate) fn resolve_local_path(path: &Path) -> PathBuf {
    std::path::absolute(path)
        .map(|abs| pnpm_fs::lexical_normalize(&abs))
        .unwrap_or_else(|_| pnpm_fs::lexical_normalize(path))
}

pub(crate) fn rebase_workspace_path(
    workspace_root: &Path,
    member_dir: &Path,
    raw_path: &str,
) -> String {
    if Path::new(raw_path).is_absolute() {
        return raw_path.to_string();
    }
    let ws_root = resolve_local_path(workspace_root);
    let mem_dir = resolve_local_path(member_dir);
    let rel = pnpm_fs::relative_path(&mem_dir, &ws_root.join(raw_path));
    if rel.as_os_str().is_empty() {
        ".".to_string()
    } else {
        rel.to_string_lossy().replace('\\', "/")
    }
}

pub(crate) fn configured_workspace_state(member_dir: &Path) -> ConfiguredWorkspace {
    let Ok(contents) = fs::read_to_string(member_dir.join("Cargo.toml")) else {
        return ConfiguredWorkspace::Unset;
    };
    let Ok(doc) = toml::from_str::<toml::Table>(&contents) else {
        return ConfiguredWorkspace::Unset;
    };
    let Some(ws_val) = doc
        .get("package")
        .and_then(toml::Value::as_table)
        .and_then(|pkg| pkg.get("workspace"))
    else {
        return ConfiguredWorkspace::Unset;
    };
    match ws_val.as_str() {
        Some(rel) => ConfiguredWorkspace::Path(resolve_local_path(&member_dir.join(rel))),
        None => ConfiguredWorkspace::Invalid,
    }
}

pub(crate) fn workspace_dependency_spec(
    member_dir: &Path,
    dep_key: &str,
    dep_package: Option<&str>,
) -> Option<WorkspaceCargoDep> {
    let (workspace_root, ws_key, dep_val) =
        find_workspace_dep_entry(member_dir, dep_key, dep_package)?;
    parse_workspace_dep_value(&workspace_root, &ws_key, &dep_val)
}

pub(crate) fn resolve_workspace_dep_for_detach(
    member_dir: &Path,
    configured_ws: Option<PathBuf>,
    dep_key: &str,
    dep_package: Option<&str>,
) -> Result<(PathBuf, String, toml::Value)> {
    let norm_member = resolve_local_path(member_dir);
    if let Some(ws_dir) = configured_ws {
        return resolve_from_configured_workspace(&ws_dir, &norm_member, dep_key, dep_package);
    }
    for candidate in norm_member.ancestors() {
        let manifest_path = candidate.join("Cargo.toml");
        if !manifest_path.is_file() {
            continue;
        }
        let doc = read_manifest_table(&manifest_path, dep_key)?;
        let Some(ws) = doc.get("workspace").and_then(toml::Value::as_table) else {
            continue;
        };
        if candidate != norm_member && !workspace_includes_member(candidate, &norm_member) {
            continue;
        }
        let (ws_key, val) = extract_workspace_dep_entry(ws, &manifest_path, dep_key, dep_package)?;
        return Ok((candidate.to_path_buf(), ws_key, val));
    }
    let member_display = norm_member.display();
    Err(miette::miette!(
        "cannot detach crate {dep_key}: {member_display} does not belong to a Cargo workspace",
    ))
}

fn resolve_from_configured_workspace(
    ws_dir: &Path,
    norm_member: &Path,
    dep_key: &str,
    dep_package: Option<&str>,
) -> Result<(PathBuf, String, toml::Value)> {
    let manifest_path = ws_dir.join("Cargo.toml");
    let doc = read_manifest_table(&manifest_path, dep_key)?;
    let Some(ws) = doc.get("workspace").and_then(toml::Value::as_table) else {
        let manifest_display = manifest_path.display();
        bail!("cannot detach crate {dep_key}: {manifest_display} does not declare `[workspace]`");
    };
    if ws_dir != norm_member && !workspace_includes_member(ws_dir, norm_member) {
        let ws_display = ws_dir.display();
        let member_display = norm_member.display();
        bail!(
            "cannot detach crate {dep_key}: workspace at {ws_display} does not include {member_display}",
        );
    }
    let (ws_key, val) = extract_workspace_dep_entry(ws, &manifest_path, dep_key, dep_package)?;
    Ok((ws_dir.to_path_buf(), ws_key, val))
}

fn read_manifest_table(manifest_path: &Path, dep_key: &str) -> Result<toml::Table> {
    let manifest_display = manifest_path.display();
    let contents = fs::read_to_string(manifest_path)
        .into_diagnostic()
        .wrap_err_with(|| format!("cannot detach crate {dep_key}: read {manifest_display}"))?;
    toml::from_str::<toml::Table>(&contents)
        .into_diagnostic()
        .wrap_err_with(|| format!("cannot detach crate {dep_key}: parse {manifest_display}"))
}

fn extract_workspace_dep_entry(
    ws: &toml::Table,
    manifest_path: &Path,
    dep_key: &str,
    dep_package: Option<&str>,
) -> Result<(String, toml::Value)> {
    let manifest_display = manifest_path.display();
    let Some(deps) = ws.get("dependencies").and_then(toml::Value::as_table) else {
        bail!(
            "cannot detach crate {dep_key}: {manifest_display} declares no `[workspace.dependencies]`",
        );
    };
    let (ws_key, dep_val) =
        lookup_inherited_workspace_entry(dep_key, dep_package, |key| deps.get_key_value(key))
            .ok_or_else(|| {
                miette::miette!(
                    "cannot detach crate {dep_key}: not found in `[workspace.dependencies]` of {manifest_display}",
                )
            })?;
    Ok((ws_key.clone(), dep_val.clone()))
}

fn find_workspace_dep_entry(
    member_dir: &Path,
    dep_key: &str,
    dep_package: Option<&str>,
) -> Option<(PathBuf, String, toml::Value)> {
    let norm_member = resolve_local_path(member_dir);
    match configured_workspace_state(&norm_member) {
        ConfiguredWorkspace::Path(ws_dir) => {
            read_workspace_dep_from_candidate(&ws_dir, &norm_member, dep_key, dep_package)
        }
        ConfiguredWorkspace::Invalid => None,
        ConfiguredWorkspace::Unset => norm_member
            .ancestors()
            .find_map(|candidate| {
                read_workspace_dep_from_candidate(candidate, &norm_member, dep_key, dep_package)
            }),
    }
}

fn read_workspace_dep_from_candidate(
    workspace_dir: &Path,
    member_dir: &Path,
    dep_key: &str,
    dep_package: Option<&str>,
) -> Option<(PathBuf, String, toml::Value)> {
    let contents = fs::read_to_string(workspace_dir.join("Cargo.toml")).ok()?;
    let doc = toml::from_str::<toml::Table>(&contents).ok()?;
    let ws = doc.get("workspace")?.as_table()?;
    if workspace_dir != member_dir && !workspace_includes_member(workspace_dir, member_dir) {
        return None;
    }
    let deps = ws.get("dependencies")?.as_table()?;
    let (ws_key, dep_val) =
        lookup_inherited_workspace_entry(dep_key, dep_package, |key| deps.get_key_value(key))?;
    Some((workspace_dir.to_path_buf(), ws_key.clone(), dep_val.clone()))
}

fn parse_workspace_dep_value(
    workspace_dir: &Path,
    ws_key: &str,
    dep_val: &toml::Value,
) -> Option<WorkspaceCargoDep> {
    if dep_val.is_str() {
        return Some(WorkspaceCargoDep {
            workspace_root: workspace_dir.to_path_buf(),
            package_name: ws_key.to_string(),
            path: None,
        });
    }
    let table = dep_val.as_table()?;
    let package_name = table
        .get("package")
        .and_then(toml::Value::as_str)
        .unwrap_or(ws_key)
        .to_string();
    let path = table
        .get("path")
        .and_then(toml::Value::as_str)
        .map(str::to_string);
    Some(WorkspaceCargoDep { workspace_root: workspace_dir.to_path_buf(), package_name, path })
}

#[cfg(test)]
mod tests;
