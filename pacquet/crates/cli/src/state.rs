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
use std::{path::{Path, PathBuf}, sync::Arc};

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

    #[display(
        "The \"resolutions\" field in package.json conflicts with \"overrides\" in \
         pnpm-workspace.yaml. Remove \"resolutions\" from package.json. To suppress this \
         error, use the --ignore-resolutions-conflict flag."
    )]
    #[diagnostic(code(ERR_PNPM_RESOLUTIONS_CONFLICT_WITH_OVERRIDES))]
    ResolutionsConflictWithOverrides,

    #[display("The value of resolutions.{selector} should be a string, but got {actual_type}")]
    #[diagnostic(code(ERR_PNPM_INVALID_RESOLUTIONS))]
    InvalidResolutionValue { selector: String, actual_type: String },

    #[display("The resolutions field should be an object, but got {actual_type}")]
    #[diagnostic(code(ERR_PNPM_INVALID_RESOLUTIONS))]
    InvalidResolutionsType { actual_type: String },

    #[display(
        "Cannot resolve version {spec} in overrides. The direct dependencies don't have dependency \"{dep_name}\"."
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
        manifest_path: PathBuf,
        config: &'static Config,
        require_lockfile: bool,
    ) -> Result<Self, InitStateError> {
        let should_load = config.lockfile || require_lockfile;
        let manifest = manifest_path
            .pipe(PackageManifest::create_if_needed)
            .map_err(InitStateError::Manifest)?;
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
/// when no workspace overrides exist, or error / warn on conflict. Mirrors
/// upstream's
/// [`addSettingsFromWorkspaceManifestToConfig`].
///
/// Reads the root manifest from `config.workspace_dir` (set when a
/// `pnpm-workspace.yaml` is found). When the project manifest *is* the root
/// (no workspace), its own `resolutions` field is used. No-op when there is
/// no root manifest.
///
/// [`addSettingsFromWorkspaceManifestToConfig`]: https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/index.ts#L875-L889
pub fn apply_root_resolutions_to_config(
    config: &mut Config,
    project_manifest_path: &Path,
) -> Result<(), InitStateError> {
    let project_manifest =
        PackageManifest::create_if_needed(project_manifest_path.to_path_buf()).map_err(InitStateError::Manifest)?;
    let root_manifest_path = config.workspace_dir.as_ref().map(|dir| dir.join("package.json"));
    let root_manifest = match root_manifest_path {
        Some(ref path) if path != project_manifest.path() => {
            pacquet_package_manifest::safe_read_package_json_from_dir(path.parent().unwrap())
                .map_err(InitStateError::Manifest)?
        }
        Some(_) => Some(project_manifest.value().clone()),
        None => None,
    };
    if let Some(root_value) = root_manifest {
        apply_resolutions_to_config(config, &root_value)?;
    }
    Ok(())
}

/// Read `resolutions` from root `package.json` and either promote them to
/// `config.overrides` (when no workspace overrides exist), error on conflict
/// (when both exist), or emit a deprecation warning. Mirrors upstream's
/// [`addSettingsFromWorkspaceManifestToConfig`].
///
/// [`addSettingsFromWorkspaceManifestToConfig`]: https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/reader/src/index.ts#L875-L889
fn apply_resolutions_to_config(
    config: &mut Config,
    root_manifest: &serde_json::Value,
) -> Result<(), InitStateError> {
    let resolutions_raw = match root_manifest.get("resolutions") {
        None | Some(serde_json::Value::Null) => return Ok(()),
        Some(v) => v,
    };
    let resolutions = match resolutions_raw {
        serde_json::Value::Object(map) if !map.is_empty() => map,
        serde_json::Value::Object(_) => return Ok(()),
        other => {
            return Err(InitStateError::InvalidResolutionsType {
                actual_type: json_value_type_name(other),
            });
        }
    };
    for (key, value) in resolutions.iter() {
        if !value.is_string() {
            return Err(InitStateError::InvalidResolutionValue {
                selector: key.clone(),
                actual_type: json_value_type_name(value),
            });
        }
    }
    let has_overrides = config.overrides.as_ref().is_some_and(|overrides| !overrides.is_empty());
    if has_overrides {
        if config.ignore_resolutions_conflict {
            eprintln!(
                " WARN  The \"resolutions\" field in package.json is ignored because \
                 \"overrides\" in pnpm-workspace.yaml takes precedence. Remove \
                 \"resolutions\" from package.json.",
            );
        } else {
            return Err(InitStateError::ResolutionsConflictWithOverrides);
        }
    } else {
        eprintln!(
            " WARN  The \"resolutions\" field in package.json is deprecated. Use \
             the \"overrides\" field in pnpm-workspace.yaml instead.",
        );
        let overrides: IndexMap<String, String> = resolutions
            .into_iter()
            .map(|(k, v)| {
                let spec = v.as_str().unwrap();
                resolve_version_reference(spec, root_manifest).map(|resolved| (k.clone(), resolved))
            })
            .collect::<Result<_, _>>()?;
        if !overrides.is_empty() {
            config.overrides = Some(overrides);
        }
    }
    Ok(())
}

fn resolve_version_reference(
    spec: &str,
    manifest: &serde_json::Value,
) -> Result<String, InitStateError> {
    if !spec.starts_with('$') {
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
            spec: spec.to_owned(),
            dep_name: dep_name.to_owned(),
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

#[cfg(test)]
mod tests;
