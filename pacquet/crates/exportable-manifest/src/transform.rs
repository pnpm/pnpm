//! Final normalization pass applied to a publish manifest, mirroring
//! upstream's
//! [`transform`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/releasing/exportable-manifest/src/transform/index.ts)
//! pipeline. The four steps run in the same order pnpm's `ramda.pipe`
//! composes them: required-field validation, `bin` normalization,
//! `peerDependenciesMeta` defaulting, then `repository` normalization.
//!
//! The TypeScript pipeline is a `pipe(...)` of single-argument
//! transforms; the Rust port is a straight sequence of in-place
//! mutations on the manifest object — no closure composition.

use derive_more::{Display, Error};
use miette::Diagnostic;
use serde_json::{Map, Value};

/// Failures raised while transforming a publish manifest. Both map
/// byte-for-byte to the upstream `PnpmError` codes.
#[derive(Debug, Display, Error, Diagnostic, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransformError {
    /// A `name` / `version` field required by the registry is missing.
    /// Mirrors upstream's `MISSING_REQUIRED_FIELD` at
    /// [`transform/requiredFields.ts`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/releasing/exportable-manifest/src/transform/requiredFields.ts#L11-L14).
    #[display("Missing required field \"{field}\"")]
    #[diagnostic(code(ERR_PNPM_MISSING_REQUIRED_FIELD))]
    MissingRequiredField { field: &'static str },

    /// A string `bin` was declared on a package whose scoped name has
    /// no `/` segment to derive the command name from. Mirrors
    /// upstream's `INVALID_SCOPED_PACKAGE_NAME` at
    /// [`transform/bin.ts`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/releasing/exportable-manifest/src/transform/bin.ts#L37-L42).
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
/// A field present but not a non-empty string counts as missing, matching
/// the JS `if (!manifest.name)` truthiness check.
fn transform_required_fields(manifest: &Map<String, Value>) -> Result<(), TransformError> {
    for field in ["name", "version"] {
        let present =
            manifest.get(field).and_then(Value::as_str).is_some_and(|value| !value.is_empty());
        if !present {
            return Err(TransformError::MissingRequiredField { field });
        }
    }
    Ok(())
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
mod tests {
    use super::{TransformError, transform};
    use serde_json::{Value, json};

    fn run(mut manifest: Value) -> Result<Value, TransformError> {
        let object = manifest.as_object_mut().unwrap();
        transform(object)?;
        Ok(manifest)
    }

    #[test]
    fn missing_name_is_rejected() {
        let err = run(json!({ "version": "1.0.0" })).unwrap_err();
        assert_eq!(err, TransformError::MissingRequiredField { field: "name" });
    }

    #[test]
    fn missing_version_is_rejected() {
        let err = run(json!({ "name": "foo" })).unwrap_err();
        assert_eq!(err, TransformError::MissingRequiredField { field: "version" });
    }

    #[test]
    fn string_bin_becomes_object_under_unscoped_name() {
        let out = run(json!({ "name": "foo", "version": "1.0.0", "bin": "cli.js" })).unwrap();
        assert_eq!(out["bin"], json!({ "foo": "cli.js" }));
    }

    #[test]
    fn string_bin_strips_scope_for_command_name() {
        let out =
            run(json!({ "name": "@scope/foo", "version": "1.0.0", "bin": "cli.js" })).unwrap();
        assert_eq!(out["bin"], json!({ "foo": "cli.js" }));
    }

    #[test]
    fn object_bin_is_left_untouched() {
        let out = run(json!({
            "name": "foo",
            "version": "1.0.0",
            "bin": { "a": "a.js", "b": "b.js" },
        }))
        .unwrap();
        assert_eq!(out["bin"], json!({ "a": "a.js", "b": "b.js" }));
    }

    #[test]
    fn scoped_name_without_slash_and_string_bin_is_rejected() {
        let err = run(json!({ "name": "@foo", "version": "1.0.0", "bin": "cli.js" })).unwrap_err();
        assert_eq!(
            err,
            TransformError::InvalidScopedPackageName { invalid_name: "@foo".to_string() }
        );
    }

    #[test]
    fn peer_dependencies_meta_gets_explicit_optional() {
        let out = run(json!({
            "name": "foo",
            "version": "1.0.0",
            "peerDependenciesMeta": { "react": {}, "vue": { "optional": true } },
        }))
        .unwrap();
        assert_eq!(out["peerDependenciesMeta"]["react"], json!({ "optional": false }));
        assert_eq!(out["peerDependenciesMeta"]["vue"], json!({ "optional": true }));
    }

    #[test]
    fn string_repository_becomes_git_object() {
        let out = run(json!({
            "name": "foo",
            "version": "1.0.0",
            "repository": "https://github.com/foo/bar",
        }))
        .unwrap();
        assert_eq!(
            out["repository"],
            json!({ "type": "git", "url": "https://github.com/foo/bar" })
        );
    }

    #[test]
    fn object_repository_is_left_untouched() {
        let repo = json!({ "type": "git", "url": "git+ssh://x" });
        let out = run(json!({
            "name": "foo",
            "version": "1.0.0",
            "repository": repo,
        }))
        .unwrap();
        assert_eq!(out["repository"], json!({ "type": "git", "url": "git+ssh://x" }));
    }
}
