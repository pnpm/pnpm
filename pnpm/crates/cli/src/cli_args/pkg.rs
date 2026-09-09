use crate::cli_args::recursive::{
    AutoExcludeRoot, discover_workspace_projects, select_recursive_projects,
};
use clap::{Args, Subcommand};
use derive_more::{Display, Error};
use miette::{Context, Diagnostic};
use pnpm_config::{
    Config,
    property_path::{self, Segment, get_object_value_by_property_path, parse_property_path},
};
use pnpm_package_manifest::PackageManifest;
use serde_json::{Map, Value};
use std::path::Path;

const UNSAFE_KEYS: [&str; 3] = ["__proto__", "constructor", "prototype"];

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
                PkgSubcommand::Delete(args) => edit_project_manifest(project, |value| {
                    for key in &args.keys {
                        delete_object_value_by_property_path(value, key)?;
                    }
                    Ok(())
                })?,
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
    let value = manifest.value_mut();
    for key in keys {
        delete_object_value_by_property_path(value, key)?;
    }
    manifest.save().wrap_err("saving package.json")?;
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

fn check_unsafe_key_in_path(key: &str) -> Result<(), PkgError> {
    let segments = parse_property_path(key).map_err(PkgError::InvalidPropertyPath)?;
    for segment in &segments {
        if let Segment::Key(k) = segment
            && UNSAFE_KEYS.contains(&k.as_str())
        {
            return Err(PkgError::UnsafeKey { key: k.clone() });
        }
    }
    Ok(())
}

pub(crate) const MAX_ARRAY_INDEX: usize = 1 << 20;

fn validate_index(idx: f64) -> Result<usize, PkgError> {
    if idx.fract() != 0.0 || idx.is_sign_negative() || !idx.is_finite() {
        return Err(PkgError::SetPathError { path: idx.to_string() });
    }
    let index = idx as usize;
    if index > MAX_ARRAY_INDEX {
        return Err(PkgError::SetPathError { path: idx.to_string() });
    }
    Ok(index)
}

fn idx_to_string(idx: f64) -> String {
    if idx.fract() == 0.0 && idx.is_finite() { format!("{}", idx as i64) } else { idx.to_string() }
}

fn set_object_value_by_property_path(
    root: &mut Value,
    path: &str,
    value: Value,
) -> miette::Result<()> {
    if path.is_empty() {
        return Err(PkgError::EmptyPath.into());
    }
    check_unsafe_key_in_path(path)?;
    let segments = parse_property_path(path)
        .map_err(|err| miette::Report::new(PkgError::InvalidPropertyPath(err)))?;
    let Some((last, parents)) = segments.split_last() else {
        return Err(PkgError::EmptyPath.into());
    };
    let mut current = root;
    for (position, segment) in parents.iter().enumerate() {
        // A container is created — or replaced — to match what the next
        // segment addresses it as, so a path can be set into a document
        // that does not describe it yet.
        let needs_array = matches!(&segments[position + 1], Segment::Index(_));
        current = descend_for_set(current, segment, needs_array)?;
    }
    place_value(current, last, value)
}

/// Step into the container `segment` names, creating it when the
/// document has nothing there or something of the wrong shape.
fn descend_for_set<'a>(
    current: &'a mut Value,
    segment: &Segment,
    needs_array: bool,
) -> miette::Result<&'a mut Value> {
    let key = match segment {
        Segment::Key(key) => key.clone(),
        Segment::Index(idx) => {
            let index = validate_index(*idx)?;
            if current.is_array() {
                return Ok(descend_array(current, index, needs_array));
            }
            if !current.is_object() {
                // Neither container the index can address, so it is
                // replaced by one; the walk continues from there.
                *current = index_container(*idx, index, needs_array);
                return Ok(current);
            }
            idx_to_string(*idx)
        }
    };
    Ok(descend_object(current, &key, needs_array))
}

fn descend_array(current: &mut Value, index: usize, needs_array: bool) -> &mut Value {
    let arr = current.as_array_mut().expect("the caller checked the value is an array");
    if index >= arr.len() {
        arr.resize(index.saturating_add(1), Value::Null);
    }
    if !container_matches(&arr[index], needs_array) {
        arr[index] = empty_container(needs_array);
    }
    &mut arr[index]
}

fn descend_object<'a>(current: &'a mut Value, key: &str, needs_array: bool) -> &'a mut Value {
    if !current.is_object() {
        *current = Value::Object(Map::new());
    }
    let obj = current.as_object_mut().expect("current was just made an object");
    if !obj.get(key).is_some_and(|entry| container_matches(entry, needs_array)) {
        obj.insert(key.to_owned(), empty_container(needs_array));
    }
    obj.get_mut(key).expect("the entry was just inserted")
}

/// Write the value at the path's last segment.
fn place_value(current: &mut Value, last: &Segment, value: Value) -> miette::Result<()> {
    let key = match last {
        Segment::Key(key) => key.clone(),
        Segment::Index(idx) => {
            let index = validate_index(*idx)?;
            if !current.is_object() {
                place_at_index(current, index, value);
                return Ok(());
            }
            idx_to_string(*idx)
        }
    };
    if !current.is_object() {
        *current = Value::Object(Map::new());
    }
    current.as_object_mut().expect("current was just made an object").insert(key, value);
    Ok(())
}

/// Write into the array slot the index names, growing — or building —
/// the array to reach it.
fn place_at_index(current: &mut Value, index: usize, value: Value) {
    if !current.is_array() {
        *current = Value::Array(Vec::new());
    }
    let arr = current.as_array_mut().expect("current was just made an array");
    if index >= arr.len() {
        arr.resize(index.saturating_add(1), Value::Null);
    }
    arr[index] = value;
}

fn container_matches(value: &Value, needs_array: bool) -> bool {
    if needs_array { value.is_array() } else { value.is_object() }
}

fn empty_container(needs_array: bool) -> Value {
    if needs_array { Value::Array(Vec::new()) } else { Value::Object(Map::new()) }
}

/// The container an index segment builds when the document has
/// something else there: an array long enough to hold the index, or an
/// object keyed by the index's string form.
fn index_container(idx: f64, index: usize, needs_array: bool) -> Value {
    if needs_array {
        let mut arr = Vec::with_capacity(index.saturating_add(1));
        arr.resize(index.saturating_add(1), Value::Null);
        return Value::Array(arr);
    }
    let mut map = Map::new();
    map.insert(idx_to_string(idx), Value::Null);
    Value::Object(map)
}

fn delete_object_value_by_property_path(root: &mut Value, path: &str) -> miette::Result<bool> {
    let segments = parse_property_path(path)
        .map_err(|err| miette::Report::new(PkgError::InvalidPropertyPath(err)))?;
    let Some((last, parents)) = segments.split_last() else {
        return Ok(false);
    };
    check_unsafe_key_in_path(path)?;
    let mut current = root;
    for segment in parents {
        let Some(next) = descend_for_delete(current, segment)? else {
            return Ok(false);
        };
        current = next;
    }
    remove_value(current, last)
}

/// Step into the container `segment` names. `None` when the document
/// has nothing there — a path that does not exist deletes nothing.
fn descend_for_delete<'a>(
    current: &'a mut Value,
    segment: &Segment,
) -> miette::Result<Option<&'a mut Value>> {
    let key = match segment {
        Segment::Key(key) => key.clone(),
        Segment::Index(idx) => {
            let index = validate_index(*idx)?;
            if current.is_array() {
                let arr = current.as_array_mut().expect("the value is an array");
                return Ok(arr.get_mut(index));
            }
            if !current.is_object() {
                return Ok(None);
            }
            idx_to_string(*idx)
        }
    };
    let Some(obj) = current.as_object_mut() else { return Ok(None) };
    Ok(obj.get_mut(&key))
}

/// Remove what the path's last segment names, reporting whether
/// anything was there.
fn remove_value(current: &mut Value, last: &Segment) -> miette::Result<bool> {
    let key = match last {
        Segment::Key(key) => key.clone(),
        Segment::Index(idx) => {
            let index = validate_index(*idx)?;
            if current.is_array() {
                let arr = current.as_array_mut().expect("the value is an array");
                if index >= arr.len() {
                    return Ok(false);
                }
                arr.remove(index);
                return Ok(true);
            }
            if !current.is_object() {
                return Ok(false);
            }
            idx_to_string(*idx)
        }
    };
    let Some(obj) = current.as_object_mut() else { return Ok(false) };
    Ok(obj.remove(&key).is_some())
}

#[cfg(test)]
mod tests;
