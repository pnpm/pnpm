use super::{
    EnvArgs,
    EnvError,
    EnvSubcommand,
};

fn args(params: &[&str]) -> EnvArgs {
    EnvArgs {
        params: params
            .iter()
            .map(ToString::to_string)
            .collect(),
    }
}

#[test]
fn a_bare_env_asks_for_a_subcommand() {
    let error = args(&[]).subcommand().unwrap_err();
    assert!(matches!(error, EnvError::NoSubcommand), "{error:?}");
}

#[test]
fn an_unrecognized_subcommand_is_rejected() {
    let error = args(&["install"]).subcommand().unwrap_err();
    assert!(matches!(error, EnvError::UnknownSubcommand), "{error:?}");
}

#[test]
fn list_takes_an_optional_selector() {
    for (params, expected) in [
        (vec!["list"], None),
        (vec!["ls"], None),
        (vec!["list", "  "], None),
        (vec!["list", "lts"], Some("lts".to_string())),
        (vec!["ls", "rc/24"], Some("rc/24".to_string())),
    ] {
        let subcommand = args(&params).subcommand().unwrap();
        let EnvSubcommand::List { version_spec } = subcommand else {
            panic!("expected a `list` subcommand for {params:?}");
        };
        assert_eq!(version_spec, expected, "{params:?}");
    }
}
