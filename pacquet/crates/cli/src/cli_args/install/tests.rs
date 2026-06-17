use super::{
    BenchmarkRegistryRewrite, InstallArgs, InstallDependencyOptions, NodeLinkerArg,
    PnprBenchmarkRegistryOverride, rewrite_resolution_registry,
};
use clap::Parser;
use pacquet_config::NodeLinker;
use pacquet_lockfile::{LockfileResolution, TarballResolution};
use pacquet_package_manifest::DependencyGroup;
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;

#[test]
fn dependency_options_to_dependency_groups() {
    use DependencyGroup::{Dev, Optional, Prod};
    let create_list = |opts: InstallDependencyOptions| opts.dependency_groups().collect::<Vec<_>>();

    assert_eq!(
        create_list(InstallDependencyOptions { prod: false, dev: false, no_optional: false }),
        [Prod, Dev, Optional],
    );

    assert_eq!(
        create_list(InstallDependencyOptions { prod: true, dev: false, no_optional: false }),
        [Prod, Optional],
    );

    assert_eq!(
        create_list(InstallDependencyOptions { prod: false, dev: true, no_optional: false }),
        [Dev, Optional],
    );

    assert_eq!(
        create_list(InstallDependencyOptions { prod: false, dev: false, no_optional: true }),
        [Prod, Dev],
    );

    assert_eq!(
        create_list(InstallDependencyOptions { prod: true, dev: false, no_optional: true }),
        [Prod],
    );

    assert_eq!(
        create_list(InstallDependencyOptions { prod: false, dev: true, no_optional: true }),
        [Dev],
    );

    assert_eq!(
        create_list(InstallDependencyOptions { prod: true, dev: true, no_optional: false }),
        [Prod, Dev, Optional],
    );

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

#[test]
fn node_linker_invalid_value_rejected() {
    let err = InstallArgsHarness::try_parse_from(["pacquet-test", "--node-linker", "bogus"])
        .expect_err("invalid value rejected");
    let msg = err.to_string();
    assert!(msg.contains("bogus"), "error mentions bad value: {msg}");
}

#[test]
fn ignore_manifest_check_flag_parses() {
    let parsed = InstallArgsHarness::try_parse_from(["pacquet-test"]).expect("parses");
    assert!(!parsed.args.ignore_manifest_check, "flag absent → false");

    let parsed = InstallArgsHarness::try_parse_from(["pacquet-test", "--ignore-manifest-check"])
        .expect("parses --ignore-manifest-check");
    assert!(parsed.args.ignore_manifest_check, "flag present → true");
}

#[test]
fn dry_run_flag_parses() {
    let parsed = InstallArgsHarness::try_parse_from(["pacquet-test"]).expect("parses");
    assert!(!parsed.args.dry_run, "flag absent → false");

    let parsed = InstallArgsHarness::try_parse_from(["pacquet-test", "--dry-run"])
        .expect("parses --dry-run");
    assert!(parsed.args.dry_run, "flag present → true");
}

/// `--frozen-store` parses to `true`. Absent → `false`. The flag is
/// folded into `config.frozen_store` at the dispatch in `cli_args.rs`
/// (any `--frozen-store` upgrades a yaml `false` to `true`), so the
/// install path reads the effective value off the config.
#[test]
fn frozen_store_flag_parses() {
    let parsed = InstallArgsHarness::try_parse_from(["pacquet-test"]).expect("parses");
    assert!(!parsed.args.frozen_store, "flag absent → false");

    let parsed = InstallArgsHarness::try_parse_from(["pacquet-test", "--frozen-store"])
        .expect("parses --frozen-store");
    assert!(parsed.args.frozen_store, "flag present → true");
}

#[test]
fn workspace_concurrency_default_is_none() {
    let parsed = ["pacquet-test"].pipe(InstallArgsHarness::try_parse_from).expect("parses");
    assert_eq!(parsed.args.workspace_concurrency, None, "flag absent → None");
}

#[test]
fn workspace_concurrency_parses_positive() {
    let parsed = ["pacquet-test", "--workspace-concurrency", "3"]
        .pipe(InstallArgsHarness::try_parse_from)
        .expect("parses --workspace-concurrency 3");
    assert_eq!(parsed.args.workspace_concurrency, Some(3));
}

#[test]
fn workspace_concurrency_parses_negative() {
    let parsed = ["pacquet-test", "--workspace-concurrency=-1"]
        .pipe(InstallArgsHarness::try_parse_from)
        .expect("parses --workspace-concurrency=-1");
    assert_eq!(parsed.args.workspace_concurrency, Some(-1));
}

#[test]
fn resolve_workspace_concurrency_keeps_config_value_when_flag_absent() {
    let args = ["pacquet-test"].pipe(InstallArgsHarness::try_parse_from).expect("parses").args;
    assert_eq!(args.resolve_workspace_concurrency(7), 7);
}

#[test]
fn resolve_workspace_concurrency_positive_flag_overrides_config() {
    let args = ["pacquet-test", "--workspace-concurrency", "3"]
        .pipe(InstallArgsHarness::try_parse_from)
        .expect("parses")
        .args;
    assert_eq!(args.resolve_workspace_concurrency(7), 3);
}

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

#[test]
fn registry_rewrite_replaces_only_the_configured_registry_prefix() {
    let rewrite = BenchmarkRegistryRewrite::new(
        ["http://server-registry.test"],
        "http://client-registry.test",
    )
    .expect("different registries create a rewrite");

    assert_eq!(
        rewrite.url("http://server-registry.test/foo/-/foo-1.0.0.tgz"),
        "http://client-registry.test/foo/-/foo-1.0.0.tgz",
    );
    assert_eq!(
        rewrite.url("http://other-registry.test/foo/-/foo-1.0.0.tgz"),
        "http://other-registry.test/foo/-/foo-1.0.0.tgz",
    );
}

#[test]
fn registry_rewrite_accepts_multiple_server_registry_prefixes() {
    let rewrite = BenchmarkRegistryRewrite::new(
        ["http://server-proxy.test", "http://server-registry.test"],
        "http://client-registry.test",
    )
    .expect("different registries create a rewrite");

    assert_eq!(
        rewrite.url("http://server-proxy.test/foo/-/foo-1.0.0.tgz"),
        "http://client-registry.test/foo/-/foo-1.0.0.tgz",
    );
    assert_eq!(
        rewrite.url("http://server-registry.test/foo/-/foo-1.0.0.tgz"),
        "http://client-registry.test/foo/-/foo-1.0.0.tgz",
    );
}

#[test]
fn registry_rewrite_is_none_for_equal_registries_after_normalization() {
    assert!(
        BenchmarkRegistryRewrite::new(["http://registry.test"], "http://registry.test/").is_none(),
    );
}

#[test]
fn registry_rewrite_updates_explicit_tarball_resolution_urls() {
    let rewrite = BenchmarkRegistryRewrite::new(
        ["http://server-registry.test"],
        "http://client-registry.test",
    )
    .expect("different registries create a rewrite");
    let mut resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "http://server-registry.test/foo/-/foo-1.0.0.tgz".to_string(),
        integrity: None,
        git_hosted: None,
        path: None,
    });

    rewrite_resolution_registry(&mut resolution, &rewrite);

    let LockfileResolution::Tarball(resolution) = resolution else {
        panic!("resolution stays tarball");
    };
    assert_eq!(resolution.tarball, "http://client-registry.test/foo/-/foo-1.0.0.tgz");
}

#[test]
fn pnpr_benchmark_override_keeps_resolve_registry_separate_from_tarball_rewrite() {
    let override_ = PnprBenchmarkRegistryOverride {
        resolve_registry: "http://server-proxy.test/".to_string(),
        tarball_rewrite: BenchmarkRegistryRewrite::new(
            ["http://server-proxy.test", "http://server-registry.test"],
            "http://client-registry.test",
        ),
    };

    assert_eq!(override_.resolve_registry(), "http://server-proxy.test/");
    assert_eq!(
        override_.client_tarball_url("http://server-registry.test/foo/-/foo-1.0.0.tgz"),
        "http://client-registry.test/foo/-/foo-1.0.0.tgz",
    );
}
