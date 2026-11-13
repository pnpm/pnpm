use super::{
    AuditLevel, ColorMode, Config, EnvVar, LoadWorkspaceYamlError, NodeLinker, Path,
    WORKSPACE_MANIFEST_FILENAME, WorkspaceSettings, assert_eq,
};
use crate::{SettingsRead, workspace_yaml::parse_settings};
use std::fmt::Write as _;

struct Env;

impl EnvVar for Env {
    fn var(name: &str) -> Option<String> {
        match name {
            "PNPM_TEST_LINKER" => Some("hoisted".to_owned()),
            "PNPM_TEST_IGNORE_SCRIPTS" => Some("true".to_owned()),
            "PNPM_TEST_HOST" => Some("internal.example.com".to_owned()),
            "PNPM_TEST_REGISTRY" => Some("https://registry.example.com/".to_owned()),
            "PNPM_TEST_TOKEN" => Some("s3cr3t".to_owned()),
            _ => None,
        }
    }
}

#[test]
fn a_fallback_names_an_enum_variant() {
    let settings =
        parse_settings::<Env, WorkspaceSettings>("nodeLinker: ${PNPM_TEST_UNSET:-isolated}\n")
            .unwrap();

    assert_eq!(settings.node_linker, Some(NodeLinker::Isolated.into()));
}

#[test]
fn an_environment_value_wins_over_the_fallback() {
    let settings =
        parse_settings::<Env, WorkspaceSettings>("nodeLinker: ${PNPM_TEST_LINKER:-isolated}\n")
            .unwrap();

    assert_eq!(settings.node_linker, Some(NodeLinker::Hoisted.into()));
}

#[test]
fn a_placeholder_sets_a_boolean_setting() {
    let set = parse_settings::<Env, WorkspaceSettings>(
        "ignoreScripts: ${PNPM_TEST_IGNORE_SCRIPTS:-false}\n",
    )
    .unwrap();
    let unset =
        parse_settings::<Env, WorkspaceSettings>("ignoreScripts: ${PNPM_TEST_UNSET:-false}\n")
            .unwrap();

    assert_eq!(set.ignore_scripts, Some(true));
    assert_eq!(unset.ignore_scripts, Some(false));
}

#[test]
fn a_placeholder_sets_a_numeric_setting() {
    let settings =
        parse_settings::<Env, WorkspaceSettings>("networkConcurrency: ${PNPM_TEST_UNSET:-4}\n")
            .unwrap();

    assert_eq!(settings.network_concurrency, Some(4));
}

#[test]
fn a_placeholder_sets_a_setting_that_also_accepts_a_boolean() {
    let settings =
        parse_settings::<Env, WorkspaceSettings>("color: ${PNPM_TEST_UNSET:-never}\n").unwrap();

    assert_eq!(settings.color, Some(ColorMode::Never));
}

#[test]
fn a_placeholder_sets_a_setting_of_a_nested_section() {
    let settings =
        parse_settings::<Env, WorkspaceSettings>("audit:\n  level: ${PNPM_TEST_UNSET:-critical}\n")
            .unwrap();

    assert_eq!(settings.audit.and_then(|audit| audit.level), Some(AuditLevel::Critical));
}

/// A repository-controlled file must not resolve an environment variable
/// into a request destination, so the placeholder has to survive parsing for
/// `substitute_env_untrusted` to drop it. `PNPM_TEST_HOST` expands to a bare
/// token, which every setting outside these keys takes.
#[test]
fn a_request_destination_keeps_its_placeholder() {
    let settings = parse_settings::<Env, WorkspaceSettings>(
        "nodeLinker: ${PNPM_TEST_UNSET:-isolated}
registry: ${PNPM_TEST_HOST}
httpsProxy: ${PNPM_TEST_HOST}
pnprServer: ${PNPM_TEST_HOST}
",
    )
    .unwrap();

    assert_eq!(settings.registry.as_deref(), Some("${PNPM_TEST_HOST}"));
    assert_eq!(settings.https_proxy.as_deref(), Some("${PNPM_TEST_HOST}"));
    assert_eq!(settings.pnpr_server.as_deref(), Some("${PNPM_TEST_HOST}"));
}

#[test]
fn a_value_that_expands_to_a_url_keeps_its_placeholder() {
    let settings = parse_settings::<Env, WorkspaceSettings>(
        "nodeLinker: ${PNPM_TEST_UNSET:-isolated}\nstoreDir: ${PNPM_TEST_REGISTRY}\n",
    )
    .unwrap();

    assert_eq!(settings.store_dir.as_deref(), Some("${PNPM_TEST_REGISTRY}"));
}

#[test]
fn a_placeholder_with_no_value_and_no_fallback_is_reported_as_written() {
    let error =
        parse_settings::<Env, WorkspaceSettings>("nodeLinker: ${PNPM_TEST_UNSET}\n").unwrap_err();

    assert!(error.to_string().contains("${PNPM_TEST_UNSET}"), "unexpected error: {error}");
}

/// A mistyped variable name puts whatever the environment holds under that
/// name into the setting, and a build log must not be where it turns up.
#[test]
fn an_expansion_that_names_no_variant_stays_out_of_the_error() {
    let error =
        parse_settings::<Env, WorkspaceSettings>("nodeLinker: ${PNPM_TEST_TOKEN}\n").unwrap_err();

    let message = error.to_string();
    assert!(message.contains("invalid environment-expanded value"), "unexpected: {message}");
    assert!(!message.contains("s3cr3t"), "the error names the expanded value");
}

#[test]
fn a_document_without_placeholders_keeps_the_error_of_the_text() {
    let error = parse_settings::<Env, WorkspaceSettings>("nodeLinker: bogus\n").unwrap_err();

    let message = error.to_string();
    assert!(message.contains("bogus"), "unexpected error: {message}");
    assert!(message.contains("line 1"), "unexpected error: {message}");
}

/// The second read resolves the placeholder, so what it cannot read is the
/// other setting — which the first read already located.
#[test]
fn an_error_beside_a_resolved_placeholder_keeps_its_location() {
    let error = parse_settings::<Env, WorkspaceSettings>(
        "hoist: notabool\nnodeLinker: ${PNPM_TEST_UNSET:-isolated}\n",
    )
    .unwrap_err();

    let message = error.to_string();
    assert!(message.contains("line 1"), "unexpected error: {message}");
    assert!(!message.contains("invalid environment-expanded value"), "unexpected: {message}");
}

/// A setting that takes free text reads as written, so the second read has
/// no reason to resolve its placeholder and leaves it to the substitution
/// that knows which layer the file came from.
#[test]
fn a_placeholder_a_setting_can_hold_is_left_for_the_substitution() {
    let mut settings = parse_settings::<Env, WorkspaceSettings>(
        "ignoreScripts: ${PNPM_TEST_UNSET:-false}
cacheDir: ${PNPM_TEST_UNSET:-cache}
userAgent: ${PNPM_TEST_HOST}
",
    )
    .unwrap();

    assert_eq!(settings.ignore_scripts, Some(false));
    assert_eq!(settings.cache_dir.as_deref(), Some("${PNPM_TEST_UNSET:-cache}"));
    assert_eq!(settings.user_agent.as_deref(), Some("${PNPM_TEST_HOST}"));

    settings.substitute_env_untrusted::<Env>().unwrap();

    assert_eq!(settings.cache_dir.as_deref(), Some("cache"));
}

/// Placeholders the document reads without are decided in groups, so a file
/// may carry any number of them — in comments here — without exhausting what
/// the second read is allowed to do, and the one setting that needs its own
/// still resolves.
#[test]
fn placeholders_the_document_reads_without_do_not_crowd_out_the_one_that_counts() {
    let padding = (0..500).fold(String::new(), |mut padding, index| {
        let _ = writeln!(padding, "# ${{PNPM_TEST_UNSET:-pad{index}}}");
        padding
    });

    let settings = parse_settings::<Env, WorkspaceSettings>(&format!(
        "nodeLinker: ${{PNPM_TEST_UNSET:-isolated}}\n{padding}",
    ))
    .unwrap();

    assert_eq!(settings.node_linker, Some(NodeLinker::Isolated.into()));
}

/// The second read must not turn a quoted scalar into the value it spells,
/// which would let a file mean something it does not say.
#[test]
fn a_quoted_scalar_beside_a_resolved_placeholder_stays_quoted() {
    parse_settings::<Env, WorkspaceSettings>(
        r#"ignoreScripts: ${PNPM_TEST_UNSET:-false}
linkWorkspacePackages: "false"
"#,
    )
    .expect_err("a quoted false is not a boolean");
}

/// The second read must leave every scalar it did not resolve exactly as the
/// file spells it.
#[test]
fn a_scalar_beside_a_resolved_placeholder_keeps_its_text() {
    let settings = parse_settings::<Env, WorkspaceSettings>(
        "nodeLinker: ${PNPM_TEST_UNSET:-isolated}
nodeVersion: 20.10
",
    )
    .unwrap();

    assert_eq!(settings.node_version.as_deref(), Some("20.10"));
}

/// An unfinished `${` is text, not the opening of a placeholder, so it must
/// not swallow the next one along with it.
#[test]
fn an_unfinished_placeholder_does_not_hide_the_next_one() {
    let settings = parse_settings::<Env, WorkspaceSettings>(
        "# example ${UNFINISHED
ignoreScripts: ${PNPM_TEST_UNSET:-false}
",
    )
    .unwrap();

    assert_eq!(settings.ignore_scripts, Some(false));
}

/// The first read stops at the placeholder, which is not the problem, so the
/// setting the file cannot mean is the one to name — wherever it sits.
#[test]
fn an_error_after_a_resolved_placeholder_names_the_setting_at_fault() {
    let error = parse_settings::<Env, WorkspaceSettings>(
        "nodeLinker: ${PNPM_TEST_UNSET:-isolated}\nhoist: notabool\n",
    )
    .unwrap_err();

    let message = error.to_string();
    assert!(message.contains("notabool"), "unexpected error: {message}");
    assert!(message.contains("line 2"), "unexpected error: {message}");
}

/// Settings that take a boolean and so cannot hold a placeholder as text.
/// Deciding one costs about two reads of the document, and a file driving
/// this many of them from the environment has to stay within what the second
/// read is allowed to do.
const BOOLEAN_SETTINGS: &[&str] = &[
    "bail",
    "progress",
    "updateNotifier",
    "embedReadme",
    "ignoreWorkspaceRootCheck",
    "optional",
    "packageLock",
    "pending",
    "recursiveInstall",
    "reverse",
    "stream",
    "aggregateOutput",
    "reporterHidePrefix",
    "useStderr",
    "ignoreWorkspace",
    "shellEmulator",
    "skipManifestObfuscation",
    "sort",
    "useBetaCli",
    "hoist",
    "shamefullyHoist",
    "nodeExperimentalPackageMap",
    "symlink",
    "enableGlobalVirtualStore",
    "virtualStoreOnly",
    "enableModulesDir",
    "lockfile",
    "preferFrozenLockfile",
    "frozenLockfile",
    "deployAllFiles",
    "forceLegacyDeploy",
    "sharedWorkspaceLockfile",
    "gitBranchLockfile",
    "mergeGitBranchLockfiles",
    "offline",
    "preferOffline",
    "lockfileIncludeTarballUrl",
    "autoInstallPeers",
];

#[test]
fn every_placeholder_of_a_document_driven_from_the_environment_resolves() {
    let document = BOOLEAN_SETTINGS
        .iter()
        .fold(String::new(), |mut document, setting| {
            let _ = writeln!(document, "{setting}: ${{PNPM_TEST_UNSET:-true}}");
            document
        });

    let settings = parse_settings::<Env, WorkspaceSettings>(&document).unwrap();

    assert_eq!(settings.bail, Some(true));
    assert_eq!(settings.auto_install_peers, Some(true));
}

#[test]
fn flow_and_fallback_registry_placeholders_are_refused_in_untrusted_settings() {
    for (yaml, expected_setting) in [
        ("{registry: https://example.com/${PNPM_TEST_HOST}/}\n", "registry"),
        ("{registry: https://example.com/${PNPM_TEST_UNSET}/}\n", "registry"),
        ("{registry: https://example.com/${PNPM_TEST_REGISTRY}/}\n", "registry"),
        ("{registry: https://example.com/${PNPM_TEST_UNSET:-a}/}\n", "registry"),
        ("registry: ${PNPM_TEST_HOST:-http://a: b}\n", "registry"),
        ("registry: ${PNPM_TEST_UNSET:-http://a:b}\n", "registry"),
        (
            "{registries: {default: https://example.com/${PNPM_TEST_HOST}/}}\n",
            "registries['default']",
        ),
        (
            "{registries: {'@work': https://example.com/${PNPM_TEST_HOST}/}}\n",
            "registries['@work']",
        ),
        (
            "{registries: {https://${PNPM_TEST_HOST}/: {scopes: ['@work']}}}\n",
            "registries['https://${PNPM_TEST_HOST}/']",
        ),
        (
            "{registries: {https://${PNPM_TEST_HOST}/: {serverType: artifactory}}}\n",
            "registries['https://${PNPM_TEST_HOST}/']",
        ),
        (
            "{namedRegistries: {work: https://example.com/${PNPM_TEST_HOST}/}}\n",
            "namedRegistries['work']",
        ),
    ] {
        let mut settings = parse_settings::<Env, WorkspaceSettings>(yaml).unwrap();
        let error = settings.substitute_env_untrusted::<Env>().unwrap_err();
        assert!(
            matches!(
                &error,
                LoadWorkspaceYamlError::RegistryEnvNotExpanded { setting, file }
                    if setting == expected_setting && file == WORKSPACE_MANIFEST_FILENAME
            ),
            "yaml={yaml:?} produced {error:?}",
        );
    }
}

#[test]
fn flow_proxy_and_request_header_placeholders_are_dropped_in_untrusted_settings() {
    let mut settings = parse_settings::<Env, WorkspaceSettings>(
        "{pnprServer: https://${PNPM_TEST_HOST}/pnpr/, httpsProxy: http://${PNPM_TEST_HOST}:8080/, httpProxy: http://${PNPM_TEST_HOST}:8081/, proxy: http://${PNPM_TEST_HOST}:8082/, userAgent: agent/${PNPM_TEST_TOKEN}, noProxy: ${PNPM_TEST_IGNORE_SCRIPTS}, noproxy: ${PNPM_TEST_HOST}}\n",
    )
    .unwrap();

    let warnings = settings.substitute_env_untrusted::<Env>().unwrap();
    let ignored: Vec<&str> = warnings
        .iter()
        .filter_map(|warning| warning.strip_prefix(r#"Ignored ""#)?.split_once('"'))
        .map(|(setting, _)| setting)
        .collect();

    assert_eq!(
        ignored,
        ["pnprServer", "httpsProxy", "httpProxy", "proxy", "userAgent", "noProxy", "noproxy"],
    );
    assert_eq!(settings.pnpr_server, None);
    assert_eq!(settings.https_proxy, None);
    assert_eq!(settings.http_proxy, None);
    assert_eq!(settings.proxy, None);
    assert_eq!(settings.user_agent, None);
    assert_eq!(settings.no_proxy, None);
    assert_eq!(settings.noproxy, None);
}

#[test]
fn flow_placeholders_expand_in_trusted_settings_and_allowed_workspace_fields() {
    let mut trusted = parse_settings::<Env, WorkspaceSettings>(
        "{registry: https://${PNPM_TEST_HOST}/npm/, httpsProxy: http://${PNPM_TEST_HOST}:8080/, shamefullyHoist: ${PNPM_TEST_IGNORE_SCRIPTS}}\n",
    )
    .unwrap();
    trusted.substitute_env_trusted::<Env>();
    assert_eq!(trusted.registry.as_deref(), Some("https://internal.example.com/npm/"));
    assert_eq!(trusted.https_proxy.as_deref(), Some("http://internal.example.com:8080/"));
    assert_eq!(trusted.shamefully_hoist, Some(true));

    let mut untrusted = parse_settings::<Env, WorkspaceSettings>(
        "{shamefullyHoist: ${PNPM_TEST_IGNORE_SCRIPTS}, nodeLinker: ${PNPM_TEST_LINKER:-isolated}, storeDir: ${PNPM_TEST_HOST}/store}\n",
    )
    .unwrap();
    let warnings = untrusted.substitute_env_untrusted::<Env>().unwrap();
    assert!(warnings.is_empty());
    assert_eq!(untrusted.shamefully_hoist, Some(true));
    assert_eq!(untrusted.node_linker, Some(NodeLinker::Hoisted.into()));
    assert_eq!(untrusted.store_dir.as_deref(), Some("internal.example.com/store"));

    let mut config = Config::new();
    untrusted.apply_to(&mut config, Path::new("/workspace"));
    assert!(config.shamefully_hoist);
}

#[test]
fn config_dependency_and_local_include_refuse_flow_registry_placeholders() {
    let dep_path = Path::new("/store/config-dep/pnpm-config.yaml");
    let dep_error = WorkspaceSettings::read_config_dependency_entry::<Env>(
        "{registry: https://example.com/${PNPM_TEST_HOST}/}\n",
        dep_path,
    )
    .unwrap_err();
    assert!(matches!(
        &dep_error,
        LoadWorkspaceYamlError::RegistryEnvNotExpanded { setting, file }
            if setting == "registry" && file == &dep_path.display().to_string()
    ));

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("pnpm-workspace.yaml"),
        "include:\n  ./reg.yaml:\n    - registry\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("reg.yaml"),
        "{registry: \"https://example.com/${PNPM_TEST_HOST}/\"}\n",
    )
    .unwrap();
    let document = pnpm_workspace::read_workspace_manifest_document(dir.path()).unwrap().unwrap();
    let mut merged = WorkspaceSettings::read_document(document, SettingsRead::All, |text, path| {
        parse_settings::<Env, WorkspaceSettings>(text)
            .map_err(|source| LoadWorkspaceYamlError::ParseYaml {
                path: path.to_path_buf(),
                source,
            })
    })
    .unwrap();
    let error = merged.substitute_env_untrusted::<Env>().unwrap_err();
    assert!(matches!(
        &error,
        LoadWorkspaceYamlError::RegistryEnvNotExpanded { setting, .. } if setting == "registry"
    ));
}

#[test]
fn sentinel_prefix_avoids_colliding_with_document_or_environment_text() {
    struct CollidingEnv;
    impl EnvVar for CollidingEnv {
        fn var(name: &str) -> Option<String> {
            match name {
                "HOIST" => Some("true".to_owned()),
                "COLLIDE" => Some("pnpmplaceholder0_value".to_owned()),
                _ => None,
            }
        }
    }

    let settings = parse_settings::<CollidingEnv, WorkspaceSettings>(
        "{shamefullyHoist: ${HOIST}, storeDir: pnpmplaceholder0_/${COLLIDE}}\n",
    )
    .unwrap();
    assert_eq!(settings.shamefully_hoist, Some(true));
    assert_eq!(settings.store_dir.as_deref(), Some("pnpmplaceholder0_/${COLLIDE}"));
}
