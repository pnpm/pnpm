use crate::cli_args::recursive::{
    AutoExcludeRoot, discover_workspace_projects, select_recursive_projects,
};
use clap::{Args, Subcommand};
use derive_more::{Display, Error};
use editing::{
    check_unsafe_key_in_path, delete_object_value_by_property_path,
    set_object_value_by_property_path,
};
use miette::{Context, Diagnostic};
use pnpm_config::{
    Config, property_path,
    property_path::{Segment, get_object_value_by_property_path, parse_property_path},
};
use pnpm_package_manifest::PackageManifest;
use serde_json::{Map, Value};
use std::path::Path;

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum PkgError {
    #[display("Missing key=value pairs")]
    #[diagnostic(code(ERR_PNPM_PKG_SET_MISSING_ARGS))]
    SetMissingArgs,

    #[display(r#"Invalid argument "{arg}". Expected key=value format"#)]
    #[diagnostic(code(ERR_PNPM_PKG_SET_INVALID_ARG))]
    SetInvalidArg { arg: String },

    #[display(r#"Failed to parse value as JSON: "{value}""#)]
    #[diagnostic(code(ERR_PNPM_PKG_SET_JSON_PARSE))]
    SetJsonParse { value: String },

    #[display("Missing keys to delete")]
    #[diagnostic(code(ERR_PNPM_PKG_DELETE_MISSING_ARGS))]
    DeleteMissingArgs,

    #[display(r#"Key "{key}" is not allowed in a property path"#)]
    #[diagnostic(code(ERR_PNPM_UNSAFE_PROPERTY_PATH_KEY))]
    UnsafeKey { key: String },

    #[display("Invalid property path: {_0}")]
    #[diagnostic(code(ERR_PNPM_PKG_INVALID_PROPERTY_PATH))]
    InvalidPropertyPath(#[error(not(source))] property_path::ParsePropertyPathError),

    #[display("Cannot set property on a non-object or non-array value at path")]
    #[diagnostic(code(ERR_PNPM_PKG_SET_PATH_ERROR))]
    SetPathError { path: String },

    #[display("Cannot use an empty property path")]
    #[diagnostic(code(ERR_PNPM_PKG_EMPTY_PROPERTY_PATH))]
    EmptyPath,

    #[display("Cannot run recursively outside of a workspace")]
    #[diagnostic(code(ERR_PNPM_PKG_RECURSIVE_NO_ROOT))]
    RecursiveNoRoot,

    #[display("No workspace packages were selected")]
    #[diagnostic(code(ERR_PNPM_PKG_RECURSIVE_NO_PACKAGES))]
    RecursiveNoPackages,
}

#[derive(Debug, Args)]
pub struct PkgArgs {
    #[clap(subcommand)]
    pub command: PkgSubcommand,

    /// When setting, parse the value as JSON. When getting a single key,
    /// return its JSON-encoded form instead of the raw value.
    #[clap(long, global = true)]
    pub json: bool,
}

#[derive(Debug, Subcommand)]
pub enum PkgSubcommand {
    /// Retrieves a value from package.json.
    Get(PkgGetArgs),
    /// Sets a value in package.json.
    Set(PkgSetArgs),
    /// Deletes a key from package.json.
    Delete(PkgDeleteArgs),
    /// Auto corrects common errors in package.json.
    Fix,
}

#[derive(Debug, Args)]
pub struct PkgGetArgs {
    /// Keys to retrieve from package.json.
    pub keys: Vec<String>,
}

#[derive(Debug, Args)]
pub struct PkgSetArgs {
    /// key=value pairs to set in package.json.
    #[clap(required = true)]
    pub pairs: Vec<String>,
}

#[derive(Debug, Args)]
pub struct PkgDeleteArgs {
    /// Keys to delete from package.json.
    #[clap(required = true)]
    pub keys: Vec<String>,
}

impl PkgArgs {
    pub fn run(self, manifest_path: &Path) -> miette::Result<()> {
        match self.command {
            PkgSubcommand::Get(args) => {
                let output = pkg_get(manifest_path, &args.keys, self.json)?;
                if !output.is_empty() {
                    println!("{output}");
                }
            }
            PkgSubcommand::Set(args) => {
                pkg_set(manifest_path, &args.pairs, self.json)?;
            }
            PkgSubcommand::Delete(args) => {
                pkg_delete(manifest_path, &args.keys)?;
            }
            PkgSubcommand::Fix => {
                pkg_fix(manifest_path)?;
            }
        }
        Ok(())
    }

    /// Run the pkg command across `--filter`-selected workspace projects.
    pub fn run_recursive(self, config: &Config, dir: &Path) -> miette::Result<()> {
        let workspace_root = config.workspace_dir.as_deref().unwrap_or(dir);
        if config.workspace_dir.is_none() {
            return Err(PkgError::RecursiveNoRoot.into());
        }
        let (projects, _patterns) = discover_workspace_projects(workspace_root, config)
            .wrap_err("discover workspace projects")?;
        let selection =
            select_recursive_projects(&projects, config, dir, AutoExcludeRoot::Disabled)?;
        if selection.selected.is_empty() {
            return Err(PkgError::RecursiveNoPackages.into());
        }
        let projects = selection.selected.values().map(|node| node.package.project);
        let PkgSubcommand::Get(args) = &self.command else {
            return self.edit_recursive(projects);
        };
        print_recursive_get(projects, &args.keys, workspace_root)
    }

    /// Apply the manifest-editing subcommands to every selected project.
    fn edit_recursive<'a>(
        &self,
        projects: impl Iterator<Item = &'a pnpm_workspace::Project>,
    ) -> miette::Result<()> {
        for project in projects {
            match &self.command {
                PkgSubcommand::Set(args) => edit_project_manifest(project, |value| {
                    apply_set_pairs(value, &args.pairs, self.json)
                })?,
                PkgSubcommand::Delete(args) => {
                    edit_project_manifest(project, |value| apply_delete_keys(value, &args.keys))?;
                }
                PkgSubcommand::Fix => edit_project_manifest(project, |value| {
                    fix_manifest(value);
                    Ok(())
                })?,
                PkgSubcommand::Get(_) => unreachable!("the get subcommand prints instead"),
            }
        }
        Ok(())
    }
}

fn pkg_get(manifest_path: &Path, keys: &[String], json: bool) -> miette::Result<String> {
    let manifest =
        PackageManifest::from_path(manifest_path.to_path_buf()).wrap_err("reading package.json")?;
    let value = manifest.value();
    let result = get_output(value, keys, json)?;
    Ok(result)
}

fn get_output(manifest: &Value, keys: &[String], json: bool) -> miette::Result<String> {
    let [key] = keys else {
        let selected = select_from_manifest(manifest, keys)?;
        return serde_json::to_string_pretty(&selected).map_err(|error| miette::miette!("{error}"));
    };
    if key.is_empty() {
        return Err(PkgError::EmptyPath.into());
    }
    let segments = parse_property_path(key).map_err(PkgError::InvalidPropertyPath)?;
    if segments.is_empty() {
        return Err(PkgError::EmptyPath.into());
    }
    let Some(found) = get_object_value_by_property_path(manifest, &segments) else {
        return Ok(String::new());
    };
    // A single string value prints bare, so `pnpm pkg get name` reads
    // as the name rather than as a quoted JSON string.
    match found {
        Value::String(text) if !json => Ok(text.clone()),
        found => serde_json::to_string_pretty(found).map_err(|error| miette::miette!("{error}")),
    }
}

/// Print the selected keys of every project, keyed by project name.
fn print_recursive_get<'a>(
    projects: impl Iterator<Item = &'a pnpm_workspace::Project>,
    keys: &[String],
    workspace_root: &Path,
) -> miette::Result<()> {
    let mut entries = Map::new();
    for project in projects {
        let name = project_report_name(project, workspace_root);
        entries.insert(name, select_from_manifest(project.manifest.value(), keys)?);
    }
    let output = serde_json::to_string_pretty(&Value::Object(entries))
        .map_err(|error| miette::miette!("{error}"))?;
    println!("{output}");
    Ok(())
}

/// How the recursive report names one project: its manifest name, or
/// its workspace-relative directory when it declares none.
fn project_report_name(project: &pnpm_workspace::Project, workspace_root: &Path) -> String {
    project.manifest.value().get("name").and_then(Value::as_str).map_or_else(
        || {
            project
                .root_dir
                .strip_prefix(workspace_root)
                .unwrap_or(&project.root_dir)
                .display()
                .to_string()
        },
        String::from,
    )
}

/// Read one project's manifest, apply `edit`, and write it back.
fn edit_project_manifest(
    project: &pnpm_workspace::Project,
    edit: impl FnOnce(&mut Value) -> miette::Result<()>,
) -> miette::Result<()> {
    let mut manifest = PackageManifest::from_path(project.root_dir.join("package.json"))
        .wrap_err("reading package.json")?;
    edit(manifest.value_mut())?;
    manifest.save().wrap_err("saving package.json")
}

/// Apply the `key=value` pairs of a `pnpm pkg set`.
fn apply_set_pairs(value: &mut Value, pairs: &[String], json: bool) -> miette::Result<()> {
    for pair in pairs {
        let (key, raw_value) =
            pair.split_once('=').ok_or_else(|| PkgError::SetInvalidArg { arg: pair.clone() })?;
        let parsed_value: Value = if json {
            serde_json::from_str(raw_value)
                .map_err(|_| PkgError::SetJsonParse { value: raw_value.to_string() })?
        } else {
            Value::String(raw_value.to_string())
        };
        set_object_value_by_property_path(value, key, parsed_value)?;
    }
    Ok(())
}

fn select_from_manifest(manifest: &Value, keys: &[String]) -> miette::Result<Value> {
    if keys.is_empty() {
        return Ok(manifest.clone());
    }
    let mut result = Map::new();
    for key in keys {
        let segments = parse_property_path(key).map_err(PkgError::InvalidPropertyPath)?;
        if let Some(found) = get_object_value_by_property_path(manifest, &segments) {
            result.insert(key.clone(), found.clone());
        }
    }
    Ok(Value::Object(result))
}

fn pkg_set(manifest_path: &Path, pairs: &[String], json: bool) -> miette::Result<()> {
    if pairs.is_empty() {
        return Err(PkgError::SetMissingArgs.into());
    }
    let mut manifest =
        PackageManifest::from_path(manifest_path.to_path_buf()).wrap_err("reading package.json")?;
    apply_set_pairs(manifest.value_mut(), pairs, json)?;
    manifest.save().wrap_err("saving package.json")?;
    Ok(())
}

fn pkg_delete(manifest_path: &Path, keys: &[String]) -> miette::Result<()> {
    if keys.is_empty() {
        return Err(PkgError::DeleteMissingArgs.into());
    }
    for key in keys {
        check_unsafe_key_in_path(key)?;
    }
    let mut manifest =
        PackageManifest::from_path(manifest_path.to_path_buf()).wrap_err("reading package.json")?;
    apply_delete_keys(manifest.value_mut(), keys)?;
    manifest.save().wrap_err("saving package.json")?;
    Ok(())
}

fn apply_delete_keys(value: &mut Value, keys: &[String]) -> miette::Result<()> {
    for key in keys {
        delete_object_value_by_property_path(value, key)?;
    }
    Ok(())
}

fn pkg_fix(manifest_path: &Path) -> miette::Result<()> {
    let mut manifest =
        PackageManifest::from_path(manifest_path.to_path_buf()).wrap_err("reading package.json")?;
    let value = manifest.value_mut();
    fix_manifest(value);
    manifest.save().wrap_err("saving package.json")?;
    Ok(())
}

fn fix_manifest(value: &mut Value) {
    let Some(obj) = value.as_object_mut() else { return };
    remove_ill_typed_field(obj, "name", Value::is_string);
    remove_ill_typed_field(obj, "version", Value::is_string);
    for field in
        ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies", "scripts"]
    {
        remove_ill_typed_field(obj, field, Value::is_object);
    }
    remove_ill_typed_field(obj, "bin", |bin| bin.is_string() || bin.is_object());
}

/// Drop a manifest field whose value is not of a shape pnpm can read.
fn remove_ill_typed_field(
    obj: &mut Map<String, Value>,
    field: &str,
    well_typed: impl Fn(&Value) -> bool,
) {
    if obj.get(field).is_some_and(|value| !well_typed(value)) {
        obj.remove(field);
    }
}

#[cfg(test)]
mod tests;

mod editing;
