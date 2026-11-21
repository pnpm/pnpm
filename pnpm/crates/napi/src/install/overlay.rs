use super::{
    Arc,
    BTreeSet,
    ConfigOverlay,
    HashMap,
    InstallOptions,
    IpAddr,
    NetworkConfigInput,
    NoProxySetting,
    NodeApiProject,
    PackageExtensionInput,
    PackageManifest,
    PathBuf,
    ProxyConfig,
    ProxyConfigInput,
    ThrottledClient,
    TlsConfig,
    invalid_option_error,
    to_napi_error,
    unsupported_option_error,
};

/// The caller's projects replace the workspace walk, so a listed project's
/// `package.json` is never read from disk. An empty list leaves discovery to
/// the engine: only an `ignorePackageManifest` install accepts one, and it
/// reads the lockfile, not the manifests.
pub(super) fn build_workspace_projects_override(
    projects: &[NodeApiProject],
) -> Option<Vec<pnpm_workspace::Project>> {
    if projects.is_empty() {
        return None;
    }
    Some(
        projects
            .iter()
            .map(|project| {
                let root_dir = PathBuf::from(&project.root_dir);
                let manifest_path = root_dir.join("package.json");
                let manifest =
                    PackageManifest::from_value(manifest_path.clone(), project.manifest.clone());
                let dependency_manifest = project.dependency_manifest
                    .as_ref()
                    .map(|value| PackageManifest::from_value(manifest_path.clone(), value.clone()));
                pnpm_workspace::Project { root_dir, manifest, dependency_manifest }
            })
            .collect(),
    )
}

/// Map the install options onto the config overlay. `fetch_shaped` is the
/// resolved `ignorePackageManifest` decision (see [`run_install_inner`](super::run_install_inner)):
/// it forces `virtualStoreOnly` on and the modules dir back on, the same
/// two settings the `pnpm fetch` handlers pin in both stacks.
pub(super) fn build_overlay(
    options: &InstallOptions,
    fetch_shaped: bool,
) -> napi::Result<ConfigOverlay> {
    let overlay = shared_network_overlay(
        "install",
        options.proxy_config.as_ref(),
        options.network_config.as_ref(),
    )?;
    let overlay = build_layout_overlay(options, fetch_shaped, overlay)?;
    let overlay = build_dependencies_overlay(options, fetch_shaped, overlay);
    let overlay = build_network_overlay(options, overlay);
    let overlay = build_fetch_overlay(options, overlay);
    let overlay = build_policy_overlay(options, overlay);
    Ok(overlay)
}

fn build_layout_overlay(
    options: &InstallOptions,
    fetch_shaped: bool,
    overlay: ConfigOverlay,
) -> napi::Result<ConfigOverlay> {
    Ok(ConfigOverlay {
        store_dir: options.store_dir.as_ref().map(PathBuf::from),
        cache_dir: options.cache_dir.as_ref().map(PathBuf::from),
        pnpm_home_dir: options.pnpm_home_dir.as_ref().map(PathBuf::from),
        virtual_store_only: fetch_shaped.then_some(true),
        // The engine reads `enableModulesDir: false` from the config: no
        // `node_modules`, every package fetched into the store.
        enable_modules_dir: if fetch_shaped { Some(true) } else { options.enable_modules_dir },
        registry: None,
        registries: options.registries
            .clone()
            .map(std::iter::IntoIterator::into_iter)
            .map(std::iter::Iterator::collect),
        node_linker: options.node_linker
            .as_deref()
            .map(parse_node_linker)
            .transpose()?,
        link_workspace_packages: parse_link_workspace_packages(
            options.link_workspace_packages.as_ref(),
        )?,
        package_import_method: options.package_import_method
            .as_deref()
            .map(parse_package_import_method_option)
            .transpose()?,
        virtual_store_dir_max_length: options.virtual_store_dir_max_length.map(u64::from),
        enable_global_virtual_store: options.enable_global_virtual_store,
        global_virtual_store_dir: options.global_virtual_store_dir.as_ref().map(PathBuf::from),
        lockfile_format: parse_lockfile_format(options.lockfile_format.as_deref())?,
        ..overlay
    })
}

fn build_dependencies_overlay(
    options: &InstallOptions,
    fetch_shaped: bool,
    overlay: ConfigOverlay,
) -> ConfigOverlay {
    ConfigOverlay {
        package_extensions: options.package_extensions
            .as_ref()
            .map(|extensions| {
                extensions
                    .iter()
                    .map(|(selector, extension)| (selector.clone(), package_extension(extension)))
                    .collect()
            }),
        patched_dependencies: options.patched_dependencies.clone(),
        allow_unused_patches: options.allow_unused_patches,
        hoist_pattern: options.hoist_pattern.clone(),
        public_hoist_pattern: options.public_hoist_pattern.clone(),
        external_dependencies: options.external_dependencies
            .as_ref()
            .map(|items| {
                items
                    .iter()
                    .cloned()
                    .collect::<BTreeSet<_>>()
            }),
        overrides: options.overrides.clone(),
        auto_install_peers: options.auto_install_peers,
        exclude_links_from_lockfile: options.exclude_links_from_lockfile,
        hoist_workspace_packages: options.hoist_workspace_packages,
        inject_workspace_packages: options.inject_workspace_packages,
        prefer_offline: options.prefer_offline,
        offline: options.offline,
        // Both of these installs are defined in terms of the lockfile, so an
        // ambient `lockfile: false` must not disable it underneath them.
        // `enableModulesDir: false` writes the lockfile while skipping
        // `node_modules`, so it runs through the lockfile-only path, which
        // requires the lockfile to be enabled, or the install fails with an
        // opaque `ERR_PNPM_CONFIG_CONFLICT_LOCKFILE_ONLY_WITH_NO_LOCKFILE`. A
        // fetch-shaped install reads the lockfile as its only input, and
        // would fail with `ERR_PNPM_NO_LOCKFILE`.
        lockfile: (fetch_shaped || !options.enable_modules_dir.unwrap_or(true)).then_some(true),
        prefer_frozen_lockfile: options.prefer_frozen_lockfile,
        dedupe_peer_dependents: options.dedupe_peer_dependents,
        dedupe_peers: options.dedupe_peers,
        add_missing_peer_types: options.add_missing_peer_types,
        project_scoped_optional_peers: options.project_scoped_optional_peers,
        dedupe_direct_deps: options.dedupe_direct_deps,
        dedupe_injected_deps: options.dedupe_injected_deps,
        resolve_peers_from_workspace_root: options.resolve_peers_from_workspace_root,
        peers_suffix_max_length: options.peers_suffix_max_length.map(u64::from),
        ..overlay
    }
}

/// The top-level install options win over the `networkConfig` values
/// [`shared_network_overlay`] already applied.
fn build_network_overlay(options: &InstallOptions, overlay: ConfigOverlay) -> ConfigOverlay {
    ConfigOverlay {
        network_concurrency: options.network_concurrency
            .map(|value| value as usize)
            .or(overlay.network_concurrency),
        fetch_retries: options.fetch_retries.or(overlay.fetch_retries),
        fetch_retry_factor: options.fetch_retry_factor.or(overlay.fetch_retry_factor),
        ..overlay
    }
}

fn build_fetch_overlay(options: &InstallOptions, overlay: ConfigOverlay) -> ConfigOverlay {
    ConfigOverlay {
        fetch_retry_mintimeout: options.fetch_retry_mintimeout
            .map(u64::from)
            .or(overlay.fetch_retry_mintimeout),
        fetch_retry_maxtimeout: options.fetch_retry_maxtimeout
            .map(u64::from)
            .or(overlay.fetch_retry_maxtimeout),
        fetch_timeout: options.fetch_timeout.map(u64::from).or(overlay.fetch_timeout),
        fetch_warn_timeout_ms: options.fetch_warn_timeout_ms
            .map(u64::from)
            .or(overlay.fetch_warn_timeout_ms),
        fetch_min_speed_ki_bps: options.fetch_min_speed_ki_bps
            .map(u64::from)
            .or(overlay.fetch_min_speed_ki_bps),
        user_agent: options.user_agent.clone().or(overlay.user_agent),
        ..overlay
    }
}

/// The `proxyConfig` and `networkConfig` options every engine call accepts.
/// `operation` names the call in an unsupported-option error.
pub(crate) fn shared_network_overlay(
    operation: &str,
    proxy_config: Option<&ProxyConfigInput>,
    network_config: Option<&NetworkConfigInput>,
) -> napi::Result<ConfigOverlay> {
    let proxy = proxy_config.map(|input| build_proxy_config(operation, input)).transpose()?;
    let tls = network_config.map(|input| build_tls_config(operation, input)).transpose()?;
    let Some(config) = network_config else {
        return Ok(ConfigOverlay { proxy, ..ConfigOverlay::default() });
    };
    Ok(ConfigOverlay {
        proxy,
        tls,
        network_concurrency: config.network_concurrency.map(|value| value as usize),
        max_sockets: config.max_sockets.map(|value| value as usize),
        fetch_retries: config.fetch_retries,
        fetch_retry_factor: config.fetch_retry_factor,
        fetch_retry_mintimeout: config.fetch_retry_mintimeout.map(u64::from),
        fetch_retry_maxtimeout: config.fetch_retry_maxtimeout.map(u64::from),
        fetch_timeout: config.fetch_timeout.map(u64::from),
        fetch_warn_timeout_ms: config.fetch_warn_timeout_ms.map(u64::from),
        fetch_min_speed_ki_bps: config.fetch_min_speed_ki_bps.map(u64::from),
        user_agent: config.user_agent.clone(),
        ..ConfigOverlay::default()
    })
}

fn build_policy_overlay(options: &InstallOptions, overlay: ConfigOverlay) -> ConfigOverlay {
    ConfigOverlay {
        // Embedders gate builds themselves, so default to report-not-fail.
        strict_dep_builds: Some(options.strict_dep_builds.unwrap_or(false)),
        allow_builds: options.allow_builds
            .clone()
            .map(std::iter::IntoIterator::into_iter)
            .map(std::iter::Iterator::collect),
        dangerously_allow_all_builds: options.dangerously_allow_all_builds,
        ignore_scripts: options.ignore_scripts,
        trust_lockfile: options.trust_lockfile,
        engine_strict: options.engine_strict,
        node_version: options.node_version.clone(),
        minimum_release_age: options.minimum_release_age.map(u64::from),
        minimum_release_age_exclude: options.minimum_release_age_exclude.clone(),
        peer_dependency_rules: options.peer_dependency_rules
            .as_ref()
            .map(|rules| crate::config::PeerDependencyRulesOverlay {
                ignore_missing: rules.ignore_missing.clone(),
                allow_any: rules.allow_any.clone(),
                allowed_versions: rules.allowed_versions
                    .clone()
                    .map(std::iter::IntoIterator::into_iter)
                    .map(std::iter::Iterator::collect),
            }),
        auth_header_by_uri: options.auth_header_by_uri
            .clone()
            .map(std::iter::IntoIterator::into_iter)
            .map(std::iter::Iterator::collect),
        ..overlay
    }
}

fn package_extension(input: &PackageExtensionInput) -> pnpm_config::PackageExtension {
    let to_sorted = |map: &Option<HashMap<String, String>>| {
        map.as_ref()
            .map(|map| {
                map.iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            })
    };
    pnpm_config::PackageExtension {
        dependencies: to_sorted(&input.dependencies),
        optional_dependencies: to_sorted(&input.optional_dependencies),
        peer_dependencies: to_sorted(&input.peer_dependencies),
        peer_dependencies_meta: input.peer_dependencies_meta
            .as_ref()
            .map(|meta| {
                meta.iter()
                    .map(|(name, entry)| {
                        (name.clone(), pnpm_config::PeerDependencyMeta { optional: entry.optional })
                    })
                    .collect()
            }),
    }
}

fn build_proxy_config(operation: &str, input: &ProxyConfigInput) -> napi::Result<ProxyConfig> {
    Ok(ProxyConfig {
        https_proxy: input.https_proxy.clone(),
        http_proxy: input.http_proxy.clone(),
        no_proxy: input.no_proxy
            .as_ref()
            .map(|value| parse_no_proxy(operation, value))
            .transpose()?
            .flatten(),
    })
}

fn parse_no_proxy(
    operation: &str,
    value: &serde_json::Value,
) -> napi::Result<Option<NoProxySetting>> {
    match value {
        serde_json::Value::Bool(true) => Ok(Some(NoProxySetting::Bypass)),
        serde_json::Value::Bool(false) | serde_json::Value::Null => Ok(None),
        serde_json::Value::String(items) => Ok(Some(NoProxySetting::List(
            items
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToOwned::to_owned)
                .collect(),
        ))),
        _ => Err(unsupported_option_error(operation, "proxyConfig.noProxy")),
    }
}

fn build_tls_config(operation: &str, input: &NetworkConfigInput) -> napi::Result<TlsConfig> {
    Ok(TlsConfig {
        ca: input.ca
            .as_ref()
            .map(|value| parse_string_list(operation, value))
            .transpose()?
            .unwrap_or_default(),
        cert: input.cert
            .as_ref()
            .map(|value| parse_single_string(operation, value))
            .transpose()?
            .flatten(),
        key: input.key.clone(),
        strict_ssl: input.strict_ssl,
        local_address: input.local_address
            .as_deref()
            .map(parse_local_address)
            .transpose()?,
    })
}

fn parse_local_address(value: &str) -> napi::Result<IpAddr> {
    value
        .parse()
        .map_err(|_| invalid_option_error("networkConfig.localAddress", value, "an IP address"))
}

fn parse_string_list(operation: &str, value: &serde_json::Value) -> napi::Result<Vec<String>> {
    match value {
        serde_json::Value::String(item) => Ok(vec![item.clone()]),
        serde_json::Value::Array(items) => items
            .iter()
            .map(|item| match item {
                serde_json::Value::String(value) => Ok(value.clone()),
                _ => Err(unsupported_option_error(operation, "networkConfig.ca")),
            })
            .collect(),
        _ => Err(unsupported_option_error(operation, "networkConfig.ca")),
    }
}

fn parse_single_string(operation: &str, value: &serde_json::Value) -> napi::Result<Option<String>> {
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(item) => Ok(Some(item.clone())),
        _ => Err(unsupported_option_error(operation, "networkConfig.cert")),
    }
}

fn parse_node_linker(value: &str) -> napi::Result<pnpm_config::NodeLinker> {
    match value {
        "hoisted" => Ok(pnpm_config::NodeLinker::Hoisted),
        "isolated" => Ok(pnpm_config::NodeLinker::Isolated),
        "pnp" => Ok(pnpm_config::NodeLinker::Pnp),
        "loaded" => Ok(pnpm_config::NodeLinker::Loaded),
        _ => Err(invalid_option_error(
            "nodeLinker",
            value,
            r#""isolated", "hoisted", "pnp", or "loaded""#,
        )),
    }
}

/// Parse the JS `linkWorkspacePackages` value (`true` / `false` / `"deep"`)
/// into a [`pnpm_config::LinkWorkspacePackages`], reusing the config
/// crate's `Deserialize`. Rejects any other value.
fn parse_link_workspace_packages(
    value: Option<&serde_json::Value>,
) -> napi::Result<Option<pnpm_config::LinkWorkspacePackages>> {
    value
        .map(|value| {
            serde_json::from_value::<pnpm_config::LinkWorkspacePackages>(value.clone()).map_err(
                |error| {
                    napi::Error::from_reason(format!(
                        r#"invalid linkWorkspacePackages (expected true, false, or "deep"): {error}"#,
                    ))
                },
            )
        })
        .transpose()
}

fn parse_lockfile_format(
    value: Option<&str>,
) -> napi::Result<Option<pnpm_lockfile::LockfileFormat>> {
    value
        .map(|value| match value {
            "v9" => Ok(pnpm_lockfile::LockfileFormat::V9),
            "v13" => Ok(pnpm_lockfile::LockfileFormat::V13),
            _ => Err(napi::Error::from_reason(format!(
                r#"invalid lockfileFormat {value:?} (expected "v9" or "v13")"#,
            ))),
        })
        .transpose()
}

fn parse_package_import_method_option(
    value: &str,
) -> napi::Result<pnpm_config::PackageImportMethod> {
    parse_import_method(value)
        .ok_or_else(|| {
            invalid_option_error(
                "packageImportMethod",
                value,
                r#""auto", "hardlink", "copy", "clone", or "clone-or-copy""#,
            )
        })
}

pub(crate) fn parse_import_method(value: &str) -> Option<pnpm_config::PackageImportMethod> {
    match value {
        "auto" => Some(pnpm_config::PackageImportMethod::Auto),
        "hardlink" => Some(pnpm_config::PackageImportMethod::Hardlink),
        "copy" => Some(pnpm_config::PackageImportMethod::Copy),
        "clone" => Some(pnpm_config::PackageImportMethod::Clone),
        "clone-or-copy" => Some(pnpm_config::PackageImportMethod::CloneOrCopy),
        _ => None,
    }
}

pub(crate) fn install_http_client(
    config: &pnpm_config::Config,
) -> napi::Result<Arc<ThrottledClient>> {
    Ok(Arc::new(
        ThrottledClient::for_installs(
            &config.proxy,
            &config.tls,
            &config.tls_by_uri,
            &config.network_settings(),
        )
        .map_err(|error| to_napi_error(&error))?
        .with_max_sockets_per_host(config.max_sockets),
    ))
}
