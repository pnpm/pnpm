#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]

mod cli_args;
mod fixtures;
mod latency_proxy;
mod verify;
mod work_env;
mod workspace_manifest;

use cli_args::{RegistryMode, TargetKind};

#[tokio::main]
async fn main() {
    use pipe_trait::Pipe;

    let cli_args::CliArgs {
        scenario,
        registry_port,
        registry: registry_mode,
        repository,
        pnpm_repository,
        fixture_dir,
        hyperfine_options,
        work_env,
        with_pnpm,
        pnpr_latency_ms,
        registry_latency_ms,
        registry_bandwidth_mbps,
        reuse_prebuilt_binaries,
        build_only,
        targets,
    } = clap::Parser::parse();

    let repository = std::fs::canonicalize(&repository).expect("get absolute path to repository");
    let pnpm_repository = pnpm_repository
        .as_ref()
        .map(|path| std::fs::canonicalize(path).expect("get absolute path to pnpm repository"));
    if !work_env.exists() {
        std::fs::create_dir_all(&work_env).expect("create work env");
    }
    let work_env = std::fs::canonicalize(&work_env).expect("get absolute path to work env");
    let registry = match registry_mode {
        RegistryMode::Verdaccio | RegistryMode::Virtual => {
            format!("http://localhost:{registry_port}/")
        }
        RegistryMode::Npm => "https://registry.npmjs.org/".to_string(),
    };

    let verdaccio = if build_only {
        None
    } else {
        match registry_mode {
            RegistryMode::Verdaccio => {
                verify::ensure_program("just")
                    .arg("install")
                    .pipe(verify::executor("just install"));
                pacquet_registry_mock::MockInstanceOptions {
                    client: &Default::default(),
                    port: registry_port,
                    stdout: work_env.join("verdaccio.stdout.log").pipe(Some).as_deref(),
                    stderr: work_env.join("verdaccio.stderr.log").pipe(Some).as_deref(),
                    max_retries: 10,
                    retry_delay: tokio::time::Duration::from_millis(500),
                }
                .spawn_if_necessary()
                .await
            }
            RegistryMode::Virtual => {
                verify::ensure_virtual_registry(&registry).await;
                None
            }
            RegistryMode::Npm => None,
        }
    };

    let has_pacquet_target = targets.iter().any(|target| target.kind == TargetKind::Pacquet);
    let has_pnpm_target = targets.iter().any(|target| target.kind == TargetKind::Pnpm);
    // A pnpr target builds the `pacquet` + `pnpr` binaries from the same
    // monorepo clone a pacquet target uses, so it needs the pacquet repo
    // and cargo just like a pacquet target does.
    let has_pnpr_target = targets.iter().any(|target| target.kind == TargetKind::Pnpr);
    let needs_pacquet_repo = has_pacquet_target || has_pnpr_target;
    if needs_pacquet_repo {
        verify::ensure_pacquet_git_repo(&repository);
    }
    if has_pnpm_target {
        let pnpm_repo = pnpm_repository.as_deref().unwrap_or(&repository);
        verify::ensure_pnpm_git_repo(pnpm_repo);
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

    let env = work_env::WorkEnv {
        root: work_env,
        with_pnpm,
        targets,
        registry,
        registry_mode,
        repository,
        pnpm_repository,
        scenario,
        hyperfine_options,
        fixture_dir,
        pnpr_latency_ms,
        registry_latency_ms,
        registry_bandwidth_mbps,
        registry_port,
        reuse_prebuilt_binaries,
    };
    if build_only {
        env.build();
    } else {
        env.run();
    }
    drop(verdaccio); // terminate verdaccio if exists
}
