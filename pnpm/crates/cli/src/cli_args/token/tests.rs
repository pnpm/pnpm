use super::{generate_token_ids, registry::TokenItem};
use crate::cli_args::{CliArgs, cli_command::CliCommand};
use clap::Parser;

#[test]
fn generates_unique_shortest_ids() {
    let mut tokens = vec![
        TokenItem {
            key: "abcdef111111".to_string(),
            token: "abcdef".to_string(),
            id: None,
            name: None,
            created: None,
            readonly: false,
            cidr_whitelist: None,
        },
        TokenItem {
            key: "abcdef222222".to_string(),
            token: "abcdef".to_string(),
            id: None,
            name: None,
            created: None,
            readonly: false,
            cidr_whitelist: None,
        },
        TokenItem {
            key: "123456789012".to_string(),
            token: "123456".to_string(),
            id: None,
            name: None,
            created: None,
            readonly: false,
            cidr_whitelist: None,
        },
    ];

    generate_token_ids(&mut tokens);

    assert_eq!(tokens[0].id.as_deref(), Some("abcdef1"));
    assert_eq!(tokens[1].id.as_deref(), Some("abcdef2"));
    assert_eq!(tokens[2].id.as_deref(), Some("123456"));
}

#[test]
fn parses_token_cli_args() {
    let args = CliArgs::try_parse_from(["pacquet", "token", "list", "--json"]).unwrap();
    let CliCommand::Token(token_args) = args.command else {
        panic!("expected Token command");
    };
    assert!(token_args.json);
    assert_eq!(token_args.params, ["list"]);

    let args = CliArgs::try_parse_from([
        "pacquet",
        "token",
        "create",
        "--read-only",
        "--cidr",
        "10.0.0.0/8,192.168.0.0/16",
    ])
    .unwrap();
    let CliCommand::Token(token_args) = args.command else {
        panic!("expected Token command");
    };
    assert!(token_args.read_only);
    assert_eq!(token_args.cidr, ["10.0.0.0/8", "192.168.0.0/16"]);
    assert_eq!(token_args.params, ["create"]);
}
