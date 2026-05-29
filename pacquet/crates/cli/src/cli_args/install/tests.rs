use super::{InstallArgs, InstallDependencyOptions, NodeLinkerArg};
use clap::Parser;
use pacquet_config::NodeLinker;
use pacquet_package_manifest::DependencyGroup;
use pipe_trait::Pipe;
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

/// `--ignore-manifest-check` parses to `true`. Absent → `false`.
/// Surfaced for the pnpm CLI `configDependencies` delegation path
/// (issue [#11797](https://github.com/pnpm/pnpm/issues/11797)); see the field doc on `InstallArgs::ignore_manifest_check`.
#[test]
fn ignore_manifest_check_flag_parses() {
    let parsed = InstallArgsHarness::try_parse_from(["pacquet-test"]).expect("parses");
    assert!(!parsed.args.ignore_manifest_check, "flag absent → false");

    let parsed = InstallArgsHarness::try_parse_from(["pacquet-test", "--ignore-manifest-check"])
        .expect("parses --ignore-manifest-check");
    assert!(parsed.args.ignore_manifest_check, "flag present → true");
}

/// `--workspace-concurrency` is absent by default, so the override
/// is `None` and the config-resolved value stays in effect.
#[test]
fn workspace_concurrency_default_is_none() {
    let parsed = ["pacquet-test"].pipe(InstallArgsHarness::try_parse_from).expect("parses");
    assert_eq!(parsed.args.workspace_concurrency, None, "flag absent → None");
}

/// A positive `--workspace-concurrency` parses to its value verbatim.
#[test]
fn workspace_concurrency_parses_positive() {
    let parsed = ["pacquet-test", "--workspace-concurrency", "3"]
        .pipe(InstallArgsHarness::try_parse_from)
        .expect("parses --workspace-concurrency 3");
    assert_eq!(parsed.args.workspace_concurrency, Some(3));
}

/// A negative `--workspace-concurrency` parses to the signed value;
/// the `parallelism - |value|` interpretation happens later at the
/// CLI dispatch via `resolve_child_concurrency`. Mirrors pnpm
/// accepting `--workspace-concurrency=-1`.
#[test]
fn workspace_concurrency_parses_negative() {
    let parsed = ["pacquet-test", "--workspace-concurrency=-1"]
        .pipe(InstallArgsHarness::try_parse_from)
        .expect("parses --workspace-concurrency=-1");
    assert_eq!(parsed.args.workspace_concurrency, Some(-1));
}

/// No `--workspace-concurrency` flag → the already-resolved config
/// value passes through untouched.
#[test]
fn resolve_workspace_concurrency_keeps_config_value_when_flag_absent() {
    let args = ["pacquet-test"].pipe(InstallArgsHarness::try_parse_from).expect("parses").args;
    assert_eq!(args.resolve_workspace_concurrency(7), 7);
}

/// A positive `--workspace-concurrency` replaces the config value
/// verbatim (it does not fall through to `config_value`).
#[test]
fn resolve_workspace_concurrency_positive_flag_overrides_config() {
    let args = ["pacquet-test", "--workspace-concurrency", "3"]
        .pipe(InstallArgsHarness::try_parse_from)
        .expect("parses")
        .args;
    assert_eq!(args.resolve_workspace_concurrency(7), 3);
}

/// A non-positive `--workspace-concurrency` resolves to
/// `max(1, parallelism - |value|)` via `getWorkspaceConcurrency`,
/// independent of the config value. Pinned exactly against the host's
/// reported parallelism.
#[test]
fn resolve_workspace_concurrency_negative_flag_resolves_to_offset() {
    let args = ["pacquet-test", "--workspace-concurrency=-1"]
        .pipe(InstallArgsHarness::try_parse_from)
        .expect("parses")
        .args;
    let expected = pacquet_config::available_parallelism().saturating_sub(1).max(1);
    assert_eq!(args.resolve_workspace_concurrency(7), expected);
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
