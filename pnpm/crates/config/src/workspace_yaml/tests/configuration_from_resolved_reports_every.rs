use super::{
    AuditLevel, BTreeMap, Config, IndexMap, NodeLinker, Path, PathBuf, PmOnFail, ProjectConfig,
    RemoteSideEffectsCacheSettings, RuntimeOnFail, SupportedArchitectures, UNREPORTED_SETTINGS,
    UpdateConfig, WorkspaceSettings, assert_eq,
};

/// Every setting must report its resolved value, so that a hook reading the
/// configuration sees what the install runs with. A key absent from the
/// projection reads as `null`, which is indistinguishable from "explicitly
/// unset" — so adding a setting to [`WorkspaceSettings`] without teaching
/// [`WorkspaceSettings::from_resolved`] about it fails here.
///
/// Settings that are genuinely `Option` on [`Config`] are exempt: a `None`
/// there is pnpm's own "unset", not a gap in the projection. The fixture
/// below therefore sets every such value, so anything still `null` is
/// unmapped.
#[test]
fn from_resolved_reports_every_setting() {
    let config = Config {
        scope: Some("@acme".to_string()),
        pnpr_server: Some("https://pnpr.example".to_string()),
        frozen_lockfile: Some(true),
        reporter_hide_prefix: Some(true),
        prefer_symlinked_executables: Some(true),
        max_sockets: Some(4),
        node_version: Some("24.0.0".to_string()),
        script_shell: Some("/bin/bash".to_string()),
        node_options: Some("--max-old-space-size=8192".to_string()),
        patches_dir: Some("patches".to_string()),
        save_prefix: Some("~".to_string()),
        save_catalog_name: Some("default".to_string()),
        pipeline_base: Some("origin/main".to_string()),
        init_author_name: Some("Example".to_string()),
        init_author_email: Some("example@example.com".to_string()),
        init_author_url: Some("https://example.com".to_string()),
        init_license: Some("MIT".to_string()),
        init_version: Some("1.0.0".to_string()),
        minimum_release_age_strict: Some(true),
        minimum_release_age_exclude: Some(vec!["is-positive".to_string()]),
        trust_policy_exclude: Some(vec!["is-negative".to_string()]),
        trust_policy_ignore_after: Some(1),
        lockfile_dir: Some(PathBuf::from("/tmp/project")),
        npmrc_auth_file: Some(PathBuf::from("/tmp/auth.ini")),
        global_pnpmfile: Some(PathBuf::from("/tmp/global/pnpmfile.cjs")),
        pnpmfile: Some(vec![PathBuf::from("/tmp/project/.pnpmfile.cjs")]),
        workspace_package_patterns: Some(vec!["packages/*".to_string()]),
        patched_dependencies: Some(IndexMap::from([(
            "is-positive".to_string(),
            "patches/is-positive.patch".to_string(),
        )])),
        overrides: Some(IndexMap::from([("is-positive".to_string(), "1.0.0".to_string())])),
        package_configs: Some(IndexMap::from([(
            "@acme/app".to_string(),
            ProjectConfig { save_exact: Some(true), ..ProjectConfig::default() },
        )])),
        ignored_optional_dependencies: Some(vec!["fsevents".to_string()]),
        supported_architectures: Some(SupportedArchitectures::default()),
        config_dependencies: Some(BTreeMap::new()),
        package_extensions: Some(IndexMap::default()),
        catalogs: Some(BTreeMap::default()),
        remote_side_effects_cache: Some(RemoteSideEffectsCacheSettings::default()),
        runtime_on_fail: Some(RuntimeOnFail::Warn),
        pm_on_fail: Some(PmOnFail::Warn),
        side_effects_cache_read_setting: Some(true),
        side_effects_cache_write_setting: Some(true),
        audit_level: Some(AuditLevel::Low),
        proxy: pnpm_network::ProxyConfig {
            https_proxy: Some("https://proxy.example".to_string()),
            http_proxy: Some("http://proxy.example".to_string()),
            no_proxy: Some(pnpm_network::NoProxySetting::List(vec!["example.com".to_string()])),
        },
        update_config: UpdateConfig { changeset: Some(true), ..UpdateConfig::default() },
        // The settings that report as written, rather than as resolved.
        explicit_settings: [
            "storeDir",
            "cacheDir",
            "modulesDir",
            "virtualStoreDir",
            "globalVirtualStoreDir",
            "globalDir",
            "globalBinDir",
        ]
        .into_iter()
        .map(|key| (key.to_string(), serde_json::Value::String(format!("../{key}"))))
        .chain(
            [
                "preferFrozenLockfile",
                "lockfile",
                "shamefullyHoist",
                "mergeGitBranchLockfiles",
                "optimisticRepeatInstall",
                "preferSymlinkedExecutables",
            ]
            .into_iter()
            .map(|key| (key.to_string(), serde_json::Value::Bool(true))),
        )
        .chain(std::iter::once((
            "minimumReleaseAge".to_string(),
            serde_json::Value::Number(60.into()),
        )))
        .collect(),
        ..Config::default()
    };

    let projected = WorkspaceSettings::from_resolved(&config);
    let Ok(serde_json::Value::Object(map)) = serde_json::to_value(&projected) else {
        panic!("the projected settings serialize to a JSON object");
    };

    let unreported: Vec<&str> =
        map.iter().filter(|(_, value)| value.is_null()).map(|(key, _)| key.as_str()).collect();
    let mut expected = UNREPORTED_SETTINGS.to_vec();
    expected.sort_unstable();
    let mut found = unreported;
    found.sort_unstable();
    dbg!(&found);
    assert_eq!(
        found, expected,
        "a setting reporting `null` is one `from_resolved` does not map; add the mapping, or add \
         the key to UNREPORTED_SETTINGS with the reason it reports elsewhere",
    );
}

/// Every setting `from_resolved` reports can be unset again, deprecated
/// spellings included, so a source deleting a setting never leaves the
/// resolved value behind.
#[test]
fn reset_setting_to_default_covers_every_setting() {
    let defaults = Config::default();
    let Ok(serde_json::Value::Object(map)) =
        serde_json::to_value(WorkspaceSettings::from_resolved(&defaults))
    else {
        panic!("the projected settings serialize to a JSON object");
    };
    let mut config =
        Config { node_linker: NodeLinker::Hoisted, lockfile: false, ..Config::default() };
    let unhandled: Vec<&str> = map
        .keys()
        .map(String::as_str)
        .filter(|key| {
            !WorkspaceSettings::reset_setting_to_default::<crate::Host>(
                &mut config,
                &defaults,
                key,
                Path::new("/tmp/project"),
            )
        })
        .collect();
    assert_eq!(unhandled, Vec::<&str>::new(), "settings with no reset");
    assert_eq!(config.node_linker, defaults.node_linker);
    assert_eq!(config.lockfile, defaults.lockfile);
}

/// Unsetting a setting another is derived from re-runs the derivation
/// against the settings still set, so an explicit `publicHoistPattern`
/// outlives `shamefullyHoist`, `lockfile` follows `packageLock`, and the
/// hoist pattern returns with `hoist`.
#[test]
fn reset_setting_to_default_rederives_from_the_settings_still_set() {
    let defaults = Config::default();
    let base_dir = Path::new("/tmp/project");
    let mut config = Config {
        shamefully_hoist: true,
        public_hoist_pattern: Some(vec!["*".to_string()]),
        package_lock: false,
        lockfile: true,
        hoist: false,
        hoist_pattern: None,
        explicit_settings: serde_json::Map::from_iter([(
            "publicHoistPattern".to_string(),
            serde_json::json!(["@types/*"]),
        )]),
        ..Config::default()
    };
    for key in ["shamefullyHoist", "lockfile", "hoist"] {
        assert!(WorkspaceSettings::reset_setting_to_default::<crate::Host>(
            &mut config,
            &defaults,
            key,
            base_dir,
        ));
    }
    assert!(!config.shamefully_hoist);
    assert_eq!(config.public_hoist_pattern, Some(vec!["@types/*".to_string()]));
    assert!(!config.lockfile);
    assert!(config.hoist);
    assert_eq!(config.hoist_pattern, defaults.hoist_pattern);
}

/// A pattern a source disabled with `null` is absent from the explicit
/// settings, so leaving `virtualStoreOnly` keeps the disabled pattern the
/// mode snapshotted rather than falling back to the default.
#[test]
fn reset_setting_to_default_keeps_disabled_patterns_when_leaving_virtual_store_only() {
    let defaults = Config::default();
    let mut config = Config {
        virtual_store_only: true,
        hoist_pattern: None,
        public_hoist_pattern: None,
        ..Config::default()
    };
    config.apply_virtual_store_only_derivation();
    assert_eq!(config.hoist_pattern, Some(Vec::new()));

    WorkspaceSettings::reset_setting_to_default::<crate::Host>(
        &mut config,
        &defaults,
        "virtualStoreOnly",
        Path::new("/tmp/project"),
    );
    assert!(!config.virtual_store_only);
    assert_eq!(config.hoist_pattern, None);
    assert_eq!(config.public_hoist_pattern, None);
}

/// A setting pnpm reads for whether it was set reports as unset until a
/// source sets it, even though [`Config`] resolved it to a value. A caller
/// diffing the projection against a hook's answer can then tell "the hook
/// set this" from "the hook left it alone", which is what decides whether
/// the setting counts as configured at all.
#[test]
fn from_resolved_leaves_explicitness_sensitive_settings_unset() {
    let unset = Config { prefer_frozen_lockfile: true, lockfile: true, ..Config::default() };
    let projected = WorkspaceSettings::from_resolved(&unset);
    assert_eq!(projected.prefer_frozen_lockfile, None);
    assert_eq!(projected.lockfile, None);

    let configured = Config {
        prefer_frozen_lockfile: true,
        explicit_settings: serde_json::Map::from_iter([(
            "preferFrozenLockfile".to_string(),
            serde_json::Value::Bool(false),
        )]),
        ..Config::default()
    };
    assert_eq!(WorkspaceSettings::from_resolved(&configured).prefer_frozen_lockfile, Some(false));
}
