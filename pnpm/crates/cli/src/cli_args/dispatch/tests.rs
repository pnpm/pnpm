use crate::cli_args::{
    cli_command::CliArgs,
    dispatch::routing::{
        json_error_message,
        prints_json_errors,
    },
    pack::PACK_ERROR_CONTEXT,
};
use clap::Parser;
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

#[test]
fn structured_stdout_commands_route_reporter_to_stderr() {
    for argv in [
        ["pacquet", "stage", "publish", "--json"].as_slice(),
        ["pacquet", "stage", "list", "--json"].as_slice(),
        ["pacquet", "pipeline", "--dry-run", "--json"].as_slice(),
        ["pacquet", "pipeline", "check", "--dry-run", "--json"].as_slice(),
        ["pacquet", "outdated", "--json"].as_slice(),
        ["pacquet", "outdated", "--format", "json"].as_slice(),
        ["pacquet", "-r", "run", "--dry-run", "--json", "test"].as_slice(),
        ["pacquet", "audit", "--json"].as_slice(),
        ["pacquet", "list", "--json"].as_slice(),
        ["pacquet", "list", "--parseable"].as_slice(),
        ["pacquet", "ll", "--json"].as_slice(),
        ["pacquet", "why", "foo", "--json"].as_slice(),
        ["pacquet", "why", "foo", "--parseable"].as_slice(),
        ["pacquet", "licenses", "list", "--json"].as_slice(),
        ["pacquet", "fund", "--json"].as_slice(),
        ["pacquet", "peers", "--json"].as_slice(),
        ["pacquet", "doctor", "--json"].as_slice(),
        ["pacquet", "pkg", "get", "name"].as_slice(),
        ["pacquet", "view", "foo", "--json"].as_slice(),
        ["pacquet", "search", "foo", "--json"].as_slice(),
        ["pacquet", "access", "list", "packages", "--json"].as_slice(),
        ["pacquet", "token", "list", "--json"].as_slice(),
        ["pacquet", "token", "list", "--parseable"].as_slice(),
        ["pacquet", "team", "ls", "@scope:devs", "--json"].as_slice(),
        ["pacquet", "profile", "get", "--json"].as_slice(),
        ["pacquet", "publish", "--json"].as_slice(),
        ["pacquet", "pack", "--json"].as_slice(),
        ["pacquet", "version", "--json"].as_slice(),
        ["pacquet", "cache", "path"].as_slice(),
        ["pacquet", "cache", "view", "foo"].as_slice(),
        ["pacquet", "cat-index", "foo@1.0.0"].as_slice(),
    ] {
        let parsed = CliArgs::try_parse_from(argv).unwrap_or_else(|err| panic!("{argv:?}: {err}"));
        assert!(
            parsed.command.uses_stderr_reporter(),
            "{argv:?} must route reporter output to stderr",
        );
    }

    for argv in [
        ["pacquet", "stage", "publish"].as_slice(),
        ["pacquet", "pipeline", "--dry-run"].as_slice(),
        ["pacquet", "pipeline", "--json"].as_slice(),
        ["pacquet", "outdated"].as_slice(),
        ["pacquet", "outdated", "--format", "list"].as_slice(),
        ["pacquet", "outdated", "--no-table"].as_slice(),
        ["pacquet", "-r", "run", "--dry-run", "test"].as_slice(),
        ["pacquet", "run", "--json", "test"].as_slice(),
        ["pacquet", "audit"].as_slice(),
        ["pacquet", "list"].as_slice(),
        ["pacquet", "why", "foo"].as_slice(),
        ["pacquet", "licenses", "list"].as_slice(),
        ["pacquet", "fund"].as_slice(),
        ["pacquet", "peers"].as_slice(),
        ["pacquet", "doctor"].as_slice(),
        ["pacquet", "pkg", "set", "--json", "a=1"].as_slice(),
        ["pacquet", "view", "foo"].as_slice(),
        ["pacquet", "search", "foo"].as_slice(),
        ["pacquet", "publish"].as_slice(),
        ["pacquet", "pack"].as_slice(),
        ["pacquet", "version"].as_slice(),
        ["pacquet", "cache", "list"].as_slice(),
    ] {
        let parsed = CliArgs::try_parse_from(argv).unwrap_or_else(|err| panic!("{argv:?}: {err}"));
        assert!(
            !parsed.command.uses_stderr_reporter(),
            "{argv:?} must keep default reporter stdout routing",
        );
    }
}

#[test]
fn stage_json_prints_structured_json_errors() {
    for (argv, expected) in [
        (["pacquet", "stage", "publish", "--json"].as_slice(), true),
        (["pacquet", "stage", "list", "--json"].as_slice(), true),
        (["pacquet", "publish", "--json"].as_slice(), true),
        (["pacquet", "view", "foo", "--json"].as_slice(), true),
        (["pacquet", "pack", "--json"].as_slice(), true),
        (["pacquet", "stage", "publish"].as_slice(), false),
        (["pacquet", "pipeline", "--dry-run", "--json"].as_slice(), false),
        (["pacquet", "outdated", "--json"].as_slice(), false),
    ] {
        let parsed = CliArgs::try_parse_from(argv).unwrap_or_else(|err| panic!("{argv:?}: {err}"));
        assert_eq!(prints_json_errors(&parsed.command), expected, "{argv:?}");
    }
}
