use super::{Map, PackageManifestError, Range, Value, json};

/// Runtime aliases recognised by `devEngines.runtime` /
/// `engines.runtime` reification.
const RUNTIME_NAMES: [&str; 3] = ["node", "deno", "bun"];

/// Whether `alias` names a runtime pnpm can download and manage
/// (`node` / `deno` / `bun`).
#[must_use]
pub fn is_runtime_alias(alias: &str) -> bool {
    RUNTIME_NAMES.contains(&alias)
}

/// Reify `devEngines.runtime` / `engines.runtime` entries with
/// `onFail: "download"` into the matching `devDependencies` /
/// `dependencies` slot as `runtime:<version>` specifiers.
///
/// This makes the lockfile entry the resolver writes
/// (`node@runtime:24.6.0`, etc.) visible to the
/// `satisfies_package_manifest` flat-record diff under the manifest's
/// own dependency map. Without this step a manifest that declares its
/// runtime exclusively through `devEngines.runtime` fails the frozen-
/// lockfile staleness check as a spurious "dependency was removed".
///
/// The `WebContainer` "no runtime download" branch is intentionally
/// omitted: pacquet does not run in `WebContainer`.
pub fn convert_engines_runtime_to_dependencies(
    manifest: &mut Value,
    engines_field: &str,
    deps_field: &str,
) {
    let to_insert = engines_runtime_dependencies(manifest, engines_field, deps_field);
    if to_insert.is_empty() {
        return;
    }
    let Some(manifest_obj) = manifest.as_object_mut() else {
        return;
    };
    let deps =
        manifest_obj.entry(deps_field.to_string()).or_insert_with(|| Value::Object(Map::new()));
    let Some(deps_obj) = deps.as_object_mut() else {
        return;
    };
    for (name, spec) in to_insert {
        deps_obj.insert(name.to_string(), Value::String(spec));
    }
}

/// Return runtime dependency edges synthesized from an engines field.
#[must_use]
pub fn engines_runtime_dependencies(
    manifest: &Value,
    engines_field: &str,
    deps_field: &str,
) -> Vec<(&'static str, String)> {
    let mut dependencies = Vec::new();
    let Some(runtime_entry) =
        manifest.get(engines_field).and_then(|engines| engines.get("runtime"))
    else {
        return dependencies;
    };
    for runtime_name in RUNTIME_NAMES {
        if manifest.get(deps_field).and_then(|deps| deps.get(runtime_name)).is_some() {
            continue;
        }
        let runtimes: &[Value] = match runtime_entry {
            Value::Array(arr) => arr.as_slice(),
            single @ Value::Object(_) => std::slice::from_ref(single),
            _ => continue,
        };
        let Some(runtime) = runtimes
            .iter()
            .find(|runtime| runtime.get("name").and_then(Value::as_str) == Some(runtime_name))
        else {
            continue;
        };
        if runtime.get("onFail").and_then(Value::as_str) != Some("download") {
            continue;
        }
        let Some(version) = runtime.get("version").and_then(Value::as_str) else {
            continue;
        };
        dependencies.push((runtime_name, format!("runtime:{}", version.trim())));
    }
    dependencies
}

/// Apply the configured runtime failure policy to both engine fields.
///
/// A non-download policy removes `runtime:` dependency entries only for names
/// managed by the corresponding engines field. `download` re-runs the normal
/// engine-to-dependency conversion.
pub fn apply_runtime_on_fail_override(manifest: &mut Value, on_fail_override: &str) {
    for (engines_field, deps_field) in
        [("devEngines", "devDependencies"), ("engines", "dependencies")]
    {
        let Some(runtime_entry) =
            manifest.get_mut(engines_field).and_then(|engines| engines.get_mut("runtime"))
        else {
            continue;
        };
        let managed_runtime_names = managed_runtimes(runtime_entry);
        if !set_runtime_on_fail(runtime_entry, on_fail_override) {
            continue;
        }
        if on_fail_override == "download" {
            convert_engines_runtime_to_dependencies(manifest, engines_field, deps_field);
            continue;
        }
        drop_runtime_dependencies(manifest, deps_field, &managed_runtime_names);
    }
}

/// Stamp the policy onto every runtime the entry declares. Reports `false`
/// when the entry is neither a runtime nor a list of them.
fn set_runtime_on_fail(runtime_entry: &mut Value, on_fail_override: &str) -> bool {
    match runtime_entry {
        Value::Array(runtimes) => {
            for runtime in runtimes {
                if let Some(runtime) = runtime.as_object_mut() {
                    runtime
                        .insert("onFail".to_string(), Value::String(on_fail_override.to_string()));
                }
            }
            true
        }
        Value::Object(runtime) => {
            runtime.insert("onFail".to_string(), Value::String(on_fail_override.to_string()));
            true
        }
        _ => false,
    }
}

/// The runtimes an `engines.runtime` entry names, in the order pnpm knows them.
fn managed_runtimes(runtime_entry: &Value) -> Vec<&'static str> {
    let names =
        |runtime: &Value, wanted: &str| runtime.get("name").and_then(Value::as_str) == Some(wanted);
    RUNTIME_NAMES
        .into_iter()
        .filter(|runtime_name| match runtime_entry {
            Value::Array(runtimes) => runtimes.iter().any(|runtime| names(runtime, runtime_name)),
            Value::Object(_) => names(runtime_entry, runtime_name),
            _ => false,
        })
        .collect()
}

/// Drop the `runtime:` dependency entries pnpm itself wrote for the runtimes
/// the engines field manages. A hand-written entry under the same name is
/// left alone.
fn drop_runtime_dependencies(manifest: &mut Value, deps_field: &str, managed: &[&str]) {
    let Some(deps) = manifest.get_mut(deps_field).and_then(Value::as_object_mut) else {
        return;
    };
    for runtime_name in managed {
        let written_by_pnpm = deps
            .get(*runtime_name)
            .and_then(Value::as_str)
            .is_some_and(|specifier| specifier.starts_with("runtime:"));
        if written_by_pnpm {
            deps.remove(*runtime_name);
        }
    }
}

/// Return the minimum Node.js version declared by `devEngines.runtime` or
/// `engines.runtime`, in that precedence order.
#[must_use]
pub fn node_version_from_engines_runtime(manifest: &Value) -> Option<String> {
    for engines_field in ["devEngines", "engines"] {
        let Some(runtime_entry) =
            manifest.get(engines_field).and_then(|value| value.get("runtime"))
        else {
            continue;
        };
        let runtimes = match runtime_entry {
            Value::Array(runtimes) => runtimes.as_slice(),
            runtime @ Value::Object(_) => std::slice::from_ref(runtime),
            _ => continue,
        };
        let Some(version) = runtimes.iter().find_map(|runtime| {
            (runtime.get("name").and_then(Value::as_str) == Some("node"))
                .then(|| runtime.get("version").and_then(Value::as_str))
                .flatten()
        }) else {
            continue;
        };
        if let Ok(range) = Range::parse(version.trim())
            && let Some(version) = range.min_version()
        {
            return Some(version.to_string());
        }
    }
    None
}

/// Fold `runtime:<version>` dependency entries back into
/// `devEngines.runtime` / `engines.runtime` before writing a manifest.
///
/// The in-memory dependency form drives resolution and lockfile checks,
/// while the on-disk manifest keeps the `devEngines.runtime` /
/// `engines.runtime` contract.
///
/// Mutates `manifest` in place and removes consumed `runtime:` dependency
/// entries. Returns `InvalidAttribute` when a field shape prevents a
/// lossless write.
pub fn convert_dependencies_to_engines_runtime(
    manifest: &mut Value,
    deps_field: &str,
    engines_field: &str,
) -> Result<(), PackageManifestError> {
    if manifest.get(deps_field).is_some_and(|deps| !deps.is_object()) {
        return Err(PackageManifestError::InvalidAttribute(format!(
            "the {deps_field} field must be an object",
        )));
    }
    for runtime_name in RUNTIME_NAMES {
        let version = manifest
            .get(deps_field)
            .and_then(Value::as_object)
            .and_then(|deps| deps.get(runtime_name))
            .and_then(Value::as_str)
            .and_then(|dep| dep.strip_prefix("runtime:"))
            .map(str::trim)
            .map(str::to_string);
        if let Some(version) = version {
            upsert_runtime_entry(manifest, engines_field, runtime_name, &version)?;
            if let Some(deps) = manifest.get_mut(deps_field).and_then(Value::as_object_mut) {
                deps.remove(runtime_name);
            }
        } else {
            remove_managed_runtime_entry(manifest, engines_field, runtime_name);
        }
    }
    Ok(())
}

fn remove_managed_runtime_entry(manifest: &mut Value, engines_field: &str, runtime_name: &str) {
    let Some(engines) = manifest.get_mut(engines_field).and_then(Value::as_object_mut) else {
        return;
    };
    let remove_runtime = match engines.get_mut("runtime") {
        Some(Value::Array(runtimes)) => {
            runtimes.retain(|runtime| !is_managed_runtime_entry(runtime, runtime_name));
            runtimes.is_empty()
        }
        Some(runtime) if is_managed_runtime_entry(runtime, runtime_name) => true,
        _ => false,
    };
    if remove_runtime {
        engines.remove("runtime");
    }
}

fn is_managed_runtime_entry(runtime: &Value, runtime_name: &str) -> bool {
    runtime.get("name").and_then(Value::as_str) == Some(runtime_name)
        && runtime.get("onFail").and_then(Value::as_str) == Some("download")
        && runtime.get("version").and_then(Value::as_str).is_some()
}

fn upsert_runtime_entry(
    manifest: &mut Value,
    engines_field: &str,
    runtime_name: &str,
    version: &str,
) -> Result<(), PackageManifestError> {
    let runtime_entry = json!({
        "name": runtime_name,
        "version": version,
        "onFail": "download",
    });
    let engines = ensure_object_field(manifest, engines_field)?;
    match engines.get_mut("runtime") {
        None | Some(Value::Null) => {
            engines.insert("runtime".to_string(), runtime_entry);
        }
        Some(Value::Array(runtimes)) => {
            if let Some(existing) = runtimes
                .iter_mut()
                .find(|runtime| runtime.get("name").and_then(Value::as_str) == Some(runtime_name))
            {
                merge_runtime_entry(existing, runtime_name, version)?;
            } else {
                runtimes.push(runtime_entry);
            }
        }
        Some(Value::Object(runtime))
            if runtime.get("name").and_then(Value::as_str) == Some(runtime_name) =>
        {
            runtime.insert("name".to_string(), Value::String(runtime_name.to_string()));
            runtime.insert("version".to_string(), Value::String(version.to_string()));
            runtime.insert("onFail".to_string(), Value::String("download".to_string()));
        }
        Some(existing) => {
            *existing = Value::Array(vec![existing.clone(), runtime_entry]);
        }
    }
    Ok(())
}

fn ensure_object_field<'a>(
    manifest: &'a mut Value,
    field: &str,
) -> Result<&'a mut Map<String, Value>, PackageManifestError> {
    let Some(root) = manifest.as_object_mut() else {
        return Err(PackageManifestError::InvalidAttribute(
            "the manifest root must be an object".to_string(),
        ));
    };
    let value = root.entry(field.to_string()).or_insert_with(|| Value::Object(Map::new()));
    if value.is_null() {
        *value = Value::Object(Map::new());
    }
    value.as_object_mut().ok_or_else(|| {
        PackageManifestError::InvalidAttribute(format!("the {field} field must be an object"))
    })
}

fn merge_runtime_entry(
    runtime: &mut Value,
    runtime_name: &str,
    version: &str,
) -> Result<(), PackageManifestError> {
    let Some(runtime) = runtime.as_object_mut() else {
        return Err(PackageManifestError::InvalidAttribute(
            "runtime entries must be objects".to_string(),
        ));
    };
    runtime.insert("name".to_string(), Value::String(runtime_name.to_string()));
    runtime.insert("version".to_string(), Value::String(version.to_string()));
    runtime.insert("onFail".to_string(), Value::String("download".to_string()));
    Ok(())
}
