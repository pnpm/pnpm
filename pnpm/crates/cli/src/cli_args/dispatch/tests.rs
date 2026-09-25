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

fn load_calls(store_use: super::configuration::StoreUse, config_dependencies: bool) -> Vec<bool> {
    use pnpm_workspace_state::ConfigDependency;

    let mut calls = Vec::new();
    store_use
        .load(|place_store| {
            calls.push(place_store);
            let config_dependencies = config_dependencies.then(|| {
                [(
                    "plugin".to_string(),
                    ConfigDependency::VersionWithIntegrity("1.0.0+sha512-x".into()),
                )]
                .into()
            });
            let config = pnpm_config::Config {
                skip_store_dir_resolution: !place_store,
                config_dependencies,
                ..Default::default()
            };
            Ok::<_, std::convert::Infallible>(config)
        })
        .expect("the loader is infallible");
    calls
}

#[test]
fn store_use_decides_where_the_config_load_places_the_store() {
    use super::configuration::StoreUse;

    assert_eq!(load_calls(StoreUse::Opens, false), [true]);
    assert_eq!(load_calls(StoreUse::Opens, true), [true]);
    assert_eq!(load_calls(StoreUse::Never, false), [false]);
    assert_eq!(load_calls(StoreUse::Never, true), [false]);
    assert_eq!(load_calls(StoreUse::ConfigDependencies, false), [false]);
    assert_eq!(load_calls(StoreUse::ConfigDependencies, true), [false, true]);
}
