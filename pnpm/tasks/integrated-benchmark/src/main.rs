#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]

mod cli_args;
mod fixtures;
mod latency_proxy;
mod verify;
mod work_env;
mod workspace_manifest;

use cli_args::{RegistryMode, TargetKind};
use latency_proxy::{LatencyProxy, LinkProfile, mbps_to_bytes_per_sec};
use std::{
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};

#[tokio::main]
async fn main() {
    let args: cli_args::CliArgs = clap::Parser::parse();

    let repository =
        std::fs::canonicalize(&args.repository).expect("get absolute path to repository");
    let pnpm_repository = args
        .pnpm_repository
        .as_ref()
        .map(|path| std::fs::canonicalize(path).expect("get absolute path to pnpm repository"));
    let work_env = prepared_work_env(&args.work_env);
    let registry = RegistryLink::plan(&args);

    if !args.build_only {
        seed_scenario_fixture(args.scenario, args.registry);
    }

    let verdaccio = benchmark_registry(&args, &registry, &work_env).await;
    let registry_proxy = registry_proxy(&args, &registry);

    verify_prerequisites(
        &args.targets,
        &repository,
        pnpm_repository.as_deref(),
        args.with_pnpm,
        args.registry,
    );

    let build_only = args.build_only;
    let env = configured_work_env(args, registry, work_env, repository, pnpm_repository);
    if build_only {
        env.build();
    } else {
        env.run();
    }
    drop(registry_proxy);
    drop(verdaccio); // terminate verdaccio if exists
}

async fn benchmark_registry(
    args: &cli_args::CliArgs,
    registry: &RegistryLink,
    work_env: &std::path::Path,
) -> Option<pnpm_registry_mock::MockInstance> {
    if args.build_only {
        None
    } else {
        spawn_registry(SpawnRegistry {
            registry_mode: args.registry,
            registry: &registry.url,
            work_env,
            spawned_registry_port: registry.spawned_port,
            public_url: registry.proxied.then_some(registry.public_url.as_str()),
        })
        .await
    }
}

fn configured_work_env(
    args: cli_args::CliArgs,
    registry: RegistryLink,
    work_env: std::path::PathBuf,
    repository: std::path::PathBuf,
    pnpm_repository: Option<std::path::PathBuf>,
) -> work_env::WorkEnv {
    work_env::WorkEnv {
        root: work_env,
        with_pnpm: args.with_pnpm,
        targets: args.targets,
        registry: registry.url,
        registry_cache_populator: registry.cache_populator,
        registry_mode: args.registry,
        repository,
        pnpm_repository,
        scenario: args.scenario,
        hyperfine_options: args.hyperfine_options,
        fixture_dir: args.fixture_dir,
        pnpr_latency_ms: args.pnpr_latency_ms,
        registry_latency_ms: args.registry_latency_ms,
        pnpr_server_registry_latency_ms: args.pnpr_server_registry_latency_ms,
        registry_bandwidth_mbps: args.registry_bandwidth_mbps,
        registry_slow_start: args.registry_slow_start,
        registry_port: registry.spawned_port,
        reuse_prebuilt_binaries: args.reuse_prebuilt_binaries,
        serve_timing: args.serve_timing,
    }
}

fn registry_proxy(args: &cli_args::CliArgs, registry: &RegistryLink) -> Option<LatencyProxy> {
    registry.proxied.then(|| {
        spawn_registry_proxy(
            args.registry_port,
            registry.spawned_port,
            LinkProfile {
                one_way: Duration::from_millis(args.registry_latency_ms) / 2,
                rate_limit: registry.rate_limit,
                slow_start: args.registry_slow_start,
            },
            args.registry_latency_ms,
            args.registry_bandwidth_mbps,
        )
    })
}

/// Where the clients are pointed, and the mock behind the latency proxy
/// when one fronts it.
struct RegistryLink {
    url: String,
    rate_limit: Option<u64>,
    /// A latency or bandwidth profile puts a proxy in front of the mock.
    proxied: bool,
    spawned_port: u16,
    cache_populator: String,
    public_url: String,
}

impl RegistryLink {
    fn plan(args: &cli_args::CliArgs) -> Self {
        let url = registry_url(args.registry, args.registry_port);
        let rate_limit = mbps_to_bytes_per_sec(args.registry_bandwidth_mbps);
        let proxied = !args.build_only
            && matches!(args.registry, RegistryMode::Verdaccio)
            && (args.registry_latency_ms > 0 || rate_limit.is_some());
        let (spawned_port, cache_populator) = upstream_registry(proxied, args.registry_port, &url);
        let public_url = url.trim_end_matches('/').to_string();
        RegistryLink { url, rate_limit, proxied, spawned_port, cache_populator, public_url }
    }
}

/// What the mock registry needs to come up.
struct SpawnRegistry<'a> {
    registry_mode: RegistryMode,
    registry: &'a str,
    work_env: &'a std::path::Path,
    spawned_registry_port: u16,
    /// Set when a latency proxy fronts the registry, so the mock advertises
    /// the proxy's URL rather than its own.
    public_url: Option<&'a str>,
}

async fn spawn_registry(opts: SpawnRegistry<'_>) -> Option<pnpm_registry_mock::MockInstance> {
    use pipe_trait::Pipe;

    match opts.registry_mode {
        RegistryMode::Verdaccio => {
            verify::ensure_program("just").arg("install").pipe(verify::executor("just install"));
            pnpm_registry_mock::MockInstanceOptions {
                client: &reqwest::Client::default(),
                port: opts.spawned_registry_port,
                public_url: opts.public_url,
                stdout: opts.work_env.join("verdaccio.stdout.log").pipe(Some).as_deref(),
                stderr: opts.work_env.join("verdaccio.stderr.log").pipe(Some).as_deref(),
                max_retries: 10,
                retry_delay: Duration::from_millis(500),
            }
            .spawn_if_necessary()
            .await
        }
        RegistryMode::Virtual => {
            verify::ensure_virtual_registry(opts.registry).await;
            None
        }
        RegistryMode::Npm => None,
    }
}

fn spawn_registry_proxy(
    listen_port: u16,
    upstream_port: u16,
    profile: LinkProfile,
    registry_latency_ms: u64,
    registry_bandwidth_mbps: f64,
) -> LatencyProxy {
    let upstream = SocketAddr::from((Ipv4Addr::LOCALHOST, upstream_port));
    let listen = SocketAddr::from((Ipv4Addr::LOCALHOST, listen_port));
    let proxy = LatencyProxy::spawn_on(listen, upstream, profile).expect("spawn registry proxy");
    let cap = if registry_bandwidth_mbps > 0.0 {
        format!("{registry_bandwidth_mbps} Mbit/s")
    } else {
        "no".to_string()
    };
    eprintln!(
        "Fronting the registry with {registry_latency_ms}ms round-trip latency + {cap} download cap (proxy at {})",
        proxy.addr,
    );
    proxy
}

/// Fail early on a missing repository, revision or program, before any
/// benchmark work starts.
fn verify_prerequisites(
    targets: &[cli_args::TargetSpec],
    repository: &std::path::Path,
    pnpm_repository: Option<&std::path::Path>,
    with_pnpm: bool,
    registry_mode: RegistryMode,
) {
    let has_pacquet_target = targets.iter().any(|target| target.kind == TargetKind::Pacquet);
    let has_pnpm_target = targets.iter().any(|target| target.kind == TargetKind::Pnpm);
    // A pnpr target builds the `pacquet` + `pnpr` binaries from the same
    // monorepo clone a pacquet target uses, so it needs the pacquet repo
    // and cargo just like a pacquet target does.
    let has_pnpr_target = targets.iter().any(|target| target.kind == TargetKind::Pnpr);
    let needs_pacquet_repo = has_pacquet_target || has_pnpr_target;
    if needs_pacquet_repo {
        verify::ensure_pacquet_git_repo(repository);
    }
    if has_pnpm_target {
        verify::ensure_pnpm_git_repo(pnpm_repository.unwrap_or(repository));
    }
    verify::validate_revision_list(targets.iter().map(|target| target.rev.as_str()));
    verify::ensure_program("bash");
    verify::ensure_program("git");
    verify::ensure_program("hyperfine");
    if needs_pacquet_repo {
        verify::ensure_program("cargo");
    }
    // `pnpm` is needed by pnpm targets (build script invokes `pnpm install`
    // and `pnpm run compile`), by `--with-pnpm` (the system pnpm bench
    // target), and by the proxy-cache populator that runs whenever the
    // registry is verdaccio or virtual (its `install.bash` shells out to
    // `pnpm install` to warm the cache).
    let needs_pnpm = has_pnpm_target
        || with_pnpm
        || matches!(registry_mode, RegistryMode::Verdaccio | RegistryMode::Virtual);
    if needs_pnpm {
        verify::ensure_program("pnpm");
    }
    if has_pnpm_target {
        verify::ensure_program("node");
    }
}

/// A scenario with its own fixture packages seeds them into the mock
/// registry's storage before it comes up.
fn seed_scenario_fixture(
    scenario: Option<cli_args::BenchmarkScenario>,
    registry_mode: RegistryMode,
) {
    if !scenario.is_some_and(cli_args::BenchmarkScenario::uses_peer_heavy_fixture) {
        return;
    }
    assert!(
        matches!(registry_mode, RegistryMode::Verdaccio),
        "the peer-heavy benchmark requires --registry=verdaccio",
    );
    work_env::seed_peer_heavy_registry(pnpm_registry_mock::runtime_storage());
}

fn prepared_work_env(work_env: &std::path::Path) -> std::path::PathBuf {
    if !work_env.exists() {
        std::fs::create_dir_all(work_env).expect("create work env");
    }
    std::fs::canonicalize(work_env).expect("get absolute path to work env")
}

/// Where clients are pointed: the local mock (or the proxy in front of it),
/// or the public npm registry.
fn registry_url(registry_mode: RegistryMode, registry_port: u16) -> String {
    match registry_mode {
        RegistryMode::Verdaccio | RegistryMode::Virtual => {
            format!("http://localhost:{registry_port}/")
        }
        RegistryMode::Npm => "https://registry.npmjs.org/".to_string(),
    }
}

/// Where the mock registry itself listens, and the URL the cache populator
/// talks to.
///
/// Behind a proxy the mock takes a port of its own and the proxy takes the
/// one clients are pointed at; the populator then talks to the mock
/// directly, since warming the cache through the delay is pure wait.
fn upstream_registry(
    proxy_spawned_registry: bool,
    registry_port: u16,
    registry: &str,
) -> (u16, String) {
    if !proxy_spawned_registry {
        return (registry_port, registry.to_string());
    }
    let port = pnpm_registry_mock::pick_unused_port()
        .expect("pick an unused port for the registry upstream");
    (port, format!("http://localhost:{port}/"))
}
