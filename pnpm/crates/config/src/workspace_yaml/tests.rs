use super::{
    AllowBuild, LoadWorkspaceYamlError, SideEffectsCacheSetting, WORKSPACE_MANIFEST_FILENAME,
    WorkspaceSettings,
    registries::{RegistryDeclaration, RegistryEntry},
};
use crate::{
    AuditLevel, CatalogMode, ColorMode, Config, GlobalShims, GlobalShimsSetting, HoistingLimits,
    LinkWorkspacePackages, NodeLinker, NodePackageMapType, ResolutionMode, ScriptsPrependNodePath,
    ShimPolicy, TrustPolicy,
    api::{EnvVar, GetHomeDir},
};
use pipe_trait::Pipe;
use pnpm_lockfile::{RegistryOptions, RegistryServerType};
use pnpm_store_dir::StoreDir;
use pnpm_workspace_state::{ConfigDependency, ConfigDependencyDetail};
use pretty_assertions::assert_eq;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[test]
fn parses_common_settings_from_yaml() {
    let yaml = r"
storeDir: ../my-store
registry: https://reg.example
lockfile: false
autoInstallPeers: true
dedupePeers: true
preferWorkspacePackages: true
nodeLinker: hoisted
nodeExperimentalPackageMap: true
nodePackageMapType: loose
packages:
  - packages/*
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.store_dir.as_deref(), Some("../my-store"));
    assert_eq!(settings.registry.as_deref(), Some("https://reg.example"));
    assert_eq!(settings.lockfile, Some(false));
    assert_eq!(settings.auto_install_peers, Some(true));
    assert_eq!(settings.dedupe_peers, Some(true));
    assert_eq!(settings.prefer_workspace_packages, Some(true));
    assert!(matches!(settings.node_linker, Some(NodeLinker::Hoisted)));
    assert_eq!(settings.node_experimental_package_map, Some(true));
    assert_eq!(settings.node_package_map_type, Some(NodePackageMapType::Loose));
}

#[test]
fn parity_settings_parse_and_apply() {
    let settings: WorkspaceSettings = serde_saphyr::from_str(
        r"
bail: false
color: never
embedReadme: true
ignoreWorkspaceRootCheck: true
optional: false
packageLock: false
pending: true
recursiveInstall: false
reverse: true
shellEmulator: true
skipManifestObfuscation: true
sort: false
useBetaCli: true
",
    )
    .unwrap();
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/workspace"));

    assert!(!config.bail);
    assert_eq!(config.color, ColorMode::Never);
    assert!(config.embed_readme);
    assert!(config.ignore_workspace_root_check);
    assert!(!config.optional);
    assert!(!config.package_lock);
    assert!(config.pending);
    assert!(!config.recursive_install);
    assert!(config.reverse);
    assert!(config.shell_emulator);
    assert!(config.skip_manifest_obfuscation);
    assert!(!config.sort);
    assert!(config.use_beta_cli);
}

#[test]
fn color_accepts_boolean_compatibility_values() {
    let always: WorkspaceSettings = serde_saphyr::from_str("color: true\n").unwrap();
    let never: WorkspaceSettings = serde_saphyr::from_str("color: false\n").unwrap();
    assert_eq!(always.color, Some(ColorMode::Always));
    assert_eq!(never.color, Some(ColorMode::Never));
}

#[test]
fn parity_settings_follow_global_config_key_routing() {
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(
        r"
bail: false
color: never
embedReadme: true
ignoreWorkspaceRootCheck: true
optional: false
packageLock: false
pending: true
recursiveInstall: false
reverse: true
shellEmulator: true
skipManifestObfuscation: true
sort: false
useBetaCli: true
",
    )
    .unwrap();
    settings.clear_workspace_only_fields();

    assert_eq!(settings.bail, Some(false));
    assert_eq!(settings.color, Some(ColorMode::Never));
    assert_eq!(settings.optional, Some(false));
    assert_eq!(settings.package_lock, Some(false));
    assert_eq!(settings.shell_emulator, Some(true));
    assert_eq!(settings.use_beta_cli, Some(true));
    assert_eq!(settings.embed_readme, None);
    assert_eq!(settings.ignore_workspace_root_check, None);
    assert_eq!(settings.pending, None);
    assert_eq!(settings.recursive_install, None);
    assert_eq!(settings.reverse, None);
    assert_eq!(settings.skip_manifest_obfuscation, None);
    assert_eq!(settings.sort, None);
}

#[test]
fn global_shims_defaults_enable_the_managed_runtimes() {
    let shims = Config::default().global_shims;
    for name in ["node", "deno", "bun"] {
        assert!(shims.is_enabled(name), "{name} should be enabled by default");
    }
    assert!(!shims.is_enabled("typescript"));
    assert!(!shims.dispatches_nothing());
}

#[test]
fn global_shims_record_merges_over_the_defaults() {
    let settings: WorkspaceSettings =
        serde_saphyr::from_str("globalShims: {bun: false, typescript: true}\n").unwrap();
    assert!(matches!(settings.global_shims, Some(GlobalShimsSetting::Entries(_))));
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.global_shims.is_enabled("node"), "untouched defaults must survive");
    assert!(!config.global_shims.is_enabled("bun"), "one default can be switched off");
    assert!(config.global_shims.is_enabled("typescript"));
}

#[test]
fn global_shims_scalar_shorthands_reset_the_record() {
    let settings: WorkspaceSettings = serde_saphyr::from_str("globalShims: false\n").unwrap();
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.global_shims.dispatches_nothing());

    let settings: WorkspaceSettings = serde_saphyr::from_str("globalShims: true\n").unwrap();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.global_shims, GlobalShims::default());
}

#[test]
fn global_shims_named_policies_parse() {
    let settings: WorkspaceSettings =
        serde_saphyr::from_str("globalShims: {node: prompt, deno: always, bun: auto}\n").unwrap();
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    let shims = config.global_shims;
    assert_eq!(shims.policy("node"), ShimPolicy::Prompt);
    assert_eq!(shims.policy("deno"), ShimPolicy::Always);
    assert_eq!(shims.policy("bun"), ShimPolicy::Auto, "explicit auto equals the true shorthand");
    assert_eq!(shims.policy("typescript"), ShimPolicy::Off);
    assert!(shims.is_enabled("node"), "prompt still counts as enabled");
}

#[test]
fn global_shims_later_layers_win_per_key() {
    let mut shims = GlobalShims::default();
    shims.apply(&serde_saphyr::from_str::<GlobalShimsSetting>("{node: false}").unwrap());
    shims
        .apply(&serde_saphyr::from_str::<GlobalShimsSetting>("{node: true, deno: false}").unwrap());
    assert!(shims.is_enabled("node"));
    assert!(!shims.is_enabled("deno"));
    assert!(shims.is_enabled("bun"));
}

#[test]
fn parses_ignore_compatibility_db_from_yaml_and_applies() {
    let settings: WorkspaceSettings =
        serde_saphyr::from_str("ignoreCompatibilityDb: true\n").unwrap();
    assert_eq!(settings.ignore_compatibility_db, Some(true));

    let mut config = Config::new();
    assert!(!config.ignore_compatibility_db);
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.ignore_compatibility_db);
}

#[test]
fn swallows_unknown_top_level_keys() {
    let yaml = r#"
catalog:
  react: ^18
onlyBuiltDependencies:
  - esbuild
packages:
  - "apps/*"
"#;
    // `pnpm-workspace.yaml` commonly contains top-level keys we do not
    // model in `WorkspaceSettings` (packages list, catalogs, build
    // allow-lists, ...). This guards against regressions that would make
    // serde reject those unknown keys during deserialization — i.e.
    // someone adding `deny_unknown_fields` to the struct.
    let _settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
}

#[test]
fn load_at_buckets_the_problem_keys() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        concat!(
            "$schema: https://json.schemastore.org/pnpm-workspace.json\n",
            "configDir: /elsewhere\n",
            "minimumReleaseAg: 100\n",
            "store-dir: /some-store\n",
            "nodeLinker: hoisted\n",
            "globalShims:\n  node: true\n",
            "packages:\n  - apps/*\n",
        ),
    )
    .unwrap();

    let settings = WorkspaceSettings::load_at(dir.path())
        .expect("load pnpm-workspace.yaml")
        .expect("pnpm-workspace.yaml is present");

    assert_eq!(settings.key_issues.refused, ["configDir"]);
    assert_eq!(settings.key_issues.unrecognized, ["minimumReleaseAg"]);
    assert_eq!(settings.key_issues.non_camel_case, ["store-dir"]);
}

#[test]
fn load_at_collects_no_issues_from_a_clean_file() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "nodeLinker: hoisted\npackages:\n  - apps/*\ncatalog:\n  react: ^18\n",
    )
    .unwrap();

    let settings = WorkspaceSettings::load_at(dir.path())
        .expect("load pnpm-workspace.yaml")
        .expect("pnpm-workspace.yaml is present");

    assert!(settings.key_issues.is_empty(), "unexpected issues: {:?}", settings.key_issues);
}

#[test]
fn apply_overrides_npmrc_defaults() {
    let yaml = r"
storeDir: /absolute/store
lockfile: false
registry: https://reg.example
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    config.lockfile = true;
    let before_registry = config.registry.clone();

    settings.apply_to(&mut config, Path::new("/irrelevant-for-absolute-paths"));

    assert_eq!(config.store_dir, StoreDir::from(Path::new("/absolute/store").to_path_buf()));
    assert!(!config.lockfile);
    assert_eq!(config.registry, "https://reg.example/");
    assert_ne!(before_registry, config.registry);
}

#[test]
fn parses_and_applies_scope_from_yaml() {
    let settings: WorkspaceSettings = serde_saphyr::from_str("scope: '@my-org'\n").unwrap();
    assert_eq!(settings.scope.as_deref(), Some("@my-org"));

    let mut config = Config::new();
    assert_eq!(config.scope, None);
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.scope.as_deref(), Some("@my-org"));
}

/// `scope` is a valid key in the global `config.yaml` (pnpm's
/// `isConfigFileKey` accepts it), so it must survive the workspace-only
/// stripping that runs on that file.
#[test]
fn scope_survives_workspace_only_field_clearing() {
    let mut settings: WorkspaceSettings =
        serde_saphyr::from_str("scope: '@from-global'\n").unwrap();
    settings.clear_workspace_only_fields();
    assert_eq!(settings.scope.as_deref(), Some("@from-global"));
}

/// The workspace-structural keys are parsed so `pnpm config get` / `list`
/// can show them, but only from the workspace yaml: the global `config.yaml`
/// refuses them.
#[test]
fn structural_keys_are_parsed_and_are_workspace_only() {
    let yaml = "
packages: ['.']
catalog:
  react: ^19.0.0
catalogs:
  react17:
    react: ^17.0.0
onlyBuiltDependencies: [esbuild]
neverBuiltDependencies: [fsevents]
ignoredBuiltDependencies: [core-js]
";
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.packages.as_deref(), Some(&[".".to_owned()][..]));
    assert_eq!(
        settings.catalog.as_ref().and_then(|c| c.get("react")).map(String::as_str),
        Some("^19.0.0"),
    );
    assert_eq!(
        settings
            .catalogs
            .as_ref()
            .and_then(|c| c.get("react17"))
            .and_then(|c| c.get("react"))
            .map(String::as_str),
        Some("^17.0.0"),
    );
    assert_eq!(settings.only_built_dependencies.as_deref(), Some(&["esbuild".to_owned()][..]));
    assert_eq!(settings.never_built_dependencies.as_deref(), Some(&["fsevents".to_owned()][..]));
    assert_eq!(settings.ignored_built_dependencies.as_deref(), Some(&["core-js".to_owned()][..]));

    settings.clear_workspace_only_fields();
    assert_eq!(settings, WorkspaceSettings::default());
}

/// An empty `scope` is a value like any other — it would clear a scope the
/// global `config.yaml` set — so it is refused like a non-empty one, while a
/// file that names no scope reports nothing.
#[test]
fn an_empty_scope_is_refused_and_a_missing_one_is_not_reported() {
    let mut empty = WorkspaceSettings::default();
    empty.collect_key_issues("scope: ''\n");
    assert_eq!(empty.key_issues.refused, vec!["scope".to_owned()]);

    let mut absent = WorkspaceSettings::default();
    absent.collect_key_issues("registry: https://reg.example/\n");
    assert!(absent.key_issues.is_empty());
}

#[test]
fn apply_scope_overrides_an_earlier_layer() {
    let settings: WorkspaceSettings =
        serde_saphyr::from_str("scope: '@from-later-layer'\n").unwrap();
    let mut config = Config::new();
    config.scope = Some("@from-global-config".to_owned());
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.scope.as_deref(), Some("@from-later-layer"));
}

#[test]
fn apply_resolves_relative_paths_against_base_dir() {
    let yaml = "storeDir: ../shared-store\npnpmfile: hooks/../custom.cjs\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    let base = Path::new("/workspace/root");

    settings.apply_to(&mut config, base);

    // Build the expected path via the same join machinery the code
    // under test uses so the component separator matches on every
    // platform (Windows uses `\` between joined components).
    assert_eq!(config.store_dir, StoreDir::from(base.join("../shared-store")));
    assert_eq!(config.pnpmfile, Some(vec![base.join("custom.cjs")]));

    let settings: WorkspaceSettings =
        serde_saphyr::from_str("pnpmfile: [hooks/../custom.cjs, custom.cjs]\n").unwrap();
    settings.apply_to(&mut config, base);
    assert_eq!(config.pnpmfile, Some(vec![base.join("custom.cjs"), base.join("custom.cjs")]));
}

/// pnpm reads `fetchRetries` / `fetchRetryFactor` /
/// `fetchRetryMintimeout` / `fetchRetryMaxtimeout` from
/// `pnpm-workspace.yaml` as camelCase keys (mirrors of the kebab-case
/// `.npmrc` form). Confirm both deserialization and `apply_to` push
/// the overrides onto the `Config`, since pacquet has to honour them
/// for parity with pnpm and for the install-time retry plumbing in
/// crates/tarball.
#[test]
fn parses_fetch_retry_settings_from_yaml_and_applies() {
    let yaml = r"
fetchRetries: 5
fetchRetryFactor: 3
fetchRetryMintimeout: 1000
fetchRetryMaxtimeout: 4000
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.fetch_retries, Some(5));
    assert_eq!(settings.fetch_retry_factor, Some(3));
    assert_eq!(settings.fetch_retry_mintimeout, Some(1000));
    assert_eq!(settings.fetch_retry_maxtimeout, Some(4000));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.fetch_retries, 5);
    assert_eq!(config.fetch_retry_factor, 3);
    assert_eq!(config.fetch_retry_mintimeout, 1000);
    assert_eq!(config.fetch_retry_maxtimeout, 4000);
}

/// `ignoreScripts` parses from `pnpm-workspace.yaml` as a camelCase
/// key and `apply_to` pushes it onto [`Config::ignore_scripts`], so
/// `ignoreScripts: true` in the workspace manifest suppresses lifecycle
/// scripts the same way the `--ignore-scripts` CLI flag does.
#[test]
fn parses_ignore_scripts_from_yaml_and_applies() {
    let settings: WorkspaceSettings = serde_saphyr::from_str("ignoreScripts: true\n").unwrap();
    assert_eq!(settings.ignore_scripts, Some(true));

    let mut config = Config::new();
    assert!(!config.ignore_scripts, "default is false");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.ignore_scripts);
}

/// `gitChecks: false` parses from `pnpm-workspace.yaml` and `apply_to`
/// pushes it onto [`Config::git_checks`], so a user can disable the
/// publish git checks via config exactly as pnpm's own hint instructs.
#[test]
fn parses_git_checks_from_yaml_and_applies() {
    let settings: WorkspaceSettings = serde_saphyr::from_str("gitChecks: false\n").unwrap();
    assert_eq!(settings.git_checks, Some(false));

    let mut config = Config::new();
    assert!(config.git_checks, "default is true");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(!config.git_checks);
}

#[test]
fn parses_network_settings_from_yaml_and_applies() {
    let yaml = r"
networkConcurrency: 8
fetchTimeout: 120000
fetchWarnTimeoutMs: 20000
fetchMinSpeedKiBps: 100
userAgent: my-agent/2.0
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.network_concurrency, Some(8));
    assert_eq!(settings.fetch_timeout, Some(120_000));
    assert_eq!(settings.fetch_warn_timeout_ms, Some(20_000));
    assert_eq!(settings.fetch_min_speed_ki_bps, Some(100));
    assert_eq!(settings.user_agent.as_deref(), Some("my-agent/2.0"));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.network_concurrency, 8);
    assert_eq!(config.fetch_timeout, 120_000);
    assert_eq!(config.fetch_warn_timeout_ms, 20_000);
    assert_eq!(config.fetch_min_speed_ki_bps, 100);
    assert_eq!(config.user_agent, "my-agent/2.0");
}

/// `namedRegistries` is the deprecated spelling of a registry's `prefix`. The
/// deserializer reads the camelCase key it still carries, and `apply_to`
/// writes the map onto [`Config::registries_by_prefix`] verbatim.
#[test]
fn parses_named_registries_from_yaml_and_applies() {
    let yaml = r"
namedRegistries:
  gh: https://npm.pkg.ghes.example.com/
  work: https://npm.work.example.com/
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let named = settings.named_registries.as_ref().expect("namedRegistries present");
    assert_eq!(named.get("gh").map(String::as_str), Some("https://npm.pkg.ghes.example.com/"));
    assert_eq!(named.get("work").map(String::as_str), Some("https://npm.work.example.com/"));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config.registries_by_prefix.get("gh").map(String::as_str),
        Some("https://npm.pkg.ghes.example.com/"),
    );
    assert_eq!(
        config.registries_by_prefix.get("work").map(String::as_str),
        Some("https://npm.work.example.com/"),
    );
}

#[test]
fn parses_registries_from_yaml_and_applies() {
    let yaml = r"
registries:
  default: https://default.example.com/npm
  '@private': https://private.example.com/npm
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let registries = settings.registries.as_ref().expect("registries present");
    assert_eq!(
        registries.get("default"),
        Some(&RegistryEntry::ScopeRoute("https://default.example.com/npm".to_owned())),
    );
    assert_eq!(
        registries.get("@private"),
        Some(&RegistryEntry::ScopeRoute("https://private.example.com/npm".to_owned())),
    );

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.registry, "https://default.example.com/npm/");
    assert_eq!(
        config.registries_by_scope.get("@private").map(String::as_str),
        Some("https://private.example.com/npm/"),
    );
}

/// Env-var placeholders inside workspace request destinations are ignored so
/// repository-controlled config cannot smuggle victim environment
/// values into outbound requests.
#[test]
fn ignores_env_vars_inside_workspace_request_destination_values() {
    struct EnvWithHost;
    impl EnvVar for EnvWithHost {
        fn var(name: &str) -> Option<String> {
            (name == "WORK_HOST").then(|| "internal.example.com".to_owned())
        }
    }

    let yaml = r"
pnprServer: https://${WORK_HOST}/pnpr/
registry: https://${WORK_HOST}/npm/
httpsProxy: http://${WORK_HOST}:8080/
httpProxy: http://${WORK_HOST}:8081/
noProxy: ${WORK_HOST}
proxy: http://${WORK_HOST}:8082/
noproxy: ${WORK_HOST}
registries:
  '@safe': https://safe.example.com/npm/
  '@work': https://${WORK_HOST}/scope/
namedRegistries:
  literal: 'https://registry.example.com/${/npm/'
  stable: https://registry.example.com/npm/
  work: https://${WORK_HOST}/npm/
";
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    settings.substitute_env_untrusted::<EnvWithHost>();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.pnpr_server, None);
    assert_eq!(config.registry, "https://registry.npmjs.org/");
    assert_eq!(config.proxy, pnpm_network::ProxyConfig::default());
    assert_eq!(
        config.registries_by_scope.get("@safe").map(String::as_str),
        Some("https://safe.example.com/npm/"),
    );
    assert_eq!(config.registries_by_scope.get("@work"), None);
    assert_eq!(
        config.registries_by_prefix.get("stable").map(String::as_str),
        Some("https://registry.example.com/npm/"),
    );
    assert_eq!(
        config.registries_by_prefix.get("literal").map(String::as_str),
        Some("https://registry.example.com/${/npm/"),
    );
    assert_eq!(config.registries_by_prefix.get("work"), None);
}

#[test]
fn expands_env_vars_inside_non_registry_workspace_values() {
    struct EnvWithPaths;
    impl EnvVar for EnvWithPaths {
        fn var(name: &str) -> Option<String> {
            match name {
                "CACHE_DIR" => Some("cache-dir".to_owned()),
                "HOOK" => Some("hook.js".to_owned()),
                "SHELL" => Some("custom-shell".to_owned()),
                "STORE_DIR" => Some("store-dir".to_owned()),
                "USER_AGENT" => Some("pacquet-test/1.0".to_owned()),
                _ => None,
            }
        }
    }

    let yaml = r"
storeDir: ${STORE_DIR}
cacheDir: ${CACHE_DIR}
scriptShell: ${SHELL}
nodeOptions: --require=${HOOK}
userAgent: ${USER_AGENT}
";
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    settings.substitute_env_untrusted::<EnvWithPaths>();

    let base = Path::new("/workspace/root");
    let mut config = Config::new();
    settings.apply_to(&mut config, base);

    assert_eq!(config.store_dir, StoreDir::from(base.join("store-dir")));
    assert_eq!(config.cache_dir, base.join("cache-dir"));
    assert_eq!(config.script_shell.as_deref(), Some("custom-shell"));
    assert_eq!(config.node_options.as_deref(), Some("--require=hook.js"));
    assert_eq!(config.user_agent, "pacquet-test/1.0");
}

#[test]
fn keeps_non_ascii_text_in_workspace_values() {
    struct EnvWithPaths;
    impl EnvVar for EnvWithPaths {
        fn var(name: &str) -> Option<String> {
            match name {
                "CACHE_DIR" => Some("cache-dir".to_owned()),
                "STORE_DIR" => Some("store-dir".to_owned()),
                _ => None,
            }
        }
    }

    let yaml = r"
storeDir: ${STORE_DIR}/café
cacheDir: 日本語/${CACHE_DIR}
scriptShell: ./ünicode-shell
";
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    settings.substitute_env_untrusted::<EnvWithPaths>();

    let base = Path::new("/workspace/root");
    let mut config = Config::new();
    settings.apply_to(&mut config, base);

    assert_eq!(config.store_dir, StoreDir::from(base.join("store-dir/café")));
    assert_eq!(config.cache_dir, base.join("日本語/cache-dir"));
    assert_eq!(config.script_shell.as_deref(), Some("./ünicode-shell"));
}

#[test]
fn trusted_settings_expand_env_vars_inside_request_destination_values() {
    struct EnvWithHost;
    impl EnvVar for EnvWithHost {
        fn var(name: &str) -> Option<String> {
            (name == "WORK_HOST").then(|| "internal.example.com".to_owned())
        }
    }

    let yaml = r"
pnprServer: https://${WORK_HOST}/pnpr/
registry: https://${WORK_HOST}/npm/
httpsProxy: http://${WORK_HOST}:8080/
httpProxy: http://${WORK_HOST}:8081/
noProxy: ${WORK_HOST}
namedRegistries:
  stable: https://registry.example.com/npm/
  work: https://${WORK_HOST}/work/
";
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    settings.substitute_env_trusted::<EnvWithHost>();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.pnpr_server.as_deref(), Some("https://internal.example.com/pnpr/"));
    assert_eq!(config.registry, "https://internal.example.com/npm/");
    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://internal.example.com:8080/"));
    assert_eq!(config.proxy.http_proxy.as_deref(), Some("http://internal.example.com:8081/"));
    assert_eq!(
        config.proxy.no_proxy,
        Some(pnpm_network::NoProxySetting::List(vec!["internal.example.com".to_string()])),
    );
    assert_eq!(
        config.registries_by_prefix.get("stable").map(String::as_str),
        Some("https://registry.example.com/npm/"),
    );
    assert_eq!(
        config.registries_by_prefix.get("work").map(String::as_str),
        Some("https://internal.example.com/work/"),
    );
}

/// `verifyStoreIntegrity` is a camelCase key that serde's rename
/// has to pick up, and the `apply_to` wiring has to thread it onto
/// the `Config` field. Parse a yaml that flips the default-true
/// setting to false and assert both steps. Guards against silent
/// regressions in the key mapping or the apply step (a copy-paste
/// omission in `apply_to` would leave `config.verify_store_integrity`
/// at its default).
#[test]
fn parses_verify_store_integrity_from_yaml_and_applies() {
    let yaml = "verifyStoreIntegrity: false\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.verify_store_integrity, Some(false));

    let mut config = Config::new();
    assert!(config.verify_store_integrity, "the default is `true` to match pnpm");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(!config.verify_store_integrity, "yaml override wins");
}

/// `strictStorePkgContentCheck` decides whether a store row that holds
/// another package fails the install. Same camelCase rename +
/// `apply_to` wiring as `verifyStoreIntegrity`, and the same
/// default-true polarity.
#[test]
fn parses_strict_store_pkg_content_check_from_yaml_and_applies() {
    let yaml = "strictStorePkgContentCheck: false\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.strict_store_pkg_content_check, Some(false));

    let mut config = Config::new();
    assert!(config.strict_store_pkg_content_check, "the default is `true` to match pnpm");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(!config.strict_store_pkg_content_check, "yaml override wins");
}

/// `includeWorkspaceRoot` keeps the workspace root in a recursive
/// selection. Default `false`, so the yaml has to flip it on.
#[test]
fn parses_include_workspace_root_from_yaml_and_applies() {
    let yaml = "includeWorkspaceRoot: true\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.include_workspace_root, Some(true));

    let mut config = Config::new();
    assert!(!config.include_workspace_root, "the default is `false` to match pnpm");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.include_workspace_root, "yaml override wins");
}

/// The two workspace-cycle knobs are independent keys — one silences the
/// report, the other promotes it to an error — so a file setting both is
/// applied to both fields.
#[test]
fn parses_the_workspace_cycle_settings_from_yaml_and_applies() {
    let yaml = "ignoreWorkspaceCycles: true\ndisallowWorkspaceCycles: true\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.ignore_workspace_cycles, Some(true));
    assert_eq!(settings.disallow_workspace_cycles, Some(true));

    let mut config = Config::new();
    assert!(!config.ignore_workspace_cycles, "the default is `false` to match pnpm");
    assert!(!config.disallow_workspace_cycles, "the default is `false` to match pnpm");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.ignore_workspace_cycles, "yaml override wins");
    assert!(config.disallow_workspace_cycles, "yaml override wins");
}

/// `sideEffectsCache` is the side-effects cache READ-path knob from
/// pnpm-workspace.yaml. Same shape as `verifyStoreIntegrity`:
/// camelCase rename + `apply_to` wiring. Parsing a yaml that flips
/// the default-true setting to false must end up at
/// `config.side_effects_cache == false`.
#[test]
fn parses_side_effects_cache_from_yaml_and_applies() {
    let yaml = "sideEffectsCache: false\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.side_effects_cache, Some(SideEffectsCacheSetting::Enabled(false)));

    let mut config = Config::new();
    assert!(config.side_effects_cache, "the default is `true` to match pnpm");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(!config.side_effects_cache, "yaml override wins");
}

/// `sideEffectsCacheReadonly` is pnpm's read-only flag for the
/// side-effects cache. Same camelCase + `apply_to` wiring as
/// `sideEffectsCache`. Default is `false`, so flipping it on via
/// yaml must end at `config.side_effects_cache_readonly == true`.
#[test]
fn parses_side_effects_cache_readonly_from_yaml_and_applies() {
    let yaml = "sideEffectsCacheReadonly: true\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.side_effects_cache_readonly, Some(true));

    let mut config = Config::new();
    assert!(!config.side_effects_cache_readonly, "the default is `false`");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.side_effects_cache_readonly, "yaml override wins");
}

/// READ / WRITE gate helpers must combine the two knobs for the
/// canonical state combinations:
///
/// - default (`cache=true`, `readonly=false`)  → read=on, write=on
/// - cache off  (`cache=false`, `readonly=false`) → read=off, write=off
/// - readonly on (`cache=true`, `readonly=true`)  → read=on, write=off
/// - cache off + readonly on                      → read=on, write=off
#[test]
fn side_effects_cache_gates_truth_table() {
    let mut config = Config::new();
    assert!(config.side_effects_cache_read());
    assert!(config.side_effects_cache_write());

    config.side_effects_cache = false;
    config.side_effects_cache_readonly = false;
    assert!(!config.side_effects_cache_read());
    assert!(!config.side_effects_cache_write());

    config.side_effects_cache = true;
    config.side_effects_cache_readonly = true;
    assert!(config.side_effects_cache_read());
    assert!(!config.side_effects_cache_write());

    config.side_effects_cache = false;
    config.side_effects_cache_readonly = true;
    assert!(config.side_effects_cache_read());
    assert!(!config.side_effects_cache_write());
}

/// `patchedDependencies` in `pnpm-workspace.yaml` is a string→string
/// map where keys carry an optional `@version` suffix and values are
/// patch-file paths. pacquet captures it raw on `WorkspaceSettings`;
/// path resolution + hashing + grouping happen at install time via
/// `Config::resolved_patched_dependencies` (which delegates to
/// `pnpm_patching::resolve_and_group`). This test guards the
/// deserialization shape only — the camelCase rename, optionality,
/// and value-as-string-path.
#[test]
fn parses_patched_dependencies_from_yaml() {
    let yaml = r#"
patchedDependencies:
  "lodash@4.17.21": patches/lodash@4.17.21.patch
  "foo@^1.0.0": patches/foo.patch
  bar: patches/bar.patch
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let map = settings.patched_dependencies.expect("field present");
    assert_eq!(map.get("lodash@4.17.21").map(String::as_str), Some("patches/lodash@4.17.21.patch"));
    assert_eq!(map.get("foo@^1.0.0").map(String::as_str), Some("patches/foo.patch"));
    assert_eq!(map.get("bar").map(String::as_str), Some("patches/bar.patch"));
}

#[test]
fn patched_dependencies_absent_yields_none() {
    let yaml = "storeDir: /s\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert!(settings.patched_dependencies.is_none());
}

/// `apply_to` records the workspace dir on `Config.workspace_dir`
/// (needed by `Config::resolved_patched_dependencies` so patch
/// file paths resolve against the same dir as upstream) and pushes
/// the raw map verbatim.
#[test]
fn apply_pushes_patched_dependencies_and_workspace_dir() {
    let yaml = r#"
patchedDependencies:
  "lodash@4.17.21": patches/lodash@4.17.21.patch
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    let base = Path::new("/workspace/root");
    settings.apply_to(&mut config, base);

    assert_eq!(config.workspace_dir.as_deref(), Some(base));
    let map = config.patched_dependencies.expect("present");
    assert_eq!(map.get("lodash@4.17.21").map(String::as_str), Some("patches/lodash@4.17.21.patch"));
}

#[test]
fn patches_dir_reads_from_workspace_yaml() {
    let settings: WorkspaceSettings =
        serde_saphyr::from_str("patchesDir: custom-patches\n").unwrap();
    assert_eq!(settings.patches_dir.as_deref(), Some("custom-patches"));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/workspace/root"));
    assert_eq!(config.patches_dir.as_deref(), Some("custom-patches"));
}

/// `configDependencies` is a map of package name → version-with-integrity
/// spec. pacquet records it into the workspace-state file so pnpm's
/// `checkDepsStatus` doesn't treat the install as stale on the next
/// `pnpm run` / `pnpm node`. Guards the camelCase rename, optionality,
/// and `apply_to` wiring.
#[test]
fn parses_config_dependencies_from_yaml_and_applies() {
    let yaml = r#"
configDependencies:
  "@pnpm/pacquet": 0.2.2-14
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let expected = settings.config_dependencies.clone();
    assert_eq!(
        expected.as_ref().and_then(|m| m.get("@pnpm/pacquet")),
        Some(&ConfigDependency::VersionWithIntegrity("0.2.2-14".to_string())),
    );

    let mut config = Config::new();
    assert!(config.config_dependencies.is_none(), "default is None");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.config_dependencies, expected);
}

/// pnpm's `configDependencies` value can also be the `{ tarball?, integrity }`
/// object form. It must parse (not error) and round-trip, otherwise an
/// upstream-supported manifest becomes a hard config-load failure.
#[test]
fn parses_object_form_config_dependencies() {
    let yaml = r#"
configDependencies:
  "@scope/dep":
    integrity: sha512-abc
    tarball: https://example.test/dep.tgz
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let map = settings.config_dependencies.expect("field present");
    assert_eq!(
        map.get("@scope/dep"),
        Some(&ConfigDependency::Detailed(ConfigDependencyDetail {
            integrity: "sha512-abc".to_string(),
            tarball: Some("https://example.test/dep.tgz".to_string()),
        })),
    );
}

/// Port of upstream's `respects testPattern` / `respects
/// changedFilesIgnorePattern` config tests: both settings come from
/// `pnpm-workspace.yaml` and default to unset (pacquet: an empty list).
#[test]
fn parses_test_pattern_and_changed_files_ignore_pattern_from_yaml_and_applies() {
    let yaml = r"
testPattern:
  - '*.spec.js'
  - '*.spec.ts'
changedFilesIgnorePattern:
  - .github/**
  - '**/README.md'
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    assert!(config.test_pattern.is_empty());
    assert!(config.changed_files_ignore_pattern.is_empty());

    settings.apply_to(&mut config, Path::new("/irrelevant"));

    assert_eq!(config.test_pattern, ["*.spec.js", "*.spec.ts"]);
    assert_eq!(config.changed_files_ignore_pattern, [".github/**", "**/README.md"]);
}

/// `testPattern` / `changedFilesIgnorePattern` cannot be set from the
/// global `config.yaml` — pnpm lists both in its excluded keys.
#[test]
fn test_pattern_and_changed_files_ignore_pattern_cleared_as_workspace_only_fields() {
    let yaml = r"
testPattern:
  - '*.spec.js'
changedFilesIgnorePattern:
  - '**/README.md'
";
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    settings.clear_workspace_only_fields();
    assert!(settings.test_pattern.is_none());
    assert!(settings.changed_files_ignore_pattern.is_none());
}

/// `versioning` is workspace-only: release plans must not be shaped by a
/// global `config.yaml`.
#[test]
fn versioning_cleared_as_workspace_only_field() {
    let yaml = r#"
versioning:
  lanes:
    "@example/cli": alpha
"#;
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert!(settings.versioning.is_some());
    settings.clear_workspace_only_fields();
    assert!(settings.versioning.is_none());
}

/// `configDependencies` is workspace-only: it must not be honored from
/// the global `config.yaml`, matching pnpm's `isConfigFileKey` filter.
#[test]
fn config_dependencies_cleared_as_workspace_only_field() {
    let yaml = r#"
deployAllFiles: true
forceLegacyDeploy: true
sharedWorkspaceLockfile: false
configDependencies:
  "@pnpm/pacquet": 0.2.2-14
"#;
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    settings.clear_workspace_only_fields();
    assert!(settings.deploy_all_files.is_none());
    assert!(settings.force_legacy_deploy.is_none());
    assert!(settings.shared_workspace_lockfile.is_none());
    assert!(settings.config_dependencies.is_none());
}

/// `allowBuilds` is a map of `name[@version]` → bool. Same camelCase
/// rename + `apply_to` wiring as the other yaml-sourced settings.
/// pnpm 10+ moved this out of `package.json#pnpm` (matches
/// pnpm/pacquet#397 item 5).
#[test]
fn parses_allow_builds_from_yaml_and_applies() {
    let yaml = r#"
allowBuilds:
  esbuild: true
  "foo@1.0.0": true
  bar: false
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let raw = settings.allow_builds.clone().expect("field present");
    assert_eq!(raw.get("esbuild").and_then(AllowBuild::decided), Some(true));
    assert_eq!(raw.get("foo@1.0.0").and_then(AllowBuild::decided), Some(true));
    assert_eq!(raw.get("bar").and_then(AllowBuild::decided), Some(false));

    let mut config = Config::new();
    assert!(config.allow_builds.is_empty(), "default is empty");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.allow_builds.get("esbuild").copied(), Some(true));
}

#[test]
fn parses_remote_side_effects_cache_from_yaml_and_applies() {
    let settings: WorkspaceSettings = serde_saphyr::from_str(
        r"
remoteSideEffectsCache:
  organization: acme
  packages:
    - native-addon
",
    )
    .unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/workspace"));

    let shared = config.remote_side_effects_cache.expect("shared cache config");
    assert_eq!(shared.org, "acme");
    assert_eq!(shared.packages, ["native-addon"]);
}

/// A boolean says whether to read and write. It says nothing about the remote
/// tier, so one an earlier layer declared has to survive it — otherwise
/// `sideEffectsCache: false` in a project silently discards the org and
/// eligibility list the machine's global config set.
#[test]
fn a_later_shorthand_keeps_the_remote_tier() {
    let global: WorkspaceSettings = serde_saphyr::from_str(
        r"
sideEffectsCache:
  remote:
    org: acme
    packages:
      - native-addon
",
    )
    .unwrap();
    let workspace: WorkspaceSettings = serde_saphyr::from_str("sideEffectsCache: false").unwrap();

    let mut config = Config::new();
    global.apply_to(&mut config, Path::new("/global"));
    workspace.apply_to(&mut config, Path::new("/workspace"));

    assert!(!config.side_effects_cache_read());
    assert!(!config.side_effects_cache_write());
    let shared = config.remote_side_effects_cache.expect("shared cache config");
    assert_eq!(shared.org, "acme");
    assert_eq!(shared.packages, ["native-addon"]);
}

/// Retaining a remote tier across a boolean must not change what the boolean
/// and `sideEffectsCacheReadonly` mean together: the read-only pair reads,
/// whether or not a remote tier was declared earlier.
#[test]
fn a_retained_remote_tier_does_not_change_the_read_only_pair() {
    let global: WorkspaceSettings = serde_saphyr::from_str(
        r"
sideEffectsCache:
  remote:
    org: acme
",
    )
    .unwrap();
    let workspace: WorkspaceSettings = serde_saphyr::from_str(
        r"
sideEffectsCache: false
sideEffectsCacheReadonly: true
",
    )
    .unwrap();

    let mut config = Config::new();
    global.apply_to(&mut config, Path::new("/global"));
    workspace.apply_to(&mut config, Path::new("/workspace"));

    assert!(config.side_effects_cache_read(), "the read-only pair still reads");
    assert!(!config.side_effects_cache_write());
    assert_eq!(config.remote_side_effects_cache.expect("shared cache config").org, "acme");
}

/// A file may carry both spellings of the field; `org` wins, and neither is a
/// parse error the way a serde alias would have made them.
#[test]
fn the_canonical_org_wins_over_the_alternative_spelling() {
    let settings: WorkspaceSettings = serde_saphyr::from_str(
        r"
sideEffectsCache:
  remote:
    org: canonical
    organization: alternative
",
    )
    .unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/workspace"));

    assert_eq!(config.remote_side_effects_cache.expect("shared cache config").org, "canonical");
}

/// Layers apply in order, so a shorthand in a later one has to beat an object
/// in an earlier one rather than being masked by what the object left behind.
#[test]
fn a_later_shorthand_overrides_an_earlier_object() {
    let global: WorkspaceSettings = serde_saphyr::from_str(
        r"
sideEffectsCache:
  read: true
  write: true
",
    )
    .unwrap();
    let workspace: WorkspaceSettings = serde_saphyr::from_str("sideEffectsCache: false").unwrap();

    let mut config = Config::new();
    global.apply_to(&mut config, Path::new("/global"));
    workspace.apply_to(&mut config, Path::new("/workspace"));

    assert!(!config.side_effects_cache_read());
    assert!(!config.side_effects_cache_write());
}

/// `organization` shipped in pacquet 12.0.0, so a file written for it keeps
/// working; `org` is what pnpr calls the same namespace.
#[test]
fn accepts_the_older_organization_spelling() {
    let settings: WorkspaceSettings = serde_saphyr::from_str(
        r"
sideEffectsCache:
  remote:
    organization: acme
",
    )
    .unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/workspace"));

    let shared = config.remote_side_effects_cache.expect("shared cache config");
    assert_eq!(shared.org, "acme");
}

#[test]
fn parses_the_canonical_side_effects_cache_declaration() {
    let settings: WorkspaceSettings = serde_saphyr::from_str(
        r"
sideEffectsCache:
  read: true
  write: false
  remote:
    organization: acme
    packages:
      - native-addon
",
    )
    .unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/workspace"));

    assert!(config.side_effects_cache_read());
    assert!(!config.side_effects_cache_write());
    let shared = config.remote_side_effects_cache.expect("shared cache config");
    assert_eq!(shared.org, "acme");
    assert_eq!(shared.packages, ["native-addon"]);
}

/// Naming the remote tier says nothing about the local one, which was on by
/// default before this setting grew an object form.
#[test]
fn declaring_only_the_remote_tier_leaves_the_local_one_on() {
    let settings: WorkspaceSettings = serde_saphyr::from_str(
        r"
sideEffectsCache:
  remote:
    organization: acme
",
    )
    .unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/workspace"));

    assert!(config.side_effects_cache_read());
    assert!(config.side_effects_cache_write());
}

#[test]
fn the_boolean_shorthand_still_reads_and_writes() {
    let settings: WorkspaceSettings = serde_saphyr::from_str("sideEffectsCache: false").unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/workspace"));

    assert!(!config.side_effects_cache_read());
    assert!(!config.side_effects_cache_write());
}

/// The two spellings of the remote tier compose rather than replace, so a
/// repository may name the packages under one and the organization under the
/// other without either dropping the other's fields.
#[test]
fn the_canonical_declaration_wins_over_the_older_spellings() {
    let settings: WorkspaceSettings = serde_saphyr::from_str(
        r"
sideEffectsCacheReadonly: true
remoteSideEffectsCache:
  packages:
    - from-the-old-key
sideEffectsCache:
  read: false
  write: true
  remote:
    organization: acme
",
    )
    .unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/workspace"));

    assert!(!config.side_effects_cache_read());
    assert!(config.side_effects_cache_write());
    let shared = config.remote_side_effects_cache.expect("shared cache config");
    assert_eq!(shared.org, "acme");
    assert_eq!(shared.packages, ["from-the-old-key"]);
}

/// The trust boundary follows the fields rather than the spelling, so the
/// canonical form must refuse everything the older one does — and the message
/// has to name the key the file actually wrote.
#[test]
fn rejects_workspace_controlled_trust_material_under_the_canonical_spelling() {
    for (trust_material, field) in [
        ("trustedKeys:\n      acme-2026: repository-controlled-key", "trustedKeys"),
        ("privateKey: repository-controlled-key", "privateKey"),
        ("publish: true", "publish"),
        ("keyId: acme-2026", "keyId"),
        ("builderId: ci/main/42", "builderId"),
        ("imageDigest: sha256:abc", "imageDigest"),
        ("architectureBaseline: x64", "architectureBaseline"),
        ("buildEnv:\n      CC: clang", "buildEnv"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(WORKSPACE_MANIFEST_FILENAME),
            format!("sideEffectsCache:\n  remote:\n    org: acme\n    {trust_material}\n"),
        )
        .unwrap();

        let error = WorkspaceSettings::load_at(dir.path()).unwrap_err().to_string();
        assert!(error.contains(&format!("sideEffectsCache.remote.{field}")), "{error}");
    }
}

/// A repository that could set `publish` would turn a key the machine holds for
/// its own builds into a signing oracle, so every field but the two that
/// declare eligibility is refused.
#[test]
fn rejects_workspace_controlled_shared_side_effects_trust_material() {
    for (trust_material, field) in [
        ("trustedKeys:\n    acme-2026: repository-controlled-key", "trustedKeys"),
        ("privateKey: repository-controlled-key", "privateKey"),
        ("publish: true", "publish"),
        ("keyId: acme-2026", "keyId"),
        ("builderId: ci/main/42", "builderId"),
        ("imageDigest: sha256:abc", "imageDigest"),
        ("architectureBaseline: x64", "architectureBaseline"),
        ("buildEnv:\n    CC: clang", "buildEnv"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(WORKSPACE_MANIFEST_FILENAME),
            format!(
                "remoteSideEffectsCache:\n  organization: acme\n  packages:\n    - native-addon\n  {trust_material}\n",
            ),
        )
        .unwrap();

        let error = WorkspaceSettings::load_at(dir.path()).unwrap_err();
        assert!(error.to_string().contains(field), "{error}");
    }
}

/// The machine keeps the publication switch a repository may not touch.
#[test]
fn a_workspace_declaring_eligibility_keeps_the_machines_publication_settings() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "remoteSideEffectsCache:\n  organization: acme\n  packages:\n    - native-addon\n",
    )
    .unwrap();
    let global: WorkspaceSettings = serde_saphyr::from_str(
        r"
remoteSideEffectsCache:
  publish: true
  keyId: acme-2026
",
    )
    .unwrap();

    let mut config = Config::new();
    global.apply_to(&mut config, Path::new("/workspace"));
    WorkspaceSettings::load_at(dir.path()).unwrap().unwrap().apply_to(&mut config, dir.path());

    let shared = config.remote_side_effects_cache.expect("shared cache config");
    assert_eq!(shared.org, "acme");
    assert_eq!(shared.packages, ["native-addon"]);
    assert_eq!(shared.publish, Some(true));
    assert_eq!(shared.key_id.as_deref(), Some("acme-2026"));
}

/// The variables are named for the setting they configure, and the names that
/// matched the older spelling keep working — a machine set up before the rename
/// is not something a `pnpm install` should start ignoring.
///
/// Every suffix is covered because they do not share a parsing path: `PUBLISH`
/// is a boolean, `BUILD_ENV` and `TRUSTED_KEYS` are JSON, the rest are strings.
#[test]
fn the_remote_tier_reads_both_environment_spellings() {
    const CANONICAL: &str = "PNPM_SIDE_EFFECTS_CACHE_REMOTE_";
    const OLDER: &str = "PNPM_REMOTE_SIDE_EFFECTS_CACHE_";

    fn read(prefixes: &[&'static str], suffix: &'static str, value: &'static str) -> Config {
        // A thread-local rather than a generic per case: `EnvVar` is a trait
        // with an associated function, so the case has to reach it somehow.
        CASE.with(|case| *case.borrow_mut() = Some((prefixes.to_vec(), suffix, value)));
        struct Env;
        impl crate::EnvVar for Env {
            fn var(key: &str) -> Option<String> {
                CASE.with(|case| {
                    let borrowed = case.borrow();
                    let (prefixes, suffix, value) = borrowed.as_ref()?;
                    prefixes
                        .iter()
                        .any(|prefix| key == format!("{prefix}{suffix}"))
                        .then(|| (*value).to_string())
                })
            }
        }
        let mut config = Config::new();
        config.apply_remote_side_effects_cache_env::<Env>();
        config
    }

    for suffix in ["KEY_ID", "BUILDER_ID", "IMAGE_DIGEST", "ARCHITECTURE_BASELINE", "PRIVATE_KEY"] {
        for prefixes in [vec![CANONICAL], vec![OLDER], vec![CANONICAL, OLDER]] {
            let config = read(&prefixes, suffix, "value");
            let shared = config.remote_side_effects_cache.expect("shared cache config");
            let read_back = match suffix {
                "KEY_ID" => shared.key_id,
                "BUILDER_ID" => shared.builder_id,
                "IMAGE_DIGEST" => shared.image_digest,
                "ARCHITECTURE_BASELINE" => shared.architecture_baseline,
                _ => shared.private_key,
            };
            assert_eq!(read_back.as_deref(), Some("value"), "{suffix} under {prefixes:?}");
        }
    }

    for prefixes in [vec![CANONICAL], vec![OLDER], vec![CANONICAL, OLDER]] {
        let config = read(&prefixes, "PUBLISH", "true");
        assert_eq!(
            config.remote_side_effects_cache.expect("shared cache config").publish,
            Some(true),
            "PUBLISH under {prefixes:?}",
        );

        let config = read(&prefixes, "BUILD_ENV", r#"{"CC":"clang"}"#);
        let shared = config.remote_side_effects_cache.expect("shared cache config");
        assert_eq!(
            shared.build_env.expect("build env").get("CC").map(String::as_str),
            Some("clang"),
            "BUILD_ENV under {prefixes:?}",
        );

        let config = read(&prefixes, "TRUSTED_KEYS", r#"{"acme-2026":"AA=="}"#);
        let shared = config.remote_side_effects_cache.expect("shared cache config");
        assert_eq!(
            shared.trusted_keys.expect("trusted keys").get("acme-2026").map(String::as_str),
            Some("AA=="),
            "TRUSTED_KEYS under {prefixes:?}",
        );
    }
}

/// A malformed JSON variable is reported by name, so it has to be the name the
/// reader actually set — pointing at the spelling they did not use sends them
/// looking for a variable that is not in their environment.
#[test]
fn malformed_json_names_the_environment_variable_that_was_set() {
    struct Older;
    impl crate::EnvVar for Older {
        fn var(key: &str) -> Option<String> {
            (key == "PNPM_REMOTE_SIDE_EFFECTS_CACHE_TRUSTED_KEYS").then(|| "not json".to_string())
        }
    }

    let warnings = crate::tests::capture_warnings(|| {
        let mut config = Config::new();
        config.apply_remote_side_effects_cache_env::<Older>();
    });

    let warning = warnings
        .iter()
        .find(|warning| warning.contains("not a string-valued JSON object"))
        .expect("a warning about the malformed variable");
    assert!(
        warning.contains("PNPM_REMOTE_SIDE_EFFECTS_CACHE_TRUSTED_KEYS"),
        "expected the variable that was set, got {warning}",
    );
}

/// Precedence is by presence, not by validity: a malformed value under the name
/// matching the setting is not quietly replaced by a valid one under the older
/// name, because that would use a variable the reader did not reach for and
/// leave the broken one unreported.
#[test]
fn a_malformed_canonical_value_is_not_replaced_by_a_valid_older_one() {
    struct Both;
    impl crate::EnvVar for Both {
        fn var(key: &str) -> Option<String> {
            match key {
                "PNPM_SIDE_EFFECTS_CACHE_REMOTE_TRUSTED_KEYS" => Some("not json".to_string()),
                "PNPM_REMOTE_SIDE_EFFECTS_CACHE_TRUSTED_KEYS" => {
                    Some(r#"{"acme-2026":"AA=="}"#.to_string())
                }
                _ => None,
            }
        }
    }

    let mut config = Config::new();
    let warnings = crate::tests::capture_warnings(|| {
        config.apply_remote_side_effects_cache_env::<Both>();
    });

    let warning = warnings
        .iter()
        .find(|warning| warning.contains("not a string-valued JSON object"))
        .expect("a warning about the malformed variable");
    assert!(
        warning.contains("PNPM_SIDE_EFFECTS_CACHE_REMOTE_TRUSTED_KEYS"),
        "expected the variable that was selected, got {warning}",
    );
    assert!(
        config.remote_side_effects_cache.is_none_or(|shared| shared.trusted_keys.is_none()),
        "the older variable's value must not stand in for the malformed one",
    );
}

/// When both spellings are set, the one matching the setting decides.
#[test]
fn the_canonical_environment_spelling_wins() {
    struct Env;
    impl crate::EnvVar for Env {
        fn var(key: &str) -> Option<String> {
            match key {
                "PNPM_SIDE_EFFECTS_CACHE_REMOTE_KEY_ID" => Some("canonical".to_string()),
                "PNPM_REMOTE_SIDE_EFFECTS_CACHE_KEY_ID" => Some("older".to_string()),
                _ => None,
            }
        }
    }

    let mut config = Config::new();
    config.apply_remote_side_effects_cache_env::<Env>();
    assert_eq!(
        config.remote_side_effects_cache.expect("shared cache config").key_id.as_deref(),
        Some("canonical"),
    );
}

thread_local! {
    static CASE: std::cell::RefCell<Option<(Vec<&'static str>, &'static str, &'static str)>> =
        const { std::cell::RefCell::new(None) };
}

/// The environment holds the signing material a CI runner must not commit, so
/// it is the last word on the section.
#[test]
fn remote_side_effects_cache_environment_overrides_the_files() {
    struct Env;
    impl crate::EnvVar for Env {
        fn var(key: &str) -> Option<String> {
            match key {
                "PNPM_REMOTE_SIDE_EFFECTS_CACHE_PUBLISH" => Some("true".to_string()),
                "PNPM_REMOTE_SIDE_EFFECTS_CACHE_KEY_ID" => Some("acme-2026".to_string()),
                "PNPM_REMOTE_SIDE_EFFECTS_CACHE_TRUSTED_KEYS" => {
                    Some(r#"{"acme-2026":"AA=="}"#.to_string())
                }
                _ => None,
            }
        }
    }

    let settings: WorkspaceSettings = serde_saphyr::from_str(
        r"
remoteSideEffectsCache:
  organization: acme
  packages:
    - native-addon
  keyId: from-the-file
",
    )
    .unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/workspace"));
    config.apply_remote_side_effects_cache_env::<Env>();

    let shared = config.remote_side_effects_cache.expect("shared cache config");
    assert_eq!(shared.org, "acme");
    assert_eq!(shared.packages, ["native-addon"]);
    assert_eq!(shared.publish, Some(true));
    assert_eq!(shared.key_id.as_deref(), Some("acme-2026"));
    assert_eq!(shared.trusted_keys.expect("trusted keys").get("acme-2026").unwrap(), "AA==");
}

/// Each source contributes the half it owns, and the later one keeps what the
/// earlier one set.
#[test]
fn remote_side_effects_cache_sources_overlay_rather_than_replace() {
    let global: WorkspaceSettings = serde_saphyr::from_str(
        r"
remoteSideEffectsCache:
  trustedKeys:
    acme-2026: AA==
  privateKey: BB==
",
    )
    .unwrap();
    let workspace: WorkspaceSettings = serde_saphyr::from_str(
        r"
remoteSideEffectsCache:
  organization: acme
  packages:
    - native-addon
",
    )
    .unwrap();
    let mut config = Config::new();
    global.apply_to(&mut config, Path::new("/workspace"));
    workspace.apply_to(&mut config, Path::new("/workspace"));

    let shared = config.remote_side_effects_cache.expect("shared cache config");
    assert_eq!(shared.org, "acme");
    assert_eq!(shared.packages, ["native-addon"]);
    assert_eq!(shared.trusted_keys.expect("trusted keys").get("acme-2026").unwrap(), "AA==");
    assert_eq!(shared.private_key.as_deref(), Some("BB=="));
}

/// pnpm scaffolds `allowBuilds` entries with a placeholder string for the
/// user to replace. The file pnpm wrote must stay loadable, and the
/// undecided package must stay under the default-deny policy rather than
/// becoming an explicit `false` (which `pnpm ignored-builds` would then
/// report as explicitly ignored).
#[test]
fn accepts_placeholder_strings_in_allow_builds() {
    let yaml = r"
allowBuilds:
  esbuild: set this to true or false
  sharp: true
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let raw = settings.allow_builds.clone().expect("field present");
    assert_eq!(
        raw.get("esbuild"),
        Some(&AllowBuild::Undecided("set this to true or false".to_string())),
    );

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.allow_builds.get("sharp").copied(), Some(true));
    assert_eq!(config.allow_builds.get("esbuild").copied(), None);
}

/// `dangerouslyAllowAllBuilds` is a single boolean — default `false`
/// to match pnpm 11.
#[test]
fn parses_dangerously_allow_all_builds_from_yaml_and_applies() {
    let yaml = "dangerouslyAllowAllBuilds: true\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.dangerously_allow_all_builds, Some(true));

    let mut config = Config::new();
    assert!(!config.dangerously_allow_all_builds, "default is false");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.dangerously_allow_all_builds);
}

/// `scriptsPrependNodePath` is a tri-state
/// (`boolean | 'warn-only'`): `true` → Always, `false` → Never,
/// `"warn-only"` → `WarnOnly`. Pacquet's default is Never.
#[test]
fn parses_scripts_prepend_node_path_true_from_yaml() {
    let yaml = "scriptsPrependNodePath: true\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.scripts_prepend_node_path, Some(ScriptsPrependNodePath::Always));

    let mut config = Config::new();
    assert_eq!(config.scripts_prepend_node_path, ScriptsPrependNodePath::Never, "default Never");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.scripts_prepend_node_path, ScriptsPrependNodePath::Always);
}

#[test]
fn parses_scripts_prepend_node_path_false_from_yaml() {
    let yaml = "scriptsPrependNodePath: false\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.scripts_prepend_node_path, Some(ScriptsPrependNodePath::Never));
}

#[test]
fn parses_scripts_prepend_node_path_warn_only_from_yaml() {
    let yaml = "scriptsPrependNodePath: warn-only\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.scripts_prepend_node_path, Some(ScriptsPrependNodePath::WarnOnly));
}

#[test]
fn rejects_invalid_scripts_prepend_node_path() {
    let yaml = "scriptsPrependNodePath: nonsense\n";
    serde_saphyr::from_str::<WorkspaceSettings>(yaml).expect_err("must reject");
}

/// `linkWorkspacePackages` accepts `true | false | "deep"`.
#[test]
fn parses_link_workspace_packages_true_from_yaml() {
    let yaml = "linkWorkspacePackages: true\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.link_workspace_packages, Some(LinkWorkspacePackages::DirectOnly));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.link_workspace_packages, LinkWorkspacePackages::DirectOnly);
}

#[test]
fn parses_link_workspace_packages_false_from_yaml() {
    let yaml = "linkWorkspacePackages: false\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.link_workspace_packages, Some(LinkWorkspacePackages::Off));
}

#[test]
fn parses_link_workspace_packages_deep_from_yaml() {
    let yaml = "linkWorkspacePackages: deep\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.link_workspace_packages, Some(LinkWorkspacePackages::Deep));
}

#[test]
fn rejects_invalid_link_workspace_packages() {
    let yaml = "linkWorkspacePackages: shallow\n";
    serde_saphyr::from_str::<WorkspaceSettings>(yaml).expect_err("must reject");
}

/// `injectWorkspacePackages: true` propagates from yaml to
/// `Config.inject_workspace_packages`.
#[test]
fn parses_inject_workspace_packages_true_from_yaml() {
    let yaml = "injectWorkspacePackages: true\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.inject_workspace_packages, Some(true));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.inject_workspace_packages);
}

#[test]
fn parses_inject_workspace_packages_false_from_yaml() {
    let yaml = "injectWorkspacePackages: false\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.inject_workspace_packages, Some(false));

    let mut config = Config::new();
    config.inject_workspace_packages = true;
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(!config.inject_workspace_packages);
}

#[test]
fn inject_workspace_packages_defaults_off_when_absent() {
    let yaml = "linkWorkspacePackages: true\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.inject_workspace_packages, None);

    let config = Config::new();
    assert!(!config.inject_workspace_packages);
}

/// `unsafePerm: false` from yaml propagates to `Config.unsafe_perm`
/// on POSIX. The starting `Config::new()` value depends on the runtime
/// uid (see [`default_unsafe_perm`]) — `true` for non-root, `false`
/// for root. Either way, `apply_to` with `Some(false)` ends in
/// `false`.
#[test]
fn parses_unsafe_perm_from_yaml_and_applies() {
    // POSIX-only: the Windows force-override below would mask this
    // test's behavior. See [`WorkspaceSettings::apply_to`].
    if cfg!(windows) {
        return;
    }
    let yaml = "unsafePerm: false\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.unsafe_perm, Some(false));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(!config.unsafe_perm, "yaml override wins on POSIX");
}

/// On Windows, `apply_to` ignores the yaml value and forces
/// `unsafe_perm = true` — running lifecycle scripts under a uid/gid
/// drop is POSIX-only.
#[cfg(windows)]
#[test]
fn unsafe_perm_force_true_on_windows() {
    let yaml = "unsafePerm: false\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("C:/irrelevant"));
    assert!(config.unsafe_perm, "Windows forces unsafe_perm true regardless of yaml");
}

/// A positive `childConcurrency` is taken verbatim.
#[test]
fn parses_positive_child_concurrency_from_yaml_and_applies() {
    let yaml = "childConcurrency: 8\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.child_concurrency, Some(8));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.child_concurrency, 8);
}

/// A non-positive `childConcurrency` is interpreted as
/// `max(1, parallelism - |value|)`. The exact result depends on
/// the host's reported parallelism, so we just bound-check it:
/// negative offsets must produce at least 1 and at most
/// `parallelism()`.
#[test]
fn parses_negative_child_concurrency_from_yaml_and_resolves() {
    let yaml = "childConcurrency: -1\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.child_concurrency, Some(-1));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    let parallelism = crate::available_parallelism();
    assert!(config.child_concurrency >= 1, "must floor at 1");
    assert!(config.child_concurrency <= parallelism, "must not exceed available parallelism");
}

/// A positive `workspaceConcurrency` is taken verbatim — same
/// resolution as `childConcurrency`.
#[test]
fn parses_positive_workspace_concurrency_from_yaml_and_applies() {
    let yaml = "workspaceConcurrency: 8\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.workspace_concurrency, Some(8));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.workspace_concurrency, 8);
}

/// A non-positive `workspaceConcurrency` is interpreted as
/// `max(1, parallelism - |value|)`. The exact result depends on the
/// host's reported parallelism, so bound-check it like the
/// `childConcurrency` sibling does.
#[test]
fn parses_negative_workspace_concurrency_from_yaml_and_resolves() {
    let yaml = "workspaceConcurrency: -1\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.workspace_concurrency, Some(-1));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    let parallelism = crate::available_parallelism();
    assert!(config.workspace_concurrency >= 1, "must floor at 1");
    assert!(config.workspace_concurrency <= parallelism, "must not exceed available parallelism");
}

/// `workspaceConcurrency` and `childConcurrency` are independent
/// settings: setting one must not move the other off its default.
/// They are separate config keys.
#[test]
fn workspace_and_child_concurrency_are_independent() {
    let yaml = "workspaceConcurrency: 7\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.child_concurrency, None);

    let mut config = Config::new();
    let child_default = config.child_concurrency;
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.workspace_concurrency, 7);
    assert_eq!(config.child_concurrency, child_default, "childConcurrency stays at its default");
}

#[test]
fn apply_leaves_unset_fields_alone() {
    let yaml = "storeDir: /s\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    let before =
        (config.hoist, config.lockfile, config.registry.clone(), config.auto_install_peers);

    settings.apply_to(&mut config, Path::new("/anywhere"));

    assert_eq!(
        (config.hoist, config.lockfile, config.registry.clone(), config.auto_install_peers),
        before,
    );
}

#[test]
fn find_walks_up_to_parent_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let nested = tmp.path().join("a/b/c");
    fs::create_dir_all(&nested).unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "storeDir: /s\n").unwrap();

    let (found, settings) = WorkspaceSettings::find_and_load(&nested).unwrap().unwrap();
    assert_eq!(found, tmp.path().join("pnpm-workspace.yaml"));
    assert_eq!(settings.store_dir.as_deref(), Some("/s"));
}

/// Pnpm's `readManifestRaw` only treats `ENOENT` as "no manifest" and
/// propagates every other failure. A directory entry named
/// `pnpm-workspace.yaml` is not a missing file, so `find_and_load`
/// must surface it as `ReadFile` rather than silently walking up.
#[test]
fn find_propagates_when_manifest_path_is_a_directory() {
    let tmp = tempfile::tempdir().unwrap();
    tmp.path().join(WORKSPACE_MANIFEST_FILENAME).pipe(fs::create_dir).unwrap();

    let err = tmp
        .path()
        .pipe_as_ref(WorkspaceSettings::find_and_load)
        .expect_err("a directory at the manifest path is not a missing file");
    assert!(
        matches!(err, LoadWorkspaceYamlError::ReadFile { .. }),
        "expected ReadFile, got {err:?}",
    );

    drop(tmp);
}

/// A `pnpm-workspace.yaml` whose contents do not parse as YAML must
/// surface as `ParseYaml` (not `ReadFile`, not silently dropped),
/// matching pnpm's `readManifestRaw` behaviour where parse failures
/// abort the install rather than fall through to defaults.
#[test]
fn find_propagates_parse_yaml_error_on_malformed_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = tmp.path().join(WORKSPACE_MANIFEST_FILENAME);
    // Unmatched bracket; serde-saphyr rejects.
    fs::write(&manifest, "storeDir: [unterminated\n").unwrap();

    let err = WorkspaceSettings::find_and_load(tmp.path())
        .expect_err("malformed yaml must surface as ParseYaml");
    let LoadWorkspaceYamlError::ParseYaml { path, .. } = err else {
        panic!("expected ParseYaml, got {err:?}");
    };
    assert_eq!(path, manifest);

    drop(tmp);
}

#[test]
fn find_returns_none_when_no_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(WorkspaceSettings::find_and_load(tmp.path()).unwrap().is_none());
}

#[test]
fn apply_replaces_git_shallow_hosts_defaults() {
    // pnpm replaces the built-in default array wholesale rather than
    // merging it, so we mirror that. See `default_git_shallow_hosts`.
    let yaml = r"
gitShallowHosts:
  - corp-git.example.com
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();

    // Sanity-check the default before applying — `github.com` is the
    // first entry in pnpm's list, and replacement (not merging) is the
    // bit we want to verify.
    assert!(config.git_shallow_hosts.iter().any(|host| host == "github.com"));

    settings.apply_to(&mut config, Path::new("/irrelevant"));

    assert_eq!(config.git_shallow_hosts, vec!["corp-git.example.com".to_string()]);
}

/// `supportedArchitectures` from `pnpm-workspace.yaml`. Optional
/// `os` / `cpu` / `libc` lists; absent fields stay `None`. Threaded
/// into [`pnpm_package_is_installable::check_platform`] via
/// [`Config::supported_architectures`] at install time.
#[test]
fn parses_supported_architectures_from_yaml_and_applies() {
    let yaml = r"
supportedArchitectures:
  os: [darwin, linux]
  cpu: [arm64, x64]
  libc: [glibc]
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let raw = settings.supported_architectures.clone().expect("field present");
    assert_eq!(raw.os.as_deref(), Some(&["darwin".to_string(), "linux".to_string()][..]));
    assert_eq!(raw.cpu.as_deref(), Some(&["arm64".to_string(), "x64".to_string()][..]));
    assert_eq!(raw.libc.as_deref(), Some(&["glibc".to_string()][..]));

    let mut config = Config::new();
    assert!(config.supported_architectures.is_none(), "default is None");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    let applied = config.supported_architectures.expect("set after apply_to");
    assert_eq!(applied.os.as_deref(), Some(&["darwin".to_string(), "linux".to_string()][..]));
    assert_eq!(applied.cpu.as_deref(), Some(&["arm64".to_string(), "x64".to_string()][..]));
    assert_eq!(applied.libc.as_deref(), Some(&["glibc".to_string()][..]));
}

/// Absent `supportedArchitectures` leaves the config field at
/// `None`. Same shape as upstream: yaml-side absence translates to
/// `targetConfig.supportedArchitectures` staying `undefined` so the
/// per-axis check falls back to the host triple.
#[test]
fn omitting_supported_architectures_keeps_default() {
    let yaml = "name: stub\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap_or_default();
    assert!(settings.supported_architectures.is_none());

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.supported_architectures.is_none());
}

/// Partial `supportedArchitectures` (only one axis set) round-trips
/// with the other axes as `None`. Matches upstream where each axis
/// is independently overridable.
#[test]
fn partial_supported_architectures_only_sets_listed_axes() {
    let yaml = r"
supportedArchitectures:
  os: [darwin]
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let raw = settings.supported_architectures.expect("field present");
    assert_eq!(raw.os.as_deref(), Some(&["darwin".to_string()][..]));
    assert!(raw.cpu.is_none());
    assert!(raw.libc.is_none());
}

/// `hoistPattern` and `publicHoistPattern` are tri-state via
/// [`super::deserialize_double_option`] — pacquet must distinguish
/// "key missing" (defaults stay) from "explicit null" (hoist
/// disabled) from "explicit list" (override). This test exercises
/// all three for both sides plus the `apply_to` plumbing.
#[test]
fn hoist_patterns_tri_state_round_trip() {
    let yaml = "registry: https://example.test\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.hoist_pattern, None);
    assert_eq!(settings.public_hoist_pattern, None);
    let mut config = Config::default();
    let defaults = (config.hoist_pattern.clone(), config.public_hoist_pattern.clone());
    settings.apply_to(&mut config, Path::new("/anywhere"));
    assert_eq!((config.hoist_pattern.clone(), config.public_hoist_pattern.clone()), defaults);

    let yaml = "hoistPattern: null\npublicHoistPattern: null\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.hoist_pattern, Some(None));
    assert_eq!(settings.public_hoist_pattern, Some(None));
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/anywhere"));
    assert_eq!(config.hoist_pattern, None);
    assert_eq!(config.public_hoist_pattern, None);

    let yaml = "hoistPattern:\n  - 'foo*'\npublicHoistPattern: []\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.hoist_pattern, Some(Some(vec!["foo*".to_string()])));
    assert_eq!(settings.public_hoist_pattern, Some(Some(vec![])));
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/anywhere"));
    assert_eq!(config.hoist_pattern, Some(vec!["foo*".to_string()]));
    assert_eq!(config.public_hoist_pattern, Some(vec![]));
}

/// `hoist: false` in `pnpm-workspace.yaml` nullifies
/// `Config.hoist_pattern` even when the user supplied an explicit
/// `hoistPattern` (or when the default `Some(["*"])` is in place):
/// `hoist === false ⇒ hoistPattern: undefined`. The install-time
/// `is_some() || is_some()` guard then short-circuits private
/// hoisting; `public_hoist_pattern` is intentionally untouched.
#[test]
fn hoist_false_disables_private_hoist_pattern() {
    let yaml = "hoist: false\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::default();
    let original_public = config.public_hoist_pattern.clone();
    settings.apply_to(&mut config, Path::new("/anywhere"));
    assert_eq!(config.hoist, false);
    assert_eq!(config.hoist_pattern, None, "hoist:false must drop hoist_pattern");
    assert_eq!(
        config.public_hoist_pattern, original_public,
        "hoist:false must NOT touch public_hoist_pattern",
    );

    let yaml = "hoist: false\nhoistPattern:\n  - 'foo*'\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/anywhere"));
    assert_eq!(config.hoist_pattern, None, "hoist:false must override an explicit hoistPattern");
}

/// `ignoredOptionalDependencies` parses from yaml as a list of
/// strings and applies onto `Config::ignored_optional_dependencies`
/// verbatim — order preserved, no sorting at apply time (the
/// freshness check sorts before comparison, but `Config` holds the
/// user-supplied order).
#[test]
fn parses_ignored_optional_dependencies_from_yaml_and_applies() {
    let yaml = r"
ignoredOptionalDependencies:
  - 'foo'
  - '@scope/bar'
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(
        settings.ignored_optional_dependencies.as_deref(),
        Some(&["foo".to_string(), "@scope/bar".to_string()][..]),
    );

    let mut config = Config::new();
    assert!(config.ignored_optional_dependencies.is_none(), "default is None");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config.ignored_optional_dependencies.as_deref(),
        Some(&["foo".to_string(), "@scope/bar".to_string()][..]),
    );
}

/// Absent `ignoredOptionalDependencies` leaves the config field at
/// `None` (same convention as `supportedArchitectures`).
#[test]
fn omitting_ignored_optional_dependencies_keeps_default() {
    let yaml = "name: stub\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap_or_default();
    assert!(settings.ignored_optional_dependencies.is_none());

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.ignored_optional_dependencies.is_none());
}

/// `overrides` parses as an ordered string→string map and applies
/// onto `Config::overrides`. Order is preserved because the field is
/// an `IndexMap` — pnpm's lockfile-drift comparison is
/// order-insensitive, but the read-package hook iterates the map and
/// downstream diagnostics reference the keys in user-supplied order.
#[test]
fn parses_overrides_from_yaml_and_applies() {
    let yaml = r"
overrides:
  foo: '1.2.3'
  '@scope/bar': '^2.0.0'
  'baz>qux': '-'
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let overrides = settings.overrides.as_ref().expect("overrides parsed");
    let entries: Vec<_> =
        overrides.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect();
    assert_eq!(entries, vec![("foo", "1.2.3"), ("@scope/bar", "^2.0.0"), ("baz>qux", "-")]);

    let mut config = Config::new();
    assert!(config.overrides.is_none(), "default is None");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    let applied = config.overrides.expect("overrides applied");
    assert_eq!(applied.get("foo").map(String::as_str), Some("1.2.3"));
    assert_eq!(applied.get("@scope/bar").map(String::as_str), Some("^2.0.0"));
    assert_eq!(applied.get("baz>qux").map(String::as_str), Some("-"));
}

/// An empty `overrides:` map collapses to `None` on `Config`, matching
/// upstream's `delete settings.overrides` short-circuit in
/// `getOptionsFromPnpmSettings`. Without this collapse, an empty
/// `overrides: {}` would diverge from "no key set" at the lockfile-
/// drift comparison.
#[test]
fn empty_overrides_map_collapses_to_none() {
    let yaml = "overrides: {}\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert!(settings.overrides.as_ref().is_some_and(indexmap::IndexMap::is_empty));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.overrides.is_none(), "empty map collapses to None");
}

/// An explicit `overrides: {}` from a later layer (env overlay,
/// later `apply_to` call) clears a non-empty value set by an earlier
/// layer. Without the empty-clears-prior semantic, an env override
/// like `PNPM_CONFIG_OVERRIDES={}` would be a silent no-op against a
/// non-empty workspace yaml.
#[test]
fn empty_overrides_clears_prior_non_empty_assignment() {
    let mut config = Config::new();
    let yaml_with_overrides = "overrides:\n  foo: '1.2.3'\n";
    let earlier: WorkspaceSettings = serde_saphyr::from_str(yaml_with_overrides).unwrap();
    earlier.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.overrides.is_some(), "non-empty overrides applied");

    let later: WorkspaceSettings = serde_saphyr::from_str("overrides: {}\n").unwrap();
    later.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.overrides.is_none(), "explicit empty must clear earlier non-empty");
}

/// Absent `overrides` leaves the config field at `None`.
#[test]
fn omitting_overrides_keeps_default() {
    let yaml = "name: stub\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap_or_default();
    assert!(settings.overrides.is_none());

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.overrides.is_none());
}

/// `packageExtensions` parses as an ordered `selector → entry` map
/// and applies onto [`Config::package_extensions`]. The entry uses
/// camelCase field names so inner sections like
/// `optionalDependencies` and `peerDependenciesMeta` round-trip
/// through the deserializer.
#[test]
fn parses_package_extensions_from_yaml_and_applies() {
    let yaml = r#"
packageExtensions:
  is-positive:
    dependencies:
      "@pnpm.e2e/bar": 100.1.0
  "@scope/foo@^2":
    peerDependencies:
      react: ">=16"
    peerDependenciesMeta:
      react:
        optional: true
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let extensions = settings.package_extensions.as_ref().expect("packageExtensions parsed");
    let is_positive = extensions.get("is-positive").expect("is-positive entry");
    assert_eq!(
        is_positive
            .dependencies
            .as_ref()
            .and_then(|map| map.get("@pnpm.e2e/bar"))
            .map(String::as_str),
        Some("100.1.0"),
    );
    let scoped = extensions.get("@scope/foo@^2").expect("scoped entry");
    assert_eq!(
        scoped.peer_dependencies.as_ref().and_then(|map| map.get("react")).map(String::as_str),
        Some(">=16"),
    );
    let meta = scoped
        .peer_dependencies_meta
        .as_ref()
        .and_then(|map| map.get("react"))
        .expect("react peerDependenciesMeta entry");
    assert_eq!(meta.optional, Some(true));

    let mut config = Config::new();
    assert!(config.package_extensions.is_none(), "default is None");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    let applied = config.package_extensions.expect("package_extensions applied");
    assert_eq!(applied.len(), 2);
}

/// An empty `packageExtensions:` map collapses to `None` on
/// `Config`, mirroring the `overrides` behavior. Without this
/// collapse, an empty `{}` would diverge from "no key set" at the
/// workspace-state drift comparison.
#[test]
fn empty_package_extensions_map_collapses_to_none() {
    let yaml = "packageExtensions: {}\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert!(settings.package_extensions.as_ref().is_some_and(indexmap::IndexMap::is_empty));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.package_extensions.is_none(), "empty map collapses to None");
}

/// `hoistingLimits` deserializes as one of the `none` / `workspaces`
/// / `dependencies` modes; the install pipeline translates the mode
/// into the per-locator border map via
/// `pnpm_package_manager::get_hoisting_limits`. Yaml-empty /
/// missing keeps the `Config` field at its [`HoistingLimits::None`]
/// default.
#[test]
fn parses_hoisting_limits_from_yaml_and_applies() {
    let yaml = "hoistingLimits: dependencies\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.hoisting_limits, Some(HoistingLimits::Dependencies));

    let mut config = Config::new();
    assert_eq!(config.hoisting_limits, HoistingLimits::None, "default is None");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.hoisting_limits, HoistingLimits::Dependencies);
}

/// `externalDependencies` deserializes as a flat list of names.
/// Yaml-empty / missing keeps the `Config` field at its
/// `BTreeSet::default()` empty value.
#[test]
fn parses_external_dependencies_from_yaml_and_applies() {
    let yaml = r"
externalDependencies:
  - bit-bin
  - some-other-external
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let raw = settings.external_dependencies.clone().expect("field present");
    assert!(raw.contains("bit-bin") && raw.contains("some-other-external"));

    let mut config = Config::new();
    assert!(config.external_dependencies.is_empty(), "default is empty");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.external_dependencies.contains("bit-bin"));
    assert!(config.external_dependencies.contains("some-other-external"));
}

/// Both knobs absent → both `Config` fields stay at their empty
/// defaults. Pins the `apply_to` skip-on-None branch so future
/// edits don't accidentally overwrite with empty when the yaml
/// just doesn't mention these settings.
#[test]
fn omitting_hoisting_limits_and_external_dependencies_keeps_defaults() {
    let yaml = "";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert!(settings.hoisting_limits.is_none());
    assert!(settings.external_dependencies.is_none());

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.hoisting_limits, HoistingLimits::None);
    assert!(config.external_dependencies.is_empty());
}

/// Lockfile-verification policy keys all live in `pnpm-workspace.yaml`
/// alongside the rest of the install settings. This test asserts the
/// camelCase rename + `apply_to` wiring for every new field
/// introduced by the gate: `cacheDir` (path-resolved against the
/// workspace dir), `minimumReleaseAge` / `…Exclude` / `…Strict` /
/// `…IgnoreMissingTime`, and `trustPolicy` / `…Exclude` /
/// `…IgnoreAfter`.
#[test]
fn parses_supply_chain_policy_settings_from_yaml_and_applies() {
    let yaml = r#"
cacheDir: ./.pacquet-cache
minimumReleaseAge: 1440
minimumReleaseAgeExclude:
  - lodash
  - "is-*"
minimumReleaseAgeIgnoreMissingTime: true
minimumReleaseAgeStrict: true
trustLockfile: true
trustPolicy: no-downgrade
trustPolicyExclude:
  - "@scope/legacy"
trustPolicyIgnoreAfter: 525600
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.cache_dir.as_deref(), Some("./.pacquet-cache"));
    assert_eq!(settings.minimum_release_age, Some(1440));
    assert_eq!(
        settings.minimum_release_age_exclude.as_deref(),
        Some(&["lodash".to_string(), "is-*".to_string()][..]),
    );
    assert_eq!(settings.minimum_release_age_ignore_missing_time, Some(true));
    assert_eq!(settings.minimum_release_age_strict, Some(true));
    assert_eq!(settings.trust_lockfile, Some(true));
    assert_eq!(settings.trust_policy, Some(TrustPolicy::NoDowngrade));
    assert_eq!(settings.trust_policy_exclude.as_deref(), Some(&["@scope/legacy".to_string()][..]));
    assert_eq!(settings.trust_policy_ignore_after, Some(525_600));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/proj"));
    assert_eq!(config.cache_dir, Path::new("/proj/.pacquet-cache"));
    assert_eq!(config.minimum_release_age, Some(1440));
    assert_eq!(
        config.minimum_release_age_exclude.as_deref(),
        Some(&["lodash".to_string(), "is-*".to_string()][..]),
    );
    assert!(config.minimum_release_age_ignore_missing_time);
    assert_eq!(config.minimum_release_age_strict, Some(true));
    assert!(config.resolved_minimum_release_age_strict());
    assert!(config.trust_lockfile);
    assert_eq!(config.trust_policy, TrustPolicy::NoDowngrade);
    assert_eq!(config.trust_policy_exclude.as_deref(), Some(&["@scope/legacy".to_string()][..]));
    assert_eq!(config.trust_policy_ignore_after, Some(525_600));
}

/// `trustPolicy` accepts the two upstream string values; an absent
/// key leaves the [`TrustPolicy::Off`] default in place.
#[test]
fn trust_policy_yaml_values_round_trip() {
    let yaml = "trustPolicy: off\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.trust_policy, Some(TrustPolicy::Off));

    let yaml = "trustPolicy: no-downgrade\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.trust_policy, Some(TrustPolicy::NoDowngrade));

    let yaml = "";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert!(settings.trust_policy.is_none());
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.trust_policy, TrustPolicy::Off, "default stays off when key is absent");
}

/// `resolutionMode` accepts the three upstream string values; an
/// absent key leaves the [`ResolutionMode::Highest`] default in place.
#[test]
fn resolution_mode_yaml_values_round_trip() {
    for (yaml, expected) in [
        ("resolutionMode: highest\n", ResolutionMode::Highest),
        ("resolutionMode: time-based\n", ResolutionMode::TimeBased),
        ("resolutionMode: lowest-direct\n", ResolutionMode::LowestDirect),
    ] {
        let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(settings.resolution_mode, Some(expected));
        let mut config = Config::new();
        settings.apply_to(&mut config, Path::new("/irrelevant"));
        assert_eq!(config.resolution_mode, expected);
    }

    let settings: WorkspaceSettings = serde_saphyr::from_str("").unwrap();
    assert!(settings.resolution_mode.is_none());
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config.resolution_mode,
        ResolutionMode::Highest,
        "default stays highest when the key is absent",
    );
}

/// `catalogMode` accepts the three upstream string values; an absent key
/// leaves the [`CatalogMode::Manual`] default in place.
#[test]
fn catalog_mode_yaml_values_round_trip() {
    for (yaml, expected) in [
        ("catalogMode: manual\n", CatalogMode::Manual),
        ("catalogMode: strict\n", CatalogMode::Strict),
        ("catalogMode: prefer\n", CatalogMode::Prefer),
    ] {
        let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(settings.catalog_mode, Some(expected));
        let mut config = Config::new();
        settings.apply_to(&mut config, Path::new("/irrelevant"));
        assert_eq!(config.catalog_mode, expected);
    }

    let settings: WorkspaceSettings = serde_saphyr::from_str("").unwrap();
    assert!(settings.catalog_mode.is_none());
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config.catalog_mode,
        CatalogMode::Manual,
        "default stays manual when the key is absent",
    );
}

/// `registrySupportsTimeField` is a camelCase boolean; default `false`.
#[test]
fn parses_registry_supports_time_field_from_yaml_and_applies() {
    let yaml = "registrySupportsTimeField: true\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.registry_supports_time_field, Some(true));

    let mut config = Config::new();
    assert!(!config.registry_supports_time_field, "the default is `false`");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.registry_supports_time_field, "yaml override wins");
}

/// `allowedDeprecatedVersions` is a `name → semver-range` map parsed
/// from camelCase yaml and applied verbatim onto `Config`.
#[test]
fn parses_allowed_deprecated_versions_from_yaml_and_applies() {
    let yaml = r#"
allowedDeprecatedVersions:
  request: "^2.88.0"
  lodash: "<5.0.0"
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();

    let mut config = Config::new();
    assert!(config.allowed_deprecated_versions.is_empty(), "default is empty");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config.allowed_deprecated_versions.get("request").map(String::as_str),
        Some("^2.88.0"),
    );
    assert_eq!(
        config.allowed_deprecated_versions.get("lodash").map(String::as_str),
        Some("<5.0.0"),
    );
}

/// The deprecated `updateConfig.ignoreDependencies` parses from the nested
/// camelCase shape and lands on `Config.update_config`.
#[test]
fn parses_update_config_from_yaml_and_applies() {
    let yaml = r#"
updateConfig:
  ignoreDependencies:
    - "@pnpm.e2e/foo"
    - "@pnpm.e2e/bar"
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();

    let mut config = Config::new();
    assert!(config.update_config.ignore_dependencies.is_none(), "default is unset");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config.update_config.ignore_dependencies.as_deref(),
        Some(&["@pnpm.e2e/foo".to_string(), "@pnpm.e2e/bar".to_string()][..]),
    );
}

#[test]
fn parses_update_section_from_yaml_and_applies() {
    let yaml = r#"
update:
  changeset: true
  githubActions: true
  githubActionsServer: https://github.example.com
  ignoreDeps:
    - "@pnpm.e2e/foo"
    - "@pnpm.e2e/bar"
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();

    let mut config = Config::new();
    assert!(config.update_config.changeset.is_none(), "default is unset");
    assert!(config.update_config.ignore_dependencies.is_none(), "default is unset");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.update_config.changeset, Some(true));
    assert_eq!(
        config.update_config.ignore_dependencies.as_deref(),
        Some(&["@pnpm.e2e/foo".to_string(), "@pnpm.e2e/bar".to_string()][..]),
    );
    assert_eq!(config.update_config.github_actions, Some(true));
    assert_eq!(
        config.update_config.github_actions_server.as_deref(),
        Some("https://github.example.com"),
    );
}

#[test]
fn update_section_takes_precedence_over_update_config() {
    let yaml = r#"
update:
  ignoreDeps:
    - "@pnpm.e2e/foo"
updateConfig:
  ignoreDependencies:
    - "@pnpm.e2e/bar"
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config.update_config.ignore_dependencies.as_deref(),
        Some(&["@pnpm.e2e/foo".to_string()][..]),
        "the update section should override updateConfig",
    );
}

#[test]
fn parses_audit_section_from_yaml_and_applies() {
    let yaml = r"
audit:
  level: high
  ignore:
    - GHSA-1
    - GHSA-2
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.audit_level, Some(AuditLevel::High));
    assert_eq!(config.audit_config.ignore_ghsas, vec!["GHSA-1".to_string(), "GHSA-2".to_string()]);
}

#[test]
fn audit_section_takes_precedence_over_audit_level_and_config() {
    let yaml = r"
audit:
  level: critical
  ignore:
    - GHSA-new
auditLevel: low
auditConfig:
  ignoreGhsas:
    - GHSA-old
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.audit_level, Some(AuditLevel::Critical));
    assert_eq!(config.audit_config.ignore_ghsas, vec!["GHSA-new".to_string()]);
}

/// `peerDependencyRules` parses its three sub-fields from camelCase
/// yaml and lands on `Config.peer_dependency_rules`.
#[test]
fn parses_peer_dependency_rules_from_yaml_and_applies() {
    let yaml = r#"
peerDependencyRules:
  ignoreMissing:
    - ajv
  allowAny:
    - react
  allowedVersions:
    bbb: "2"
    "xxx>@foo/bar": "2"
"#;
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();

    let mut config = Config::new();
    assert_eq!(
        config.peer_dependency_rules,
        crate::PeerDependencyRules::default(),
        "default is empty",
    );
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    let rules = &config.peer_dependency_rules;
    assert_eq!(rules.ignore_missing.as_deref(), Some(&["ajv".to_string()][..]));
    assert_eq!(rules.allow_any.as_deref(), Some(&["react".to_string()][..]));
    let allowed = rules.allowed_versions.as_ref().expect("allowedVersions set");
    assert_eq!(allowed.get("bbb").map(String::as_str), Some("2"));
    assert_eq!(allowed.get("xxx>@foo/bar").map(String::as_str), Some("2"));
}

#[test]
fn parses_script_shell_and_node_options_from_yaml_and_applies() {
    let yaml = r"
scriptShell: /usr/bin/bash
nodeOptions: --max-old-space-size=4096
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.script_shell, Some(Some("/usr/bin/bash".to_string())));
    assert_eq!(settings.node_options, Some(Some("--max-old-space-size=4096".to_string())));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.script_shell.as_deref(), Some("/usr/bin/bash"));
    assert_eq!(config.node_options.as_deref(), Some("--max-old-space-size=4096"));
}

/// The tri-state distinguishes "absent" from "explicit null", matching
/// pnpm: an explicit `scriptShell: null` / `nodeOptions: null` clears a
/// value inherited from global `config.yaml`, while an absent key leaves
/// the inherited value untouched.
#[test]
fn script_shell_and_node_options_null_clears_inherited_value() {
    let absent: WorkspaceSettings = serde_saphyr::from_str("hoist: true").unwrap();
    assert_eq!(absent.script_shell, None);
    assert_eq!(absent.node_options, None);

    let mut config = Config::new();
    config.script_shell = Some("/inherited/sh".to_string());
    config.node_options = Some("--inherited".to_string());
    absent.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.script_shell.as_deref(), Some("/inherited/sh"), "absent must inherit");
    assert_eq!(config.node_options.as_deref(), Some("--inherited"), "absent must inherit");

    let cleared: WorkspaceSettings =
        serde_saphyr::from_str("scriptShell: null\nnodeOptions: null").unwrap();
    assert_eq!(cleared.script_shell, Some(None));
    assert_eq!(cleared.node_options, Some(None));

    let mut config = Config::new();
    config.script_shell = Some("/inherited/sh".to_string());
    config.node_options = Some("--inherited".to_string());
    cleared.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.script_shell, None, "explicit null must clear the inherited shell");
    assert_eq!(config.node_options, None, "explicit null must clear inherited NODE_OPTIONS");
}

/// `frozenStore` parses from `pnpm-workspace.yaml` as a camelCase
/// boolean and `apply_to` pushes it onto the `Config`. Defaults to
/// `false` when the key is absent, matching pnpm's `frozen-store`
/// default. Drives the read-only-store open path (`immutable=1`) and
/// the disabled `index.db` writer.
#[test]
fn parses_frozen_store_from_yaml_and_applies() {
    let absent: WorkspaceSettings = serde_saphyr::from_str("hoist: true").unwrap();
    assert_eq!(absent.frozen_store, None);
    let mut config = Config::new();
    assert!(!config.frozen_store, "frozen_store must default to false");
    absent.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(!config.frozen_store, "absent frozenStore must leave the default in place");

    let enabled: WorkspaceSettings = serde_saphyr::from_str("frozenStore: true").unwrap();
    assert_eq!(enabled.frozen_store, Some(true));
    let mut config = Config::new();
    enabled.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.frozen_store, "frozenStore: true must apply onto the config");
}

/// `frozenLockfile` parses from `pnpm-workspace.yaml` as a camelCase
/// boolean and `apply_to` pushes it onto the `Config` as an explicit
/// `Some`, which the CLI layers `--frozen-lockfile` /
/// `--no-frozen-lockfile` over. It is excluded from the global
/// `config.yaml`, matching pnpm's `excludedPnpmKeys`.
#[test]
fn parses_frozen_lockfile_from_yaml_and_applies() {
    let absent: WorkspaceSettings = serde_saphyr::from_str("hoist: true").unwrap();
    assert_eq!(absent.frozen_lockfile, None);
    let mut config = Config::new();
    assert_eq!(config.frozen_lockfile, None, "frozen_lockfile must default to unset");
    absent.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.frozen_lockfile, None, "absent frozenLockfile must leave the default");

    let enabled: WorkspaceSettings = serde_saphyr::from_str("frozenLockfile: true").unwrap();
    assert_eq!(enabled.frozen_lockfile, Some(true));
    let mut config = Config::new();
    enabled.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.frozen_lockfile, Some(true));

    let mut global: WorkspaceSettings = serde_saphyr::from_str("frozenLockfile: true").unwrap();
    global.clear_workspace_only_fields();
    assert_eq!(global.frozen_lockfile, None);
}

/// The `add`-time save settings parse from `pnpm-workspace.yaml` and
/// apply onto the `Config`. `savePeer` and `saveCatalogName` are
/// workspace-only (pnpm's `excludedPnpmKeys`); `savePrefix` is an npm
/// key and stays readable from the global `config.yaml`.
#[test]
fn parses_save_settings_from_yaml_and_applies() {
    let yaml = "savePrefix: '~'\nsavePeer: true\nsaveCatalogName: shared\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.save_prefix.as_deref(), Some("~"));
    assert_eq!(settings.save_peer, Some(true));
    assert_eq!(settings.save_catalog_name.as_deref(), Some("shared"));

    let mut config = Config::new();
    assert_eq!(config.save_prefix, None, "default is unset");
    assert!(!config.save_peer, "default is false");
    assert_eq!(config.save_catalog_name, None, "default is unset");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.save_prefix.as_deref(), Some("~"));
    assert!(config.save_peer);
    assert_eq!(config.save_catalog_name.as_deref(), Some("shared"));

    let mut global: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    global.clear_workspace_only_fields();
    assert_eq!(global.save_prefix.as_deref(), Some("~"));
    assert_eq!(global.save_peer, None);
    assert_eq!(global.save_catalog_name, None);
}

/// `apply_to` keys the per-registry options by registry URL with a trailing
/// slash so a lookup by the registry a package resolved from matches.
#[test]
fn parses_registry_declarations_from_yaml_and_normalizes_the_keys() {
    let yaml = r"
registries:
  https://artifactory.example/artifactory/api/npm/npm-virtual: {serverType: artifactory}
  https://npm.example.com/: {serverType: npm}
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config
            .registry_options_by_url
            .get("https://artifactory.example/artifactory/api/npm/npm-virtual/")
            .map(|options| options.server_type),
        Some(Some(RegistryServerType::Artifactory)),
    );
    assert_eq!(
        config
            .registry_options_by_url
            .get("https://npm.example.com/")
            .map(|options| options.server_type),
        Some(Some(RegistryServerType::Npm)),
    );
}

/// Credentials belong in `.npmrc`, which is not committed. Refused after
/// parsing, not by `deny_unknown_fields`: a parse error renders the offending
/// source line verbatim, which would print the very token being refused.
#[test]
fn rejects_credentials_in_a_registry_declaration() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  https://npm.example.com/: {_authToken: hunter2}\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path())
        .expect_err("credentials in a declaration must not load")
        .to_string();
    assert!(error.contains("_authToken"), "the field is named: {error}");
    assert!(!error.contains("hunter2"), "the token must not be echoed: {error}");
}

/// A misspelled field would otherwise sit there doing nothing.
#[test]
fn rejects_an_unknown_registry_declaration_field() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  https://npm.example.com/: {scope: '@acme'}\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path())
        .expect_err("an unknown declaration field must not load")
        .to_string();
    assert!(error.contains("scope"), "the field is named: {error}");
}

#[test]
fn rejects_an_unknown_registry_server_type() {
    let yaml = r"
registries:
  https://npm.example.com/: {serverType: nexus}
";
    let received = serde_saphyr::from_str::<WorkspaceSettings>(yaml);
    assert!(received.is_err(), "an unknown serverType must not parse");
}

/// The registry URL is the key here, so the untrusted-environment gate has to
/// drop the entry rather than expanding a placeholder into a request URL. The
/// variable resolves, so a dropped entry is distinguishable from one expanded
/// to an empty string — expanding it is how a token would reach an
/// attacker-chosen host from a committed pnpm-workspace.yaml.
#[test]
fn drops_a_registry_declaration_whose_url_has_an_env_placeholder() {
    struct EnvWithToken;
    impl EnvVar for EnvWithToken {
        fn var(name: &str) -> Option<String> {
            (name == "PNPM_TEST_REGISTRY_TOKEN").then(|| "super-secret-token".to_owned())
        }
    }

    let yaml = r"
registries:
  https://evil.example.com/${PNPM_TEST_REGISTRY_TOKEN}/: {serverType: artifactory}
  https://npm.example.com/: {serverType: artifactory}
";
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    settings.substitute_env_untrusted::<EnvWithToken>();
    let entries = settings.registries.as_ref().expect("registries present");
    assert_eq!(entries.len(), 1);
    assert!(entries.contains_key("https://npm.example.com/"));
    assert!(
        !entries.keys().any(|registry| registry.contains("super-secret-token")),
        "the token must never be expanded into a registry URL: {entries:?}",
    );
}

/// A declared `serverType` is workspace-only: it decides which tarball URLs
/// are omitted from the lockfile, so a user's global `config.yaml` must not
/// shape a lockfile their collaborators read back with a different layout. The
/// routes the same entry declares are a legitimate global preference and stay.
#[test]
fn registry_server_type_cleared_as_workspace_only_field() {
    let yaml = r"
registries:
  https://artifactory.example/artifactory/api/npm/npm-virtual/:
    serverType: artifactory
    scopes: ['@acme']
";
    let mut settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    settings.clear_workspace_only_fields();
    let entries = settings.registries.as_ref().expect("registries present");
    let RegistryEntry::Declaration(declaration) = entries.values().next().expect("one declaration")
    else {
        panic!("expected a declaration: {entries:?}")
    };
    assert_eq!(declaration.server_type, None);
    assert_eq!(declaration.scopes.as_deref(), Some(["@acme".to_owned()].as_slice()));
}

/// A credential in the key is the same secret in the same committed file as a
/// credential in a field, so both are refused. The check runs after parsing so
/// the error carries a redacted URL instead of serde's verbatim source line.
#[test]
fn rejects_a_registry_key_that_embeds_credentials() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  https://ci-user-6e42:hunter2@npm.example.com/: {serverType: artifactory}\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path())
        .expect_err("a key with credentials must not load")
        .to_string();
    assert!(!error.contains("hunter2"), "the password must not be echoed: {error}");
    assert!(!error.contains("ci-user-6e42"), "the username must not be echoed: {error}");
    assert!(error.contains("npm.example.com"), "the host is still named: {error}");
}

/// `.npmrc` scopes settings with a scheme-less `//host/`, and this setting's
/// own error points users at that syntax, so it is the form they are most
/// likely to write — and it must not slip past the check.
#[test]
fn rejects_a_scheme_less_registry_key_that_embeds_credentials() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  //ci-user-6e42:hunter2@npm.example.com/: {serverType: artifactory}\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path())
        .expect_err("a scheme-less key with credentials must not load")
        .to_string();
    assert!(!error.contains("hunter2"), "the password must not be echoed: {error}");
    assert!(!error.contains("ci-user-6e42"), "the username must not be echoed: {error}");
}

/// A later `@` in the path is not userinfo.
#[test]
fn accepts_a_registry_key_with_an_at_sign_in_the_path() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  https://npm.example.com/scope@1/: {serverType: artifactory}\n",
    )
    .unwrap();

    let settings = WorkspaceSettings::load_at(dir.path()).unwrap().expect("settings");
    assert!(settings.registries.is_some());
}

/// Searching for the first `://` would find the one in the path and parse the
/// authority from there, leaving the real credentials unexamined.
#[test]
fn rejects_a_scheme_less_key_whose_path_contains_a_scheme_separator() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  '//ci-user-6e42:hunter2@npm.example.com/a://b': {serverType: artifactory}\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path())
        .expect_err("credentials must not slip past a path scheme separator")
        .to_string();
    assert!(!error.contains("hunter2"), "the password must not be echoed: {error}");
    assert!(!error.contains("ci-user-6e42"), "the username must not be echoed: {error}");
}

/// A scope resolves to one registry while a registry serves many, so the
/// declaration reads the way it is written and the lookup is its inverse. A
/// bare `@` is the scope-less default registry.
#[test]
fn routes_the_scopes_a_registry_declares() {
    let yaml = r"
registries:
  https://npm.corp.example:
    serverType: npm
    scopes: ['@', '@foo', '@bar']
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.registry, "https://npm.corp.example/");
    assert_eq!(
        config.registries_by_scope.get("@foo").map(String::as_str),
        Some("https://npm.corp.example/"),
    );
    assert_eq!(
        config.registries_by_scope.get("@bar").map(String::as_str),
        Some("https://npm.corp.example/"),
    );
}

#[test]
fn rejects_a_scope_declared_without_its_at_sign() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  https://npm.corp.example/: {scopes: [foo]}\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path())
        .expect_err("a scope without its @ must not load")
        .to_string();
    assert!(error.contains(r#""foo""#), "the scope is named: {error}");
}

#[test]
fn rejects_one_scope_routed_to_two_registries() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  https://npm.corp.example/: {scopes: ['@foo']}\n  https://artifactory.example/: {scopes: ['@foo']}\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path())
        .expect_err("a scope routed twice must not load")
        .to_string();
    assert!(error.contains("routed to two registries"), "{error}");
}

/// Keyed by the URL as written: a named registry's URL is what a lockfile's
/// recorded tarball URLs are matched against, so normalizing it here would
/// change what an existing lockfile verifies against.
#[test]
fn reads_a_declared_prefix_as_a_named_registry() {
    let yaml = r"
registries:
  https://npm.corp.example: {prefix: work, serverType: artifactory}
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config.registries_by_prefix.get("work").map(String::as_str),
        Some("https://npm.corp.example"),
    );
    assert_eq!(
        config
            .registry_options_by_url
            .get("https://npm.corp.example/")
            .map(|options| options.server_type),
        Some(Some(RegistryServerType::Artifactory)),
    );
}

#[test]
fn rejects_one_prefix_declared_by_two_registries() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  https://npm.corp.example/: {prefix: work}\n  https://artifactory.example/: {prefix: work}\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path())
        .expect_err("a prefix declared twice must not load")
        .to_string();
    assert!(error.contains("declared by two registries"), "{error}");
}

/// The deprecated spelling of the same thing, so a declared prefix wins.
#[test]
fn a_declared_prefix_wins_over_named_registries() {
    let yaml = r"
namedRegistries:
  work: https://stale.example/
  other: https://other.example/
registries:
  https://npm.corp.example/: {prefix: work}
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config.registries_by_prefix.get("work").map(String::as_str),
        Some("https://npm.corp.example/"),
    );
    assert_eq!(
        config.registries_by_prefix.get("other").map(String::as_str),
        Some("https://other.example/"),
    );
}

#[test]
fn rejects_a_registries_map_that_mixes_both_shapes() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  '@acme': https://npm.example.com/\n  https://artifactory.example/: {serverType: artifactory}\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path())
        .expect_err("a mixed registries map must not load")
        .to_string();
    assert!(error.contains("mixes registry declarations"), "{error}");
    assert!(error.contains(r#""@acme""#), "the scope-routed entry is named: {error}");
}

/// A scope routes to a registry, so a URL in that position routes nothing and
/// would sit there inert. It is the declaration shape, half-written.
#[test]
fn rejects_a_url_keyed_registries_entry_written_as_a_string() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "registries:\n  https://npm.example.com/: artifactory\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path())
        .expect_err("a URL-keyed string entry must not load")
        .to_string();
    assert!(error.contains("is a string"), "{error}");
}

/// The inverse of the split a `registries` setting goes through, for the
/// request a pnpr server reads. Every route to one registry lands in its entry.
#[test]
fn rebuilds_the_declarations_from_the_lookups() {
    let mut config = Config::new();
    config.registries_by_scope = BTreeMap::from([
        ("default".to_owned(), "https://registry.npmjs.org/".to_owned()),
        ("@acme".to_owned(), "https://npm.corp.example/".to_owned()),
        ("@acme-internal".to_owned(), "https://npm.corp.example/".to_owned()),
        ("@other".to_owned(), "https://npm.other.example/".to_owned()),
    ]);
    config.registries_by_prefix =
        BTreeMap::from([("work".to_owned(), "https://npm.corp.example/".to_owned())]);
    config.registry_options_by_url = BTreeMap::from([(
        "https://npm.corp.example/".to_owned(),
        RegistryOptions {
            server_type: Some(RegistryServerType::Artifactory),
            supports_time_field: None,
        },
    )]);

    let declarations = config.registry_declarations();
    assert_eq!(
        declarations.get("https://npm.corp.example/"),
        Some(&RegistryDeclaration {
            scopes: Some(vec!["@acme".to_owned(), "@acme-internal".to_owned()]),
            prefix: Some("work".to_owned()),
            server_type: Some(RegistryServerType::Artifactory),
            supports_time_field: None,
            unknown: BTreeMap::new(),
        }),
    );
    assert_eq!(
        declarations.get("https://npm.other.example/"),
        Some(&RegistryDeclaration {
            scopes: Some(vec!["@other".to_owned()]),
            ..RegistryDeclaration::default()
        }),
    );
    // The default registry travels as the request's own `registry` field.
    assert!(!declarations.contains_key("https://registry.npmjs.org/"), "{declarations:?}");
}

/// The resolved view declares every route: the default registry appears as
/// the bare `@` scope — first in the scope list it shares with real scopes —
/// and the built-in `@jsr` scope and `gh` / `npmjs` prefixes are declared
/// unless the user pointed them elsewhere.
#[test]
fn resolved_declarations_declare_every_route() {
    let mut config = Config::new();
    config.registry = "https://npm.corp.example/".to_owned();
    config.registries_by_scope =
        BTreeMap::from([("@acme".to_owned(), "https://npm.corp.example/".to_owned())]);
    config.registries_by_prefix =
        BTreeMap::from([("gh".to_owned(), "https://github.corp.example/".to_owned())]);

    let declarations = config.resolved_registry_declarations();
    assert_eq!(
        declarations.get("https://npm.corp.example/"),
        Some(&RegistryDeclaration {
            scopes: Some(vec!["@".to_owned(), "@acme".to_owned()]),
            ..RegistryDeclaration::default()
        }),
    );
    assert_eq!(
        declarations.get("https://npm.jsr.io/"),
        Some(&RegistryDeclaration {
            scopes: Some(vec!["@jsr".to_owned()]),
            ..RegistryDeclaration::default()
        }),
    );
    // The user's `gh` route wins over the built-in of the same name.
    assert_eq!(
        declarations.get("https://github.corp.example/"),
        Some(&RegistryDeclaration {
            prefix: Some("gh".to_owned()),
            ..RegistryDeclaration::default()
        }),
    );
    assert_eq!(declarations.get("https://npm.pkg.github.com/"), None);
    assert_eq!(
        declarations.get("https://registry.npmjs.org/"),
        Some(&RegistryDeclaration {
            prefix: Some("npmjs".to_owned()),
            ..RegistryDeclaration::default()
        }),
    );
}

/// A declaration map survives the round trip through the lookups it is split
/// into, which is what makes it safe to rebuild one for a pnpr request.
#[test]
fn declarations_round_trip_through_the_lookups() {
    let yaml = r"
registries:
  https://npm.corp.example/:
    serverType: artifactory
    scopes: ['@acme']
    prefix: work
  https://npm.other.example/:
    scopes: ['@other']
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let entries = settings.registries.clone().expect("registries present");
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));

    let rebuilt = config.registry_declarations();
    let original: std::collections::BTreeMap<String, RegistryDeclaration> = entries
        .into_iter()
        .map(|(registry, entry)| match entry {
            RegistryEntry::Declaration(declaration) => (registry, declaration),
            RegistryEntry::ScopeRoute(_) => panic!("declarations only"),
        })
        .collect();
    assert_eq!(rebuilt, original);
}

/// The public registry omits `time` from its abbreviated metadata, so a
/// time-based resolution reads the full document from it. A registry that
/// declares otherwise answers for itself, and paying for the full document at
/// every registry because one of them needs it is the cost this removes.
#[test]
fn a_registry_declaring_the_time_field_needs_no_full_metadata() {
    let yaml = r"
resolutionMode: time-based
registries:
  https://time.example.com/: {supportsTimeField: true}
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));

    assert!(!config.requires_full_metadata_for_registry("https://time.example.com/"));
    assert!(config.requires_full_metadata_for_registry("https://registry.npmjs.org/"));
    // However either side spelled the trailing slash.
    assert!(!config.requires_full_metadata_for_registry("https://time.example.com"));
}

/// A reason that holds whatever the registry serves is not undone by one.
#[test]
fn a_declared_time_field_does_not_waive_the_trust_policy() {
    let yaml = r"
resolutionMode: time-based
trustPolicy: no-downgrade
registries:
  https://time.example.com/: {supportsTimeField: true}
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));

    assert!(config.requires_full_metadata_for_registry("https://time.example.com/"));
}

/// The setting is the answer for every registry that does not describe itself.
#[test]
fn the_time_field_setting_answers_for_an_undeclared_registry() {
    let yaml = r"
resolutionMode: time-based
registrySupportsTimeField: true
registries:
  https://old.example.com/: {supportsTimeField: false}
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));

    assert!(!config.requires_full_metadata_for_registry("https://registry.npmjs.org/"));
    assert!(config.requires_full_metadata_for_registry("https://old.example.com/"));
}

/// The filtered mirror is chosen once for the whole resolver, so it has to
/// cover the registry that asks for the most: one the setting exempts but a
/// declaration does not.
#[test]
fn the_filtered_mirror_covers_a_registry_the_setting_exempts() {
    let yaml = r"
resolutionMode: time-based
registrySupportsTimeField: true
registries:
  https://old.example.com/: {supportsTimeField: false}
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));

    assert!(!config.requires_full_metadata_for_resolution());
    assert!(config.requires_full_metadata_for_registry("https://old.example.com/"));
    assert!(config.requires_filtered_full_metadata());
}

/// Nothing is filtered when no registry can need full metadata.
#[test]
fn no_filtered_mirror_without_a_reason_for_full_metadata() {
    let mut config = Config::new();
    assert!(!config.requires_filtered_full_metadata());
    config.resolution_mode = ResolutionMode::TimeBased;
    assert!(config.requires_filtered_full_metadata());
}

/// The scan that decides whether the file is worth re-reading must never
/// answer "nothing here" for a key there is something to say about, so a
/// top-level key written in a shape it cannot classify still gets collected.
#[test]
fn load_at_collects_issues_from_a_key_it_cannot_scan() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(WORKSPACE_MANIFEST_FILENAME), "{zzzNotASettingZzz: 1}\n").unwrap();

    let settings = WorkspaceSettings::load_at(dir.path())
        .expect("load pnpm-workspace.yaml")
        .expect("pnpm-workspace.yaml is present");

    assert_eq!(settings.key_issues.unrecognized, ["zzzNotASettingZzz"]);
}

/// A `$schema` line is what an editor adds to an otherwise correct file, so
/// it must not be what makes every command re-read it.
#[test]
fn load_at_collects_no_issues_from_a_clean_file_carrying_a_schema_line() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "$schema: https://json.schemastore.org/pnpm-workspace.json\nnodeLinker: hoisted\n",
    )
    .unwrap();

    let settings = WorkspaceSettings::load_at(dir.path())
        .expect("load pnpm-workspace.yaml")
        .expect("pnpm-workspace.yaml is present");

    assert!(settings.key_issues.is_empty(), "unexpected issues: {:?}", settings.key_issues);
}

/// A root mapping may itself be indented, and the whole file is then more
/// indented than column zero without a single key being nested.
#[test]
fn load_at_collects_issues_from_an_indented_root_mapping() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "  zzzNotASettingZzz: 1\n  nodeLinker: hoisted\n",
    )
    .unwrap();

    let settings = WorkspaceSettings::load_at(dir.path())
        .expect("load pnpm-workspace.yaml")
        .expect("pnpm-workspace.yaml is present");

    assert_eq!(settings.key_issues.unrecognized, ["zzzNotASettingZzz"]);
}

/// Indentation is not measurable where a tab stands in for it, so such a file
/// is read rather than judged by its shape.
#[test]
fn load_at_collects_issues_from_a_tab_indented_file() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(WORKSPACE_MANIFEST_FILENAME), "\tzzzNotASettingZzz: 1\n").unwrap();

    let settings = WorkspaceSettings::load_at(dir.path())
        .expect("load pnpm-workspace.yaml")
        .expect("pnpm-workspace.yaml is present");

    assert_eq!(settings.key_issues.unrecognized, ["zzzNotASettingZzz"]);
}

/// A nested key is not a setting of this file, so a catalog naming a package
/// after nothing pnpm knows is not something to report.
#[test]
fn load_at_ignores_keys_nested_under_a_setting() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "catalog:\n  zzzNotASettingZzz: ^1\noverrides:\n  alsoNotASetting: 2\n",
    )
    .unwrap();

    let settings = WorkspaceSettings::load_at(dir.path())
        .expect("load pnpm-workspace.yaml")
        .expect("pnpm-workspace.yaml is present");

    assert!(settings.key_issues.is_empty(), "unexpected issues: {:?}", settings.key_issues);
}

#[test]
fn parses_a_valid_tasks_section_and_applies_it() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        concat!(
            "packages:\n  - packages/*\n",
            "tasks:\n",
            "  build:\n    concurrency: 2\n    dependsOn: ['^build']\n",
            "  test:\n    dependsOn: ['build']\n",
            "  lint: {}\n",
        ),
    )
    .unwrap();

    let settings = WorkspaceSettings::load_at(dir.path())
        .expect("load pnpm-workspace.yaml")
        .expect("pnpm-workspace.yaml is present");
    assert!(settings.key_issues.is_empty());

    let mut config = Config::default();
    settings.apply_to(&mut config, dir.path());
    assert_eq!(
        config.tasks.get("build").unwrap().depends_on.as_deref(),
        Some(&["^build".to_string()][..]),
    );
    assert_eq!(config.tasks.get("build").unwrap().concurrency, Some(2));
    assert_eq!(
        config.tasks.get("test").unwrap().depends_on.as_deref(),
        Some(&["build".to_string()][..]),
    );
    // `lint: {}` declares an explicitly empty dependency list — a different
    // statement from omitting the entry.
    assert_eq!(config.tasks.get("lint").unwrap().depends_on, None);
}

#[test]
fn rejects_an_unknown_task_setting_field() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "packages:\n  - packages/*\ntasks:\n  build:\n    dependson: ['^build']\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path()).unwrap_err();
    assert!(matches!(
        error,
        LoadWorkspaceYamlError::UnknownTaskSettingField { ref task, ref field }
            if task == "build" && field == "dependson"
    ));
}

#[test]
fn rejects_a_depends_on_entry_with_no_task_name() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "packages:\n  - packages/*\ntasks:\n  build:\n    dependsOn: ['^']\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path()).unwrap_err();
    assert!(matches!(
        error,
        LoadWorkspaceYamlError::EmptyTaskDependsOnEntry { ref task, ref entry }
            if task == "build" && entry == "^"
    ));
}

#[test]
fn rejects_zero_task_concurrency() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "packages:\n  - packages/*\ntasks:\n  build:\n    concurrency: 0\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path()).unwrap_err();
    assert!(matches!(
        error,
        LoadWorkspaceYamlError::InvalidTaskConcurrency { ref task, ref concurrency }
            if task == "build" && concurrency == "0"
    ));
}

#[test]
fn rejects_negative_task_concurrency() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "packages:\n  - packages/*\ntasks:\n  build:\n    concurrency: -1\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path()).unwrap_err();
    assert!(matches!(
        error,
        LoadWorkspaceYamlError::InvalidTaskConcurrency { ref task, ref concurrency }
            if task == "build" && concurrency == "-1"
    ));
}

#[test]
fn rejects_fractional_task_concurrency_as_an_invalid_setting() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "packages:\n  - packages/*\ntasks:\n  build:\n    concurrency: 1.5\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path()).unwrap_err();
    assert!(matches!(
        error,
        LoadWorkspaceYamlError::InvalidTaskConcurrency { ref task, ref concurrency }
            if task == "build" && concurrency == "1.5"
    ));
}

#[test]
fn rejects_string_task_concurrency_as_an_invalid_setting() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "packages:\n  - packages/*\ntasks:\n  build:\n    concurrency: '2'\n",
    )
    .unwrap();

    let error = WorkspaceSettings::load_at(dir.path()).unwrap_err();
    assert!(matches!(
        error,
        LoadWorkspaceYamlError::InvalidTaskConcurrency { ref task, ref concurrency }
            if task == "build" && concurrency == r#""2""#
    ));
}

/// The odd suffixes pnpm's `path.join(homedir, rest)` swallows: a doubled
/// separator must not turn the value absolute, and a parent segment must
/// collapse rather than survive into the resolved path.
#[test]
fn expanding_a_home_prefix_joins_the_way_pnpm_does() {
    struct FakeHome;
    impl GetHomeDir for FakeHome {
        fn home_dir() -> Option<PathBuf> {
            Some(PathBuf::from("/home/example"))
        }
    }

    // Compared as paths, not strings: the join separator is `\\` on Windows.
    let home = PathBuf::from("/home/example");
    for (configured, expected) in [
        ("~/bin", home.join("bin")),
        ("~//bin", home.join("bin")),
        ("~/../bin", PathBuf::from("/home").join("bin")),
        ("~/nested/../bin", home.join("bin")),
    ] {
        let mut settings = WorkspaceSettings {
            global_dir: Some(configured.to_string()),
            global_bin_dir: Some(configured.to_string()),
            ..WorkspaceSettings::default()
        };
        settings.expand_global_dir_home_prefixes::<FakeHome>();
        let expected = Some(expected.as_path());
        assert_eq!(
            settings.global_dir.as_deref().map(Path::new),
            expected,
            "globalDir {configured}",
        );
        assert_eq!(
            settings.global_bin_dir.as_deref().map(Path::new),
            expected,
            "globalBinDir {configured}",
        );
    }
}

/// A tilde that names no home-relative path is an ordinary value: pnpm's
/// `/^~[/\\]/` does not match it either.
#[test]
fn a_tilde_without_a_separator_is_left_alone() {
    struct FakeHome;
    impl GetHomeDir for FakeHome {
        fn home_dir() -> Option<PathBuf> {
            Some(PathBuf::from("/home/example"))
        }
    }

    let mut settings = WorkspaceSettings {
        global_dir: Some("~backup/global".to_string()),
        global_bin_dir: Some("bin/~/nested".to_string()),
        ..WorkspaceSettings::default()
    };
    settings.expand_global_dir_home_prefixes::<FakeHome>();
    assert_eq!(settings.global_dir.as_deref(), Some("~backup/global"));
    assert_eq!(settings.global_bin_dir.as_deref(), Some("bin/~/nested"));
}
