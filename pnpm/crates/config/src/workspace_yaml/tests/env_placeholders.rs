use super::{AuditLevel, ColorMode, EnvVar, NodeLinker, assert_eq};
use crate::workspace_yaml::settings::parse_settings;
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
    let settings = parse_settings::<Env>("nodeLinker: ${PNPM_TEST_UNSET:-isolated}\n").unwrap();

    assert_eq!(settings.node_linker, Some(NodeLinker::Isolated));
}

#[test]
fn an_environment_value_wins_over_the_fallback() {
    let settings = parse_settings::<Env>("nodeLinker: ${PNPM_TEST_LINKER:-isolated}\n").unwrap();

    assert_eq!(settings.node_linker, Some(NodeLinker::Hoisted));
}

#[test]
fn a_placeholder_sets_a_boolean_setting() {
    let set = parse_settings::<Env>("ignoreScripts: ${PNPM_TEST_IGNORE_SCRIPTS:-false}\n").unwrap();
    let unset = parse_settings::<Env>("ignoreScripts: ${PNPM_TEST_UNSET:-false}\n").unwrap();

    assert_eq!(set.ignore_scripts, Some(true));
    assert_eq!(unset.ignore_scripts, Some(false));
}

#[test]
fn a_placeholder_sets_a_numeric_setting() {
    let settings = parse_settings::<Env>("networkConcurrency: ${PNPM_TEST_UNSET:-4}\n").unwrap();

    assert_eq!(settings.network_concurrency, Some(4));
}

#[test]
fn a_placeholder_sets_a_setting_that_also_accepts_a_boolean() {
    let settings = parse_settings::<Env>("color: ${PNPM_TEST_UNSET:-never}\n").unwrap();

    assert_eq!(settings.color, Some(ColorMode::Never));
}

#[test]
fn a_placeholder_sets_a_setting_of_a_nested_section() {
    let settings =
        parse_settings::<Env>("audit:\n  level: ${PNPM_TEST_UNSET:-critical}\n").unwrap();

    assert_eq!(settings.audit.and_then(|audit| audit.level), Some(AuditLevel::Critical));
}

/// A repository-controlled file must not resolve an environment variable
/// into a request destination, so the placeholder has to survive parsing for
/// `substitute_env_untrusted` to drop it. `PNPM_TEST_HOST` expands to a bare
/// token, which every setting outside these keys takes.
#[test]
fn a_request_destination_keeps_its_placeholder() {
    let settings = parse_settings::<Env>(
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
    let settings = parse_settings::<Env>(
        "nodeLinker: ${PNPM_TEST_UNSET:-isolated}\nstoreDir: ${PNPM_TEST_REGISTRY}\n",
    )
    .unwrap();

    assert_eq!(settings.store_dir.as_deref(), Some("${PNPM_TEST_REGISTRY}"));
}

#[test]
fn a_placeholder_with_no_value_and_no_fallback_is_reported_as_written() {
    let error = parse_settings::<Env>("nodeLinker: ${PNPM_TEST_UNSET}\n").unwrap_err();

    assert!(error.to_string().contains("${PNPM_TEST_UNSET}"), "unexpected error: {error}");
}

/// A mistyped variable name puts whatever the environment holds under that
/// name into the setting, and a build log must not be where it turns up.
#[test]
fn an_expansion_that_names_no_variant_stays_out_of_the_error() {
    let error = parse_settings::<Env>("nodeLinker: ${PNPM_TEST_TOKEN}\n").unwrap_err();

    let message = error.to_string();
    assert!(message.contains("invalid environment-expanded value"), "unexpected: {message}");
    assert!(!message.contains("s3cr3t"), "the error names the expanded value");
}

#[test]
fn a_document_without_placeholders_keeps_the_error_of_the_text() {
    let error = parse_settings::<Env>("nodeLinker: bogus\n").unwrap_err();

    let message = error.to_string();
    assert!(message.contains("bogus"), "unexpected error: {message}");
    assert!(message.contains("line 1"), "unexpected error: {message}");
}

/// The second read resolves the placeholder, so what it cannot read is the
/// other setting — which the first read already located.
#[test]
fn an_error_beside_a_resolved_placeholder_keeps_its_location() {
    let error =
        parse_settings::<Env>("hoist: notabool\nnodeLinker: ${PNPM_TEST_UNSET:-isolated}\n")
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
    let mut settings = parse_settings::<Env>(
        "ignoreScripts: ${PNPM_TEST_UNSET:-false}
cacheDir: ${PNPM_TEST_UNSET:-cache}
userAgent: ${PNPM_TEST_HOST}
",
    )
    .unwrap();

    assert_eq!(settings.ignore_scripts, Some(false));
    assert_eq!(settings.cache_dir.as_deref(), Some("${PNPM_TEST_UNSET:-cache}"));
    assert_eq!(settings.user_agent.as_deref(), Some("${PNPM_TEST_HOST}"));

    settings.substitute_env_untrusted::<Env>();

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

    let settings =
        parse_settings::<Env>(&format!("nodeLinker: ${{PNPM_TEST_UNSET:-isolated}}\n{padding}"))
            .unwrap();

    assert_eq!(settings.node_linker, Some(NodeLinker::Isolated));
}

/// The second read must not turn a quoted scalar into the value it spells,
/// which would let a file mean something it does not say.
#[test]
fn a_quoted_scalar_beside_a_resolved_placeholder_stays_quoted() {
    parse_settings::<Env>(
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
    let settings = parse_settings::<Env>(
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
    let settings = parse_settings::<Env>(
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
    let error =
        parse_settings::<Env>("nodeLinker: ${PNPM_TEST_UNSET:-isolated}\nhoist: notabool\n")
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

    let settings = parse_settings::<Env>(&document).unwrap();

    assert_eq!(settings.bail, Some(true));
    assert_eq!(settings.auto_install_peers, Some(true));
}
