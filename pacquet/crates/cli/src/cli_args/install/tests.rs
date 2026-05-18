use super::{InstallArgs, InstallDependencyOptions, NodeLinkerArg};
use clap::Parser;
use pacquet_config::NodeLinker;
use pacquet_package_manifest::DependencyGroup;
use pretty_assertions::assert_eq;

#[test]
fn dependency_options_to_dependency_groups() {
    use DependencyGroup::{Dev, Optional, Prod};
    let create_list = |opts: InstallDependencyOptions| opts.dependency_groups().collect::<Vec<_>>();

    // no flags -> prod + dev + optional
    assert_eq!(
        create_list(InstallDependencyOptions { prod: false, dev: false, no_optional: false }),
        [Prod, Dev, Optional],
    );

    // --prod -> prod + optional
    assert_eq!(
        create_list(InstallDependencyOptions { prod: true, dev: false, no_optional: false }),
        [Prod, Optional],
    );

    // --dev -> dev + optional
    assert_eq!(
        create_list(InstallDependencyOptions { prod: false, dev: true, no_optional: false }),
        [Dev, Optional],
    );

    // --no-optional -> prod + dev
    assert_eq!(
        create_list(InstallDependencyOptions { prod: false, dev: false, no_optional: true }),
        [Prod, Dev],
    );

    // --prod --no-optional -> prod
    assert_eq!(
        create_list(InstallDependencyOptions { prod: true, dev: false, no_optional: true }),
        [Prod],
    );

    // --dev --no-optional -> dev
    assert_eq!(
        create_list(InstallDependencyOptions { prod: false, dev: true, no_optional: true }),
        [Dev],
    );

    // --prod --dev -> prod + dev + optional
    assert_eq!(
        create_list(InstallDependencyOptions { prod: true, dev: true, no_optional: false }),
        [Prod, Dev, Optional],
    );

    // --prod --dev --no-optional -> prod + dev
    assert_eq!(
        create_list(InstallDependencyOptions { prod: true, dev: true, no_optional: true }),
        [Prod, Dev],
    );
}

/// Helper test wrapper so the `--node-linker` parser tests can
/// drive `clap::Parser::try_parse_from` against `InstallArgs`
/// without needing the full `pacquet install` CLI surface.
#[derive(Debug, clap::Parser)]
struct InstallArgsHarness {
    #[clap(flatten)]
    args: InstallArgs,
}

#[test]
fn node_linker_default_is_none() {
    let parsed = InstallArgsHarness::try_parse_from(["pacquet-test"]).expect("parses");
    assert!(parsed.args.node_linker.is_none(), "flag absent → field is None");
}

#[test]
fn node_linker_hoisted() {
    let parsed = InstallArgsHarness::try_parse_from(["pacquet-test", "--node-linker", "hoisted"])
        .expect("parses --node-linker hoisted");
    let resolved = parsed.args.node_linker.expect("flag present").into_config();
    assert_eq!(resolved, NodeLinker::Hoisted);
}

#[test]
fn node_linker_isolated() {
    let parsed = InstallArgsHarness::try_parse_from(["pacquet-test", "--node-linker", "isolated"])
        .expect("parses --node-linker isolated");
    let resolved = parsed.args.node_linker.expect("flag present").into_config();
    assert_eq!(resolved, NodeLinker::Isolated);
}

#[test]
fn node_linker_pnp() {
    let parsed = InstallArgsHarness::try_parse_from(["pacquet-test", "--node-linker", "pnp"])
        .expect("parses --node-linker pnp");
    let resolved = parsed.args.node_linker.expect("flag present").into_config();
    assert_eq!(resolved, NodeLinker::Pnp);
}

/// Unknown values are rejected by clap, matching pnpm's CLI
/// behavior on `--node-linker something-else`. The error contains
/// the bad value so the user can fix the typo without consulting
/// docs.
#[test]
fn node_linker_invalid_value_rejected() {
    let err = InstallArgsHarness::try_parse_from(["pacquet-test", "--node-linker", "bogus"])
        .expect_err("invalid value rejected");
    let msg = err.to_string();
    assert!(msg.contains("bogus"), "error mentions bad value: {msg}");
}

/// `NodeLinkerArg::into_config` maps every variant 1:1 to the
/// canonical `pacquet_config::NodeLinker` enum. Tied to the
/// `ValueEnum` derive's kebab-case rename — if a future variant
/// is added, this test starts failing at compile time as a
/// reminder to update the mapping.
#[test]
fn node_linker_arg_into_config_matches_every_variant() {
    use clap::ValueEnum;
    for variant in NodeLinkerArg::value_variants() {
        let canonical = variant.into_config();
        match (variant, canonical) {
            (NodeLinkerArg::Isolated, NodeLinker::Isolated)
            | (NodeLinkerArg::Hoisted, NodeLinker::Hoisted)
            | (NodeLinkerArg::Pnp, NodeLinker::Pnp) => {}
            other => panic!("mapping mismatch: {other:?}"),
        }
    }
}
