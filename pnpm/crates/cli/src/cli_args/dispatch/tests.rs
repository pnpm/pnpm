use crate::cli_args::{dispatch::routing::json_error_message, pack::PACK_ERROR_CONTEXT};
use miette::Diagnostic;

#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
#[display("canonical publish failure")]
#[diagnostic(code(ERR_PNPM_TEST_JSON_ERROR))]
struct CanonicalError {
    #[error(source)]
    source: SensitiveCause,
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
#[display("registry response included token=secret")]
struct SensitiveCause;

#[test]
fn json_error_message_omits_nested_causes() {
    let error = miette::Report::new(CanonicalError { source: SensitiveCause });
    let message = json_error_message(&error);

    assert_eq!(message, "canonical publish failure");
    assert!(!message.contains("token=secret"));
}

#[test]
fn json_error_message_unwraps_pack_context() {
    let error =
        miette::Report::new(CanonicalError { source: SensitiveCause }).wrap_err(PACK_ERROR_CONTEXT);

    assert_eq!(json_error_message(&error), "canonical publish failure");
}

#[test]
fn script_commands_place_the_store_only_for_config_dependencies() {
    use super::configuration::StoreUse;
    use pnpm_config::Config;
    use pnpm_workspace_state::ConfigDependency;

    let mut config = Config::default();
    assert!(StoreUse::Opens.needs_store_placed(&config));
    assert!(!StoreUse::Never.needs_store_placed(&config));
    assert!(!StoreUse::ConfigDependencies.needs_store_placed(&config));

    config.config_dependencies = Some(
        [("plugin".to_string(), ConfigDependency::VersionWithIntegrity("1.0.0+sha512-x".into()))]
            .into(),
    );
    assert!(StoreUse::ConfigDependencies.needs_store_placed(&config));
    assert!(!StoreUse::Never.needs_store_placed(&config));
}
