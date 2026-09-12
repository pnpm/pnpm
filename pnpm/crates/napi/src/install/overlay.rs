use super::{
    BTreeSet, ConfigOverlay, HashMap, InstallOptions, IpAddr, NetworkConfigInput, NoProxySetting,
    NodeApiProject, PackageExtensionInput, PackageManifest, PathBuf, ProxyConfig, ProxyConfigInput,
    TlsConfig, unsupported_option_error,
};

pub(super) fn build_workspace_projects_override(
    projects: &[NodeApiProject],
) -> Option<Vec<pnpm_workspace::Project>> {
    if projects.len() <= 1 {
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
                let dependency_manifest = project
                    .dependency_manifest
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
    let overlay = ConfigOverlay::default();
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
    let network_config = options.network_config.as_ref();
    Ok(ConfigOverlay {
        store_dir: options.store_dir.as_ref().map(PathBuf::from),
        cache_dir: options.cache_dir.as_ref().map(PathBuf::from),
        pnpm_home_dir: options.pnpm_home_dir.as_ref().map(PathBuf::from),
        virtual_store_only: fetch_shaped.then_some(true),
        enable_modules_dir: fetch_shaped.then_some(true),
        registry: None,
        registries: options.registries.as_ref().map(|map| map.clone().into_iter().collect()),
        proxy: options.proxy_config.as_ref().map(build_proxy_config).transpose()?,
        tls: network_config.map(build_tls_config).transpose()?,
        node_linker: options.node_linker.as_deref().and_then(parse_node_linker),
        link_workspace_packages: parse_link_workspace_packages(
            options.link_workspace_packages.as_ref(),
        )?,
        package_import_method: options
            .package_import_method
            .as_deref()
            .and_then(parse_import_method),
        virtual_store_dir_max_length: options.virtual_store_dir_max_length.map(u64::from),
        enable_global_virtual_store: options.enable_global_virtual_store,
        global_virtual_store_dir: options.global_virtual_store_dir.as_ref().map(PathBuf::from),
        ..overlay
    })
}

fn build_dependencies_overlay(
    options: &InstallOptions,
    fetch_shaped: bool,
    overlay: ConfigOverlay,
) -> ConfigOverlay {
    ConfigOverlay {
        package_extensions: options.package_extensions.as_ref().map(|extensions| {
            extensions
                .iter()
                .map(|(selector, extension)| (selector.clone(), package_extension(extension)))
                .collect()
        }),
        patched_dependencies: options.patched_dependencies.clone(),
        allow_unused_patches: options.allow_unused_patches,
        hoist_pattern: options.hoist_pattern.clone(),
        public_hoist_pattern: options.public_hoist_pattern.clone(),
        external_dependencies: options
            .external_dependencies
            .as_ref()
            .map(|items| items.iter().cloned().collect::<BTreeSet<_>>()),
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
        // `node_modules`, so it runs through the lockfile-only path — which
        // requires the lockfile to be enabled, or the install fails with an
        // opaque `ERR_PNPM_CONFIG_CONFLICT_LOCKFILE_ONLY_WITH_NO_LOCKFILE`. A
        // fetch-shaped install reads the lockfile as its only input, and
        // would fail with `ERR_PNPM_NO_LOCKFILE`.
        lockfile: (fetch_shaped || options.enable_modules_dir == Some(false)).then_some(true),
        prefer_frozen_lockfile: options.prefer_frozen_lockfile,
        dedupe_peer_dependents: options.dedupe_peer_dependents,
        dedupe_peers: options.dedupe_peers,
        dedupe_direct_deps: options.dedupe_direct_deps,
        dedupe_injected_deps: options.dedupe_injected_deps,
        resolve_peers_from_workspace_root: options.resolve_peers_from_workspace_root,
        peers_suffix_max_length: options.peers_suffix_max_length.map(u64::from),
        ..overlay
    }
}

fn build_network_overlay(options: &InstallOptions, overlay: ConfigOverlay) -> ConfigOverlay {
    let network_config = options.network_config.as_ref();
    ConfigOverlay {
        network_concurrency: options
            .network_concurrency
            .or_else(|| network_config.and_then(|config| config.network_concurrency))
            .map(|value| value as usize),
        max_sockets: network_config
            .and_then(|config| config.max_sockets)
            .map(|value| value as usize),
        fetch_retries: options
            .fetch_retries
            .or_else(|| network_config.and_then(|config| config.fetch_retries)),
        fetch_retry_factor: options
            .fetch_retry_factor
            .or_else(|| network_config.and_then(|config| config.fetch_retry_factor)),
        ..overlay
    }
}

fn build_fetch_overlay(options: &InstallOptions, overlay: ConfigOverlay) -> ConfigOverlay {
    let network_config = options.network_config.as_ref();
    ConfigOverlay {
        fetch_retry_mintimeout: options
            .fetch_retry_mintimeout
            .or_else(|| network_config.and_then(|config| config.fetch_retry_mintimeout))
            .map(u64::from),
        fetch_retry_maxtimeout: options
            .fetch_retry_maxtimeout
            .or_else(|| network_config.and_then(|config| config.fetch_retry_maxtimeout))
            .map(u64::from),
        fetch_timeout: options
            .fetch_timeout
            .or_else(|| network_config.and_then(|config| config.fetch_timeout))
            .map(u64::from),
        fetch_warn_timeout_ms: options
            .fetch_warn_timeout_ms
            .or_else(|| network_config.and_then(|config| config.fetch_warn_timeout_ms))
            .map(u64::from),
        fetch_min_speed_ki_bps: options
            .fetch_min_speed_ki_bps
            .or_else(|| network_config.and_then(|config| config.fetch_min_speed_ki_bps))
            .map(u64::from),
        user_agent: options
            .user_agent
            .clone()
            .or_else(|| network_config.and_then(|config| config.user_agent.clone())),
        ..overlay
    }
}

fn build_policy_overlay(options: &InstallOptions, overlay: ConfigOverlay) -> ConfigOverlay {
    ConfigOverlay {
        // Embedders gate builds themselves, so default to report-not-fail.
        strict_dep_builds: Some(options.strict_dep_builds.unwrap_or(false)),
        allow_builds: options.allow_builds.clone().map(|map| map.into_iter().collect()),
        dangerously_allow_all_builds: options.dangerously_allow_all_builds,
        ignore_scripts: options.ignore_scripts,
        trust_lockfile: options.trust_lockfile,
        engine_strict: options.engine_strict,
        node_version: options.node_version.clone(),
        minimum_release_age: options.minimum_release_age.map(u64::from),
        minimum_release_age_exclude: options.minimum_release_age_exclude.clone(),
        peer_dependency_rules: options.peer_dependency_rules.as_ref().map(|rules| {
            crate::config::PeerDependencyRulesOverlay {
                ignore_missing: rules.ignore_missing.clone(),
                allow_any: rules.allow_any.clone(),
                allowed_versions: rules
                    .allowed_versions
                    .as_ref()
                    .map(|map| map.clone().into_iter().collect()),
            }
        }),
        auth_header_by_uri: options.auth_header_by_uri.clone().map(|map| map.into_iter().collect()),
        ..overlay
    }
}

fn package_extension(input: &PackageExtensionInput) -> pnpm_config::PackageExtension {
    let to_sorted = |map: &Option<HashMap<String, String>>| {
        map.as_ref().map(|map| map.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
    };
    pnpm_config::PackageExtension {
        dependencies: to_sorted(&input.dependencies),
        optional_dependencies: to_sorted(&input.optional_dependencies),
        peer_dependencies: to_sorted(&input.peer_dependencies),
        peer_dependencies_meta: input.peer_dependencies_meta.as_ref().map(|meta| {
            meta.iter()
                .map(|(name, entry)| {
                    (name.clone(), pnpm_config::PeerDependencyMeta { optional: entry.optional })
                })
                .collect()
        }),
    }
}

fn build_proxy_config(input: &ProxyConfigInput) -> napi::Result<ProxyConfig> {
    Ok(ProxyConfig {
        https_proxy: input.https_proxy.clone(),
        http_proxy: input.http_proxy.clone(),
        no_proxy: input.no_proxy.as_ref().map(parse_no_proxy).transpose()?.flatten(),
    })
}

fn parse_no_proxy(value: &serde_json::Value) -> napi::Result<Option<NoProxySetting>> {
    match value {
        serde_json::Value::Bool(true) => Ok(Some(NoProxySetting::Bypass)),
        serde_json::Value::Bool(false) | serde_json::Value::Null => Ok(None),
        serde_json::Value::String(items) => {
            let entries = items.split(',').map(str::trim).filter(|item| !item.is_empty());
            Ok(Some(NoProxySetting::List(entries.map(ToOwned::to_owned).collect())))
        }
        _ => Err(unsupported_option_error("install", "proxyConfig.noProxy")),
    }
}

fn build_tls_config(input: &NetworkConfigInput) -> napi::Result<TlsConfig> {
    Ok(TlsConfig {
        ca: input.ca.as_ref().map(parse_string_list).transpose()?.unwrap_or_default(),
        cert: input.cert.as_ref().map(parse_single_string).transpose()?.flatten(),
        key: input.key.clone(),
        strict_ssl: input.strict_ssl,
        local_address: input
            .local_address
            .as_deref()
            .and_then(|value| value.parse::<IpAddr>().ok()),
    })
}

fn parse_string_list(value: &serde_json::Value) -> napi::Result<Vec<String>> {
    match value {
        serde_json::Value::String(item) => Ok(vec![item.clone()]),
        serde_json::Value::Array(items) => items
            .iter()
            .map(|item| match item {
                serde_json::Value::String(value) => Ok(value.clone()),
                _ => Err(unsupported_option_error("install", "networkConfig.ca")),
            })
            .collect(),
        _ => Err(unsupported_option_error("install", "networkConfig.ca")),
    }
}

fn parse_single_string(value: &serde_json::Value) -> napi::Result<Option<String>> {
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(item) => Ok(Some(item.clone())),
        _ => Err(unsupported_option_error("install", "networkConfig.cert")),
    }
}

fn parse_node_linker(value: &str) -> Option<pnpm_config::NodeLinker> {
    match value {
        "hoisted" => Some(pnpm_config::NodeLinker::Hoisted),
        "isolated" => Some(pnpm_config::NodeLinker::Isolated),
        "pnp" => Some(pnpm_config::NodeLinker::Pnp),
        _ => None,
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
