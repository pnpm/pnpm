use clap::{Args, Subcommand};
use derive_more::{Display, Error};
use miette::{Context, Diagnostic};
use pacquet_config::property_path::{
    self, Segment, get_object_value_by_property_path, parse_property_path,
};
use pacquet_package_manifest::PackageManifest;
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
}

fn pkg_get(manifest_path: &Path, keys: &[String], json: bool) -> miette::Result<String> {
    let manifest =
        PackageManifest::from_path(manifest_path.to_path_buf()).wrap_err("reading package.json")?;
    let value = manifest.value();
    let result = get_output(value, keys, json)?;
    Ok(result)
}

fn get_output(manifest: &Value, keys: &[String], json: bool) -> miette::Result<String> {
    if keys.len() == 1 {
        let key = &keys[0];
        let segments = parse_property_path(key).map_err(PkgError::InvalidPropertyPath)?;
        match get_object_value_by_property_path(manifest, &segments) {
            None => Ok(String::new()),
            Some(found) => {
                if json {
                    serde_json::to_string_pretty(found).map_err(|e| miette::miette!("{e}"))
                } else {
                    match found {
                        Value::String(s) => Ok(s.clone()),
                        other => {
                            serde_json::to_string_pretty(other).map_err(|e| miette::miette!("{e}"))
                        }
                    }
                }
            }
        }
    } else {
        let selected = select_from_manifest(manifest, keys);
        serde_json::to_string_pretty(&selected).map_err(|e| miette::miette!("{e}"))
    }
}

fn select_from_manifest(manifest: &Value, keys: &[String]) -> Value {
    if keys.is_empty() {
        return manifest.clone();
    }
    let mut result = Map::new();
    for key in keys {
        let value = parse_property_path(key)
            .ok()
            .and_then(|segments| get_object_value_by_property_path(manifest, &segments))
            .map_or(Value::Null, std::borrow::ToOwned::to_owned);
        result.insert(key.clone(), value);
    }
    Value::Object(result)
}

fn pkg_set(manifest_path: &Path, pairs: &[String], json: bool) -> miette::Result<()> {
    if pairs.is_empty() {
        return Err(PkgError::SetMissingArgs.into());
    }
    let mut manifest =
        PackageManifest::from_path(manifest_path.to_path_buf()).wrap_err("reading package.json")?;
    let value = manifest.value_mut();
    for pair in pairs {
        let eq_index =
            pair.find('=').ok_or_else(|| PkgError::SetInvalidArg { arg: pair.clone() })?;
        let key = &pair[..eq_index];
        let raw_value = &pair[eq_index + 1..];
        let parsed_value: Value = if json {
            serde_json::from_str(raw_value)
                .map_err(|_| PkgError::SetJsonParse { value: raw_value.to_string() })?
        } else {
            Value::String(raw_value.to_string())
        };
        set_object_value_by_property_path(value, key, parsed_value)?;
    }
    manifest.save().wrap_err("saving package.json")?;
    Ok(())
}

fn pkg_delete(manifest_path: &Path, keys: &[String]) -> miette::Result<()> {
    if keys.is_empty() {
        return Err(PkgError::DeleteMissingArgs.into());
    }
    let mut manifest =
        PackageManifest::from_path(manifest_path.to_path_buf()).wrap_err("reading package.json")?;
    let value = manifest.value_mut();
    for key in keys {
        delete_object_value_by_property_path(value, key);
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
    if let Some(name) = obj.get("name")
        && !name.is_string()
    {
        obj.remove("name");
    }
    if let Some(version) = obj.get("version")
        && !version.is_string()
    {
        obj.remove("version");
    }
    for field in
        &["dependencies", "devDependencies", "optionalDependencies", "peerDependencies", "scripts"]
    {
        if let Some(val) = obj.get(*field)
            && !val.is_object()
        {
            obj.remove(*field);
        }
    }
    if let Some(bin) = obj.get("bin")
        && !bin.is_string()
        && !bin.is_object()
    {
        obj.remove("bin");
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

fn set_path_err(path: &str) -> miette::Report {
    miette::Report::new(PkgError::SetPathError { path: path.to_string() })
}

fn set_object_value_by_property_path(
    root: &mut Value,
    path: &str,
    value: Value,
) -> miette::Result<()> {
    check_unsafe_key_in_path(path)?;
    let segments = parse_property_path(path)
        .map_err(|err| miette::Report::new(PkgError::InvalidPropertyPath(err)))?;
    if segments.is_empty() {
        return Ok(());
    }
    let last_idx = segments.len() - 1;
    let mut current = root;
    for segment in &segments[..last_idx] {
        match segment {
            Segment::Key(k) => {
                let obj = current.as_object_mut().ok_or_else(|| set_path_err(path))?;
                current = obj.entry(k.clone()).or_insert_with(|| Value::Object(Map::new()));
            }
            Segment::Index(idx) => {
                let index = *idx as usize;
                let arr = current.as_array_mut().ok_or_else(|| set_path_err(path))?;
                if index >= arr.len() {
                    arr.resize(index + 1, Value::Null);
                }
                current = &mut arr[index];
            }
        }
    }
    match &segments[last_idx] {
        Segment::Key(k) => {
            let obj = current.as_object_mut().ok_or_else(|| set_path_err(path))?;
            obj.insert(k.clone(), value);
        }
        Segment::Index(idx) => {
            let index = *idx as usize;
            let arr = current.as_array_mut().ok_or_else(|| set_path_err(path))?;
            if index >= arr.len() {
                arr.resize(index + 1, Value::Null);
            }
            arr[index] = value;
        }
    }
    Ok(())
}

fn delete_object_value_by_property_path(root: &mut Value, path: &str) -> bool {
    let Ok(segments) = parse_property_path(path) else { return false };
    if segments.is_empty() {
        return false;
    }
    let last_idx = segments.len() - 1;
    let mut current = root;
    for segment in &segments[..last_idx] {
        match segment {
            Segment::Key(k) => {
                let Some(obj) = current.as_object_mut() else { return false };
                let Some(next) = obj.get_mut(k) else { return false };
                current = next;
            }
            Segment::Index(idx) => {
                let index = *idx as usize;
                let Some(arr) = current.as_array_mut() else { return false };
                let Some(next) = arr.get_mut(index) else { return false };
                current = next;
            }
        }
    }
    match &segments[last_idx] {
        Segment::Key(k) => {
            let Some(obj) = current.as_object_mut() else { return false };
            obj.remove(k).is_some()
        }
        Segment::Index(idx) => {
            let index = *idx as usize;
            let Some(arr) = current.as_array_mut() else { return false };
            if index < arr.len() {
                arr.remove(index);
                true
            } else {
                false
            }
        }
    }
}

#[cfg(test)]
mod tests;
