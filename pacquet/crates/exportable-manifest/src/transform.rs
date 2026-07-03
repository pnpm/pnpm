//! Final normalization pass applied to a publish manifest. The four
//! steps run in order: required-field validation, `bin` normalization,
//! `peerDependenciesMeta` defaulting, then `repository` normalization.
//!
//! The pass is a straight sequence of in-place mutations on the
//! manifest object — no closure composition.

use derive_more::{Display, Error};
use miette::Diagnostic;
use serde_json::{Map, Value};

/// Failures raised while transforming a publish manifest. Both carry
/// the pnpm error codes published for these conditions.
#[derive(Debug, Display, Error, Diagnostic, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransformError {
    /// A `name` / `version` field required by the registry is missing.
    #[display("Missing required field \"{field}\"")]
    #[diagnostic(code(ERR_PNPM_MISSING_REQUIRED_FIELD))]
    MissingRequiredField { field: &'static str },

    /// A string `bin` was declared on a package whose scoped name has
    /// no `/` segment to derive the command name from.
    #[display("The name \"{invalid_name}\" is not a valid scoped package name")]
    #[diagnostic(code(ERR_PNPM_INVALID_SCOPED_PACKAGE_NAME))]
    InvalidScopedPackageName { invalid_name: String },
}

/// Run the publish-manifest transform pipeline in place.
pub fn transform(manifest: &mut Map<String, Value>) -> Result<(), TransformError> {
    transform_required_fields(manifest)?;
    transform_bin(manifest)?;
    transform_peer_dependencies_meta(manifest);
    transform_repository(manifest);
    Ok(())
}

/// Reject a manifest missing the registry-required `name` / `version`.
/// A field counts as missing only when it is absent or JS-falsy
/// (`null`, `false`, `0`, or `""`), so any other present value —
/// including a non-string — passes.
fn transform_required_fields(manifest: &Map<String, Value>) -> Result<(), TransformError> {
    for field in ["name", "version"] {
        if !manifest.get(field).is_some_and(is_truthy) {
            return Err(TransformError::MissingRequiredField { field });
        }
    }
    Ok(())
}

/// Whether a JSON value is truthy under JavaScript's coercion rules, so
/// the publish-manifest transforms gate on the same falsy set a JS
/// `if (!value)` check would. Arrays and objects are always truthy,
/// even when empty.
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(boolean) => *boolean,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(string) => !string.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// Normalize a string `bin` into the object form `{ <command>: <path> }`.
/// A `bin` that is already an object (or absent) is left untouched.
fn transform_bin(manifest: &mut Map<String, Value>) -> Result<(), TransformError> {
    let Some(Value::String(bin)) = manifest.get("bin") else {
        return Ok(());
    };
    let bin = bin.clone();
    // `transformRequiredFields` already guaranteed a string `name`.
    let pkg_name = manifest.get("name").and_then(Value::as_str).unwrap_or_default();
    let command_name = normalize_bin_name(pkg_name)?;
    let mut bin_object = Map::new();
    bin_object.insert(command_name, Value::String(bin));
    manifest.insert("bin".to_string(), Value::Object(bin_object));
    Ok(())
}

/// Derive the command name a string `bin` maps to. For a scoped name
/// the scope is stripped (`@scope/foo` → `foo`); an unscoped name is
/// used verbatim. A scoped name without a `/` is invalid.
fn normalize_bin_name(name: &str) -> Result<String, TransformError> {
    if !name.starts_with('@') {
        return Ok(name.to_string());
    }
    match name.find('/') {
        Some(slash_index) => Ok(name[slash_index + 1..].to_string()),
        None => Err(TransformError::InvalidScopedPackageName { invalid_name: name.to_string() }),
    }
}

/// Default each `peerDependenciesMeta` entry's `optional` to `false`
/// when it is absent, so the published manifest always states the flag
/// explicitly.
fn transform_peer_dependencies_meta(manifest: &mut Map<String, Value>) {
    let Some(Value::Object(meta)) = manifest.get("peerDependenciesMeta") else {
        return;
    };
    let mut out = Map::with_capacity(meta.len());
    for (key, entry) in meta {
        let mut entry = match entry {
            Value::Object(map) => map.clone(),
            // A non-object entry can't carry `optional`; preserve it
            // as-is rather than fabricate a shape the user didn't write.
            other => {
                out.insert(key.clone(), other.clone());
                continue;
            }
        };
        let optional = entry.get("optional").and_then(Value::as_bool).unwrap_or(false);
        entry.insert("optional".to_string(), Value::Bool(optional));
        out.insert(key.clone(), Value::Object(entry));
    }
    manifest.insert("peerDependenciesMeta".to_string(), Value::Object(out));
}

/// Normalize a string `repository` into the object form
/// `{ type: "git", url: <string> }`. npm's `normalize-package-data`
/// performs the same conversion before publishing; some registries
/// reject a bare-string `repository`. See
/// [pnpm/pnpm#12099](https://github.com/pnpm/pnpm/issues/12099).
fn transform_repository(manifest: &mut Map<String, Value>) {
    let Some(Value::String(url)) = manifest.get("repository") else {
        return;
    };
    let mut repository = Map::new();
    repository.insert("type".to_string(), Value::String("git".to_string()));
    repository.insert("url".to_string(), Value::String(url.clone()));
    manifest.insert("repository".to_string(), Value::Object(repository));
}

#[cfg(test)]
mod tests;
