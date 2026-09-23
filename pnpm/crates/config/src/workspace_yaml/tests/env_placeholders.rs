use super::{AuditLevel, ColorMode, EnvVar, NodeLinker, assert_eq};
use crate::workspace_yaml::settings::parse_settings;

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

/// A setting that takes free text keeps it: the yaml scalar a resolved
/// placeholder spells is read against the setting it lands in, as a value
/// written out in the file would be.
#[test]
fn an_expansion_beside_a_typed_one_stays_text_where_the_setting_takes_text() {
    let settings = parse_settings::<Env>(
        "ignoreScripts: ${PNPM_TEST_UNSET:-false}
nodeVersion: ${PNPM_TEST_UNSET:-22}
userAgent: ${PNPM_TEST_UNSET:-true}
",
    )
    .unwrap();

    assert_eq!(settings.ignore_scripts, Some(false));
    assert_eq!(settings.node_version.as_deref(), Some("22"));
    assert_eq!(settings.user_agent.as_deref(), Some("true"));
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
