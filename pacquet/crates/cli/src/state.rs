use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::Diagnostic;
use pacquet_config::Config;
use pacquet_lockfile::LazyLockfile;
use pacquet_network::{ForInstallsError, NetworkSettings, ThrottledClient};
use pacquet_package_manager::ResolvedPackages;
use pacquet_package_manifest::{PackageManifest, PackageManifestError};
use pacquet_tarball::MemCache;
use pipe_trait::Pipe;
use std::sync::Arc;

/// Application state when running `pacquet run` or `pacquet install`.
pub struct State {
    /// Shared cache that stores downloaded tarballs. Held behind
    /// [`Arc`] so the resolve-time prefetch
    /// ([`pacquet_package_manager::PrefetchingResolver`]) can capture
    /// an owned clone into the `tokio::spawn`ed background download
    /// while every install sub-pipeline still takes a borrowed
    /// `&MemCache` via deref.
    pub tarball_mem_cache: Arc<MemCache>,
    /// HTTP client to make HTTP requests. Held behind [`std::sync::Arc`] so
    /// the lockfile-verification gate can own a clone for the
    /// `NpmResolutionVerifier`'s lifetime while every install
    /// sub-pipeline takes a borrowed `&ThrottledClient` via deref.
    pub http_client: std::sync::Arc<ThrottledClient>,
    /// Merged runtime configuration: built-in defaults, with overlays from
    /// the auth subset of `.npmrc` and from `pnpm-workspace.yaml`.
    pub config: &'static Config,
    /// Data from the `package.json` file.
    pub manifest: PackageManifest,
    /// The `pnpm-lock.yaml` file, read + parsed on first access so the
    /// repeat-install fast path (which never needs its contents) skips
    /// the YAML parse.
    pub lockfile: LazyLockfile,
    /// In-memory cache for packages that have started resolving dependencies.
    pub resolved_packages: ResolvedPackages,
}

/// Error type of [`State::init`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum InitStateError {
    #[diagnostic(transparent)]
    Manifest(#[error(source)] PackageManifestError),

    #[diagnostic(transparent)]
    Network(#[error(source)] ForInstallsError),

    #[display("The value of resolutions.{selector} should be a string, but got {actual_type}")]
    #[diagnostic(code(ERR_PNPM_INVALID_RESOLUTIONS))]
    InvalidResolutionValue { selector: String, actual_type: String },

    #[display("The resolutions field should be an object, but got {actual_type}")]
    #[diagnostic(code(ERR_PNPM_INVALID_RESOLUTIONS))]
    InvalidResolutionsType { actual_type: String },

    #[display(
        r#"Cannot resolve version {spec} in overrides. The direct dependencies don't have dependency "{dep_name}"."#
    )]
    #[diagnostic(code(ERR_PNPM_CANNOT_RESOLVE_OVERRIDE_VERSION))]
    CannotResolveOverrideVersion { spec: String, dep_name: String },
}

impl State {
    /// Initialize the application state.
    ///
    /// `require_lockfile` is `true` when the caller has committed to the
    /// frozen-lockfile install path (via `--frozen-lockfile`) and needs
    /// the lockfile loaded even when `config.lockfile` is `false`.
    /// Matches pnpm's CLI: `--frozen-lockfile` is the strongest signal,
    /// it must not be silently dropped because `lockfile` is disabled
    /// (or unset) in config.
    pub fn init(
        manifest: PackageManifest,
        config: &'static Config,
        require_lockfile: bool,
    ) -> Result<Self, InitStateError> {
        let should_load = config.lockfile || require_lockfile;
        let lockfile = if should_load {
            manifest
                .path()
                .parent()
                .expect("manifest path always has a parent dir")
                .to_path_buf()
                .pipe(LazyLockfile::deferred)
        } else {
            LazyLockfile::disabled()
        };
        Ok(State {
            config,
            manifest,
            lockfile,
            http_client: std::sync::Arc::new(
                ThrottledClient::for_installs(
                    &config.proxy,
                    &config.tls,
                    &config.tls_by_uri,
                    &NetworkSettings {
                        network_concurrency: config.network_concurrency,
                        fetch_timeout: std::time::Duration::from_millis(config.fetch_timeout),
                        user_agent: config.user_agent.clone(),
                    },
                )
                .map_err(InitStateError::Network)?,
            ),
            tarball_mem_cache: Arc::new(MemCache::new()),
            resolved_packages: ResolvedPackages::new(),
        })
    }
}

/// Promote `resolutions` from the root `package.json` into `config.overrides`
/// when no workspace overrides exist, or warn when both are present (in which
/// case `overrides` wins and `resolutions` is dropped). Mirrors upstream's
/// [`addSettingsFromWorkspaceManifestToConfig`].
///
/// Takes the already-read project manifest so the caller can reuse it for
/// [`State::init`] — `package.json` is parsed once per command, not twice.
/// Reads the root manifest from `config.workspace_dir` (set when a
/// `pnpm-workspace.yaml` is found); when there is no workspace, the project
/// manifest *is* the root, so its own `resolutions` field is used.
///
/// Returns the user-facing warning strings the caller should surface via
/// the active reporter's `LogEvent::Pnpm` channel (`level: Warn`). This
/// function does no I/O of its own so `--reporter=silent` can drop the
/// warnings and `--reporter=ndjson` can serialize them.
///
/// [`addSettingsFromWorkspaceManifestToConfig`]: https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/index.ts#L875-L889
pub fn apply_root_resolutions_to_config(
    config: &mut Config,
    project_manifest: &PackageManifest,
) -> Result<Vec<String>, InitStateError> {
    let root_manifest_path = config.workspace_dir.as_ref().map(|dir| dir.join("package.json"));
    match root_manifest_path {
        // Workspace root differs from project root: read the workspace's
        // package.json separately. The owned `Value` stays in this scope
        // and is borrowed for the call below.
        Some(ref path) if path != project_manifest.path() => {
            let root_manifest =
                pacquet_package_manifest::safe_read_package_json_from_dir(path.parent().unwrap())
                    .map_err(InitStateError::Manifest)?;
            if let Some(value) = root_manifest {
                apply_resolutions_to_config(config, &value)
            } else {
                Ok(Vec::new())
            }
        }
        // Either workspace root matches project root, or there's no
        // workspace — in both cases the project manifest IS the root.
        // Borrow directly to avoid a deep `serde_json::Value::clone()`
        // of the entire manifest tree on every install/add/update/remove
        // startup.
        _ => apply_resolutions_to_config(config, project_manifest.value()),
    }
}

/// Read `resolutions` from root `package.json` and either promote them to
/// `config.overrides` (when no workspace overrides exist) or warn and drop
/// them (when both exist). Mirrors upstream's
/// [`addSettingsFromWorkspaceManifestToConfig`].
///
/// Returns warning strings for the caller to emit; see
/// [`apply_root_resolutions_to_config`].
///
/// [`addSettingsFromWorkspaceManifestToConfig`]: https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/index.ts#L875-L889
fn apply_resolutions_to_config(
    config: &mut Config,
    root_manifest: &serde_json::Value,
) -> Result<Vec<String>, InitStateError> {
    let resolutions_raw = match root_manifest.get("resolutions") {
        None | Some(serde_json::Value::Null) => return Ok(Vec::new()),
        Some(v) => v,
    };
    let resolutions = match resolutions_raw {
        serde_json::Value::Object(map) if !map.is_empty() => map,
        serde_json::Value::Object(_) => return Ok(Vec::new()),
        other => {
            return Err(InitStateError::InvalidResolutionsType {
                actual_type: json_value_type_name(other),
            });
        }
    };
    for (key, value) in resolutions {
        if !value.is_string() {
            return Err(InitStateError::InvalidResolutionValue {
                selector: sanitize_for_log(key),
                actual_type: json_value_type_name(value),
            });
        }
    }
    let has_overrides = config.overrides.as_ref().is_some_and(|overrides| !overrides.is_empty());
    if has_overrides {
        Ok(vec![
        r#"The "resolutions" field in package.json is ignored because "overrides" in pnpm-workspace.yaml takes precedence. Remove "resolutions" from package.json."#
            .to_string(),
        ])
    } else {
        // `(selector, original_spec, resolved_spec)`. The original is
        // kept so the warning can show `original -> resolved` when the
        // `$dep` version-reference rewrite actually changed the value.
        let pairs: Vec<(String, String, String)> = resolutions
            .into_iter()
            .map(|(k, v)| {
                let original = v.as_str().unwrap();
                // Values are copied verbatim — `${VAR}` placeholders are NOT
                // expanded. Unlike `pnpm-workspace.yaml` overrides (which
                // expand env vars through `replaceEnvInSettings` / our
                // `substitute_optional_string_map`), `package.json` is a
                // repo-controlled manifest and its `resolutions` flow into
                // the lockfile's `overrides`, a shared and persisted
                // artifact. Expanding env vars here would materialize victim
                // environment secrets into the lockfile. `$dep` version
                // references (which start with `$`, not `${`) are still
                // resolved against the manifest below.
                let resolved = resolve_version_reference(original, root_manifest)?;
                Ok((k.clone(), original.to_owned(), resolved))
            })
            .collect::<Result<_, _>>()?;
        let overrides: IndexMap<String, String> =
            pairs.iter().map(|(k, _original, resolved)| (k.clone(), resolved.clone())).collect();
        if !overrides.is_empty() {
            config.overrides = Some(overrides);
        }
        let entries: Vec<String> = pairs
            .iter()
            .map(|(selector, original, resolved)| {
                // Selectors and specs are repo-controlled manifest values —
                // strip ASCII control chars before interpolation so a
                // malicious or malformed `package.json` can't inject fake
                // log lines or ANSI escapes into CI output. Mirrors
                // `sanitizeForLog` in upstream's getOptionsFromRootManifest.ts.
                let selector = sanitize_for_log(selector);
                let original = sanitize_for_log(original);
                let resolved = sanitize_for_log(resolved);
                if original == resolved {
                    format!("  {selector}: {resolved}")
                } else {
                    format!("  {selector}: {original} -> {resolved}")
                }
            })
            .collect();
        let message = format!(
            r#"The "resolutions" field in package.json is deprecated. We attempted to migrate your resolutions to pnpm overrides. Please verify:
{}
Use the "overrides" field in pnpm-workspace.yaml instead."#,
            entries.join("\n"),
        );
        Ok(vec![message])
    }
}

fn resolve_version_reference(
    spec: &str,
    manifest: &serde_json::Value,
) -> Result<String, InitStateError> {
    // `${VAR}` is the env-placeholder syntax, not a `$dep` version reference.
    // The two happen to share a leading `$`, but the brace disambiguates: a
    // version reference is always `$ident` (no brace). Env placeholders are
    // preserved literally so they don't materialize victim environment
    // secrets into the lockfile overrides.
    if !spec.starts_with('$') || spec.starts_with("${") {
        return Ok(spec.to_owned());
    }
    let dep_name = &spec[1..];
    let dep_version =
        ["optionalDependencies", "dependencies", "devDependencies"].iter().find_map(|field| {
            manifest.get(*field).and_then(|v| v.get(dep_name)).and_then(|v| v.as_str())
        });
    match dep_version {
        Some(v) => Ok(v.to_owned()),
        None => Err(InitStateError::CannotResolveOverrideVersion {
            spec: sanitize_for_log(spec),
            dep_name: sanitize_for_log(dep_name),
        }),
    }
}

fn json_value_type_name(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_owned(),
        serde_json::Value::Bool(_) => "boolean".to_owned(),
        serde_json::Value::Number(_) => "number".to_owned(),
        serde_json::Value::String(_) => "string".to_owned(),
        serde_json::Value::Array(_) => "array".to_owned(),
        serde_json::Value::Object(_) => "object".to_owned(),
    }
}

/// Replace ASCII control characters (C0 + DEL) with `'?'`.
///
/// Repo-controlled manifest values (selectors, version specs) are
/// interpolated into warnings and errors; without stripping, a
/// malicious `package.json` can inject fake log lines or ANSI escape
/// sequences into CI output. Mirrors upstream's `sanitizeForLog` in
/// `config/reader/src/getOptionsFromRootManifest.ts`.
fn sanitize_for_log(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            let code = ch as u32;
            if code <= 0x1F || code == 0x7F { '?' } else { ch }
        })
        .collect()
}

#[cfg(test)]
mod tests;
